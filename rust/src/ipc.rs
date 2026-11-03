//! i3/sway IPC client: fetch the running window manager's config, with the
//! contents of every file it includes.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

use gtk::gio;
use gtk::gio::prelude::*;

use crate::error_message;
use crate::glibstr;
use crate::json::{self, Json};

const MAGIC: &[u8; 6] = b"i3-ipc";
const GET_VERSION: u32 = 7;
const GET_CONFIG: u32 = 9;
/// Largest single read request; the payload is read in as many as needed.
const READ_CHUNK: usize = 64 * 1024;

/// A connection to the i3 or sway IPC socket.
pub struct Client {
    socket: gio::Socket,
    input: gio::DataInputStream,
    // Owns the stream `input` reads from.
    _connection: gio::SocketConnection,
}

impl Client {
    pub fn connect(socket_path: &Path) -> Result<Self, String> {
        let address = gio::UnixSocketAddress::new(socket_path);
        let socket = gio::Socket::new(
            gio::SocketFamily::Unix,
            gio::SocketType::Stream,
            gio::SocketProtocol::Default,
        )
        .map_err(error_message)?;
        SocketExt::connect(&socket, &address, gio::Cancellable::NONE).map_err(error_message)?;
        socket.set_blocking(true);

        let connection = socket.connection_factory_create_connection();
        let input = gio::DataInputStream::new(&connection.input_stream());
        input.set_byte_order(gio::DataStreamByteOrder::HostEndian);
        Ok(Self {
            socket,
            input,
            _connection: connection,
        })
    }

    /// Send one request with an empty payload and read the complete reply,
    /// however many chunks it arrives in. `None` for an empty reply.
    fn request(&self, command: u32) -> Result<Option<Json>, String> {
        let mut message = Vec::with_capacity(14);
        message.extend_from_slice(MAGIC);
        message.extend_from_slice(&0u32.to_ne_bytes());
        message.extend_from_slice(&command.to_ne_bytes());
        let sent = self
            .socket
            .send(&message, gio::Cancellable::NONE)
            .map_err(error_message)?;
        glib_debug(&format!("Sent {sent} bytes to i3.\n"));

        for &expected in MAGIC {
            let byte = DataInputStreamExt::read_byte(&self.input, gio::Cancellable::NONE)
                .map_err(error_message)?;
            if byte != expected {
                return Err("Invalid reply header".to_owned());
            }
        }
        let length = self
            .input
            .read_uint32(gio::Cancellable::NONE)
            .map_err(error_message)?;
        self.input
            .read_uint32(gio::Cancellable::NONE)
            .map_err(error_message)?;

        let length = usize::try_from(length).map_err(|_| "Reply too large".to_owned())?;
        let mut payload = Vec::with_capacity(length.min(READ_CHUNK));
        while payload.len() < length {
            let wanted = (length - payload.len()).min(READ_CHUNK);
            let chunk = self
                .input
                .read_bytes(wanted, gio::Cancellable::NONE)
                .map_err(error_message)?;
            if chunk.is_empty() {
                return Err(format!(
                    "Connection closed after {} of {} bytes",
                    payload.len(),
                    length
                ));
            }
            payload.extend_from_slice(&chunk);
        }
        glib_debug(&format!("Received {} bytes from i3.\n", payload.len()));

        json::parse(&payload).map_err(|e| e.to_string())
    }

    /// The config text: the main config plus every file it includes.
    pub fn config(&self) -> Result<Vec<u8>, String> {
        let reply = self
            .request(GET_CONFIG)?
            .ok_or_else(|| "No Response".to_owned())?;
        if reply.as_object().is_none() {
            return Err("Invalid reply: expected a JSON object".to_owned());
        }

        // i3 sends the main config and every file it includes, in the order
        // it read them.
        if reply.has_member("included_configs") {
            return Ok(i3_config(&reply));
        }

        // sway sends only the main config: read the files it includes.
        // Relative include paths start from the main config's directory.
        let mut config_dir = glib_current_dir();
        if let Some(version) = self.request(GET_VERSION)? {
            if let Some(main) = version.string_member("loaded_config_file_name") {
                config_dir = glibstr::dirname(main.as_bytes());
            }
        }
        let main = reply.string_member("config").unwrap_or_default();
        let main = glibstr::until_nul(main.as_bytes());
        let mut read_files = HashSet::new();
        let mut config = Vec::new();
        append_config(&mut config, main, &config_dir, &mut read_files, None, true);
        Ok(config)
    }
}

/// The i3 shape: `included_configs` holds `{path, raw_contents}` per file.
/// Once one included file has a config error, such as a duplicate binding,
/// i3 leaves it and the files included after it out, although it still
/// loads their bindings: walk the main config's includes for those.
fn i3_config(reply: &Json) -> Vec<u8> {
    let entries = reply
        .as_object()
        .and_then(|members| members.get("included_configs"))
        .and_then(Json::as_array)
        .unwrap_or_default();

    let mut config = Vec::new();
    let mut sent_files = HashSet::new();
    let mut main: Option<(Vec<u8>, Vec<u8>)> = None;
    for entry in entries {
        let contents = glibstr::until_nul(
            entry
                .string_member("raw_contents")
                .unwrap_or_default()
                .as_bytes(),
        )
        .to_vec();
        let path =
            glibstr::until_nul(entry.string_member("path").unwrap_or_default().as_bytes()).to_vec();
        if main.is_none() {
            main = Some((contents.clone(), path.clone()));
        }
        sent_files.insert(path);
        config.extend_from_slice(&contents);
        if !contents.ends_with(b"\n") {
            config.push(b'\n');
        }
    }

    if let Some((main_config, main_path)) = main {
        let mut read_files = HashSet::from([real_path(&main_path)]);
        append_config(
            &mut config,
            &main_config,
            &glibstr::dirname(&main_path),
            &mut read_files,
            Some(&sent_files),
            false,
        );
    }
    config
}

/// Append `config` to `result` if `show`, and after each of its include
/// lines the files it includes, recursively, as sway reads them. Files are
/// identified by their real path, as i3 and sway do, and read once. The
/// files in `sent_files` are already in `result`: only the files they
/// include are added.
fn append_config(
    result: &mut Vec<u8>,
    config: &[u8],
    config_dir: &[u8],
    read_files: &mut HashSet<Vec<u8>>,
    sent_files: Option<&HashSet<Vec<u8>>>,
    show: bool,
) {
    for line in config_lines(config) {
        if show {
            result.extend_from_slice(line);
            result.push(b'\n');
        }

        let stripped = glibstr::strip(line);
        if !stripped.starts_with(b"include ") && !stripped.starts_with(b"include\t") {
            continue;
        }

        let pattern = expand_path(glibstr::strip(&stripped[8..]), config_dir);
        for found in find_files(&pattern) {
            let path = real_path(&found);
            if !read_files.insert(path.clone()) {
                continue;
            }
            let Ok(contents) = std::fs::read(OsStr::from_bytes(&path)) else {
                continue;
            };
            let sent = sent_files.is_some_and(|sent| sent.contains(&path));
            append_config(
                result,
                glibstr::until_nul(&contents),
                &glibstr::dirname(&path),
                read_files,
                sent_files,
                !sent,
            );
        }
    }
}

/// The lines of `config`, without their `\n`; no empty line after a final
/// newline.
fn config_lines(config: &[u8]) -> impl Iterator<Item = &[u8]> {
    let body = config.strip_suffix(b"\n").unwrap_or(config);
    body.split(|&b| b == b'\n')
        .take(if config.is_empty() { 0 } else { usize::MAX })
}

/// The absolute path of a file with symbolic links resolved, or `path` if
/// it does not exist.
fn real_path(path: &[u8]) -> Vec<u8> {
    std::fs::canonicalize(OsStr::from_bytes(path))
        .map_or_else(|_| path.to_vec(), |real| real.into_os_string().into_vec())
}

/// Expand `~`, `$VAR` and `${VAR}`, and resolve a relative path from
/// `config_dir`. A malformed `$` stays literal.
fn expand_path(path: &[u8], config_dir: &[u8]) -> Vec<u8> {
    let mut expanded = path.to_vec();
    if expanded == b"~" || expanded.starts_with(b"~/") {
        let mut home = glib_home_dir();
        home.extend_from_slice(&expanded[1..]);
        expanded = home;
    }

    let is_name = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut result = Vec::with_capacity(expanded.len());
    let mut i = 0;
    while i < expanded.len() {
        if expanded[i] != b'$' {
            result.push(expanded[i]);
            i += 1;
            continue;
        }
        let braced = expanded.get(i + 1) == Some(&b'{');
        let name_start = if braced { i + 2 } else { i + 1 };
        let name_end = name_start
            + expanded
                .get(name_start..)
                .unwrap_or_default()
                .iter()
                .take_while(|&&b| is_name(b))
                .count();
        if name_end == name_start || (braced && expanded.get(name_end) != Some(&b'}')) {
            result.push(expanded[i]);
            i += 1;
            continue;
        }
        let name = OsStr::from_bytes(&expanded[name_start..name_end]);
        if let Some(value) = std::env::var_os(name) {
            result.extend_from_slice(value.as_bytes());
        }
        i = if braced { name_end + 1 } else { name_end };
    }

    if glibstr::is_absolute(&result) {
        result
    } else {
        glibstr::build_filename(&[config_dir, &result])
    }
}

/// The files matching `path`, sorted; `path` may have `*` and `?` wildcards
/// in its last component. Hidden files never match a wildcard.
fn find_files(path: &[u8]) -> Vec<Vec<u8>> {
    let name = glibstr::basename(path);
    if !name.contains(&b'*') && !name.contains(&b'?') {
        return vec![path.to_vec()];
    }

    let dir_path = glibstr::dirname(path);
    let Ok(entries) = std::fs::read_dir(OsStr::from_bytes(&dir_path)) else {
        // No such directory: nothing is included.
        return Vec::new();
    };
    let mut files: Vec<Vec<u8>> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().into_vec())
        .filter(|entry| !entry.starts_with(b".") && glibstr::pattern_match_simple(&name, entry))
        .map(|entry| glibstr::build_filename(&[&dir_path, &entry]))
        .collect();
    files.sort();
    files
}

fn glib_home_dir() -> Vec<u8> {
    gtk::glib::home_dir().into_os_string().into_vec()
}

fn glib_current_dir() -> Vec<u8> {
    gtk::glib::current_dir().into_os_string().into_vec()
}

/// A debug message through GLib logging, shown with `G_MESSAGES_DEBUG`.
fn glib_debug(message: &str) {
    gtk::glib::g_log!(gtk::glib::LogLevel::Debug, "{}", message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("remontoire-ipc-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir.canonicalize().unwrap())
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        fn bytes(&self) -> Vec<u8> {
            self.0.clone().into_os_string().into_vec()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn walk(config: &str, dir: &[u8]) -> String {
        let mut result = Vec::new();
        append_config(
            &mut result,
            config.as_bytes(),
            dir,
            &mut HashSet::new(),
            None,
            true,
        );
        String::from_utf8(result).unwrap()
    }

    #[test]
    fn lines_split_like_the_walk() {
        let lines: Vec<&[u8]> = config_lines(b"a\n\nb").collect();
        assert_eq!(lines, [&b"a"[..], b"", b"b"]);
        assert_eq!(config_lines(b"a\n").count(), 1);
        assert_eq!(config_lines(b"").count(), 0);
        assert_eq!(config_lines(b"\n").count(), 1);
    }

    #[test]
    fn includes_are_inserted_after_their_line_once() {
        let dir = TempDir::new("walk");
        fs::create_dir(dir.path("parts")).unwrap();
        fs::write(dir.path("parts/20-b.conf"), "B\n").unwrap();
        fs::write(dir.path("parts/10-a.conf"), "A\ninclude 10-a.conf\n").unwrap();
        fs::write(dir.path("parts/.hidden.conf"), "H\n").unwrap();
        fs::write(dir.path("parts/30-long.conf"), "L\n").unwrap();
        symlink(dir.path("parts/10-a.conf"), dir.path("alias.conf")).unwrap();
        let out = walk(
            "M1\n  include\tparts/??-?.conf\ninclude alias.conf\nM2",
            &dir.bytes(),
        );
        assert_eq!(
            out,
            "M1\n  include\tparts/??-?.conf\nA\ninclude 10-a.conf\nB\ninclude alias.conf\nM2\n"
        );
    }

    #[test]
    fn variables_and_home_expand() {
        let dir = TempDir::new("vars");
        let base = dir.bytes();
        std::env::set_var("REMONTOIRE_TEST_DIR", OsStr::from_bytes(&base));
        let mut expected = base.clone();
        expected.extend_from_slice(b"/x");
        assert_eq!(expand_path(b"$REMONTOIRE_TEST_DIR/x", b"/"), expected);
        assert_eq!(expand_path(b"${REMONTOIRE_TEST_DIR}/x", b"/"), expected);
        assert_eq!(expand_path(b"$", b"/d"), b"/d/$");
        assert_eq!(
            expand_path(b"${REMONTOIRE_TEST_DIR", b"/d"),
            b"/d/${REMONTOIRE_TEST_DIR"
        );
        assert_eq!(expand_path(b"a$-b", b"/d"), b"/d/a$-b");
        assert_eq!(expand_path(b"$REMONTOIRE_UNSET_VARIABLE_X/y", b"/d"), b"/y");
        let mut home = glib_home_dir();
        home.extend_from_slice(b"/h.conf");
        assert_eq!(expand_path(b"~/h.conf", b"/d"), home);
        assert_eq!(expand_path(b"~x", b"/d"), b"/d/~x");
    }

    #[test]
    fn missing_directories_and_files_are_skipped() {
        assert!(find_files(b"/nonexistent-remontoire/*.conf").is_empty());
        assert_eq!(
            walk("include /nonexistent-remontoire.conf\nK", b"/"),
            "include /nonexistent-remontoire.conf\nK\n"
        );
    }

    #[test]
    fn i3_reply_adds_files_it_left_out() {
        let dir = TempDir::new("i3");
        let main = dir.path("main.conf");
        fs::write(dir.path("a.conf"), "disk A\ninclude b.conf\n").unwrap();
        fs::write(dir.path("b.conf"), "disk B\n").unwrap();
        fs::write(dir.path("c.conf"), "disk C\n").unwrap();
        let reply = Json::Object(
            [(
                "included_configs".to_owned(),
                Json::Array(vec![
                    entry(&main, "main\ninclude a.conf\ninclude c.conf"),
                    entry(&dir.path("a.conf"), "ipc A\ninclude b.conf\n"),
                ]),
            )]
            .into(),
        );
        let out = String::from_utf8(i3_config(&reply)).unwrap();
        assert_eq!(
            out,
            "main\ninclude a.conf\ninclude c.conf\nipc A\ninclude b.conf\ndisk B\ndisk C\n"
        );
    }

    fn entry(path: &Path, contents: &str) -> Json {
        Json::Object(
            [
                (
                    "path".to_owned(),
                    Json::String(path.to_str().unwrap().to_owned()),
                ),
                ("raw_contents".to_owned(), Json::String(contents.to_owned())),
            ]
            .into(),
        )
    }
}

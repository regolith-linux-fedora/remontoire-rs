//! Reading the config text from a file (`-c`) or standard input (`-i`).
//!
//! Both read the bytes literally and never follow includes. As in the
//! original, the text is handled as C strings: a NUL byte ends its line
//! (file) or its `fgets` chunk (stdin).

use std::io::Read;
use std::path::Path;

use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;

use crate::glibstr;

/// Size of the `fgets` buffer the standard input was read with.
const STDIN_CHUNK: usize = 1024;

/// Why a config or stylesheet could not be loaded.
#[derive(Debug)]
pub enum LoadError {
    /// The path does not exist; holds the path as GIO resolves it.
    Missing(String),
    /// Any other failure, with its message.
    Failed(String),
}

impl From<glib::Error> for LoadError {
    fn from(error: glib::Error) -> Self {
        Self::Failed(crate::error_message(error))
    }
}

/// `path` as `g_file_new_for_path` normalizes it (absolute, no `.`/`..`),
/// failing with [`LoadError::Missing`] if it does not exist.
pub fn existing_file(path: &Path) -> Result<gio::File, LoadError> {
    let file = gio::File::for_path(path);
    if file.query_exists(gio::Cancellable::NONE) {
        Ok(file)
    } else {
        let shown = file
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        Err(LoadError::Missing(shown))
    }
}

/// The lines of the file at `path`, each followed by `\n`.
pub fn read_file_config(path: &Path) -> Result<Vec<u8>, LoadError> {
    let file = existing_file(path)?;
    let stream = file.read(gio::Cancellable::NONE)?;
    let mut data = Vec::new();
    loop {
        let chunk = stream.read_bytes(64 * 1024, gio::Cancellable::NONE)?;
        if chunk.is_empty() {
            break;
        }
        data.extend_from_slice(&chunk);
    }
    Ok(file_lines(&data))
}

/// What reading `data` line by line yields: every line (the last one may
/// lack its newline) cut at a NUL and followed by `\n`.
fn file_lines(data: &[u8]) -> Vec<u8> {
    let body = data.strip_suffix(b"\n").unwrap_or(data);
    let mut config = Vec::with_capacity(data.len() + 1);
    if data.is_empty() {
        return config;
    }
    for line in body.split(|&b| b == b'\n') {
        config.extend_from_slice(glibstr::until_nul(line));
        config.push(b'\n');
    }
    config
}

/// Everything on standard input.
pub fn read_stdin_config() -> Vec<u8> {
    let mut data = Vec::new();
    // A read error ends the input like end-of-file does.
    let _ = std::io::stdin().lock().read_to_end(&mut data);
    stdin_chunks(&data)
}

/// What reading `data` with `fgets` into a 1024-byte buffer yields: chunks
/// of up to 1023 bytes that end at a newline, each cut at a NUL.
fn stdin_chunks(data: &[u8]) -> Vec<u8> {
    let mut config = Vec::with_capacity(data.len());
    let mut rest = data;
    while !rest.is_empty() {
        let limit = rest.len().min(STDIN_CHUNK - 1);
        let end = rest[..limit]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(limit, |newline| newline + 1);
        config.extend_from_slice(glibstr::until_nul(&rest[..end]));
        rest = &rest[end..];
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_lines_end_with_newlines() {
        assert_eq!(file_lines(b"a\nb"), b"a\nb\n");
        assert_eq!(file_lines(b"a\n"), b"a\n");
        assert_eq!(file_lines(b"\n\n"), b"\n\n");
        assert_eq!(file_lines(b""), b"");
        assert_eq!(file_lines(b"a\0x\nb\r\n"), b"a\nb\r\n");
    }

    #[test]
    fn stdin_chunks_cut_at_nul() {
        assert_eq!(stdin_chunks(b"ab\ncd"), b"ab\ncd");
        // The NUL also drops the rest of its chunk, newline included.
        assert_eq!(stdin_chunks(b"a\0b\ncd"), b"acd");
        let mut long = vec![b'x'; 1023];
        long.extend_from_slice(b"\0y\n");
        // The NUL starts the second chunk, which then holds nothing.
        assert_eq!(stdin_chunks(&long), vec![b'x'; 1023]);
    }

    #[test]
    fn missing_file_reports_the_normalized_path() {
        match read_file_config(Path::new("/nonexistent-remontoire/../x//y")) {
            Err(LoadError::Missing(path)) => assert_eq!(path, "/x/y"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn directory_fails_with_gio_message() {
        match read_file_config(Path::new("/")) {
            Err(LoadError::Failed(message)) => assert!(message.contains('/'), "{message}"),
            other => panic!("unexpected {other:?}"),
        }
    }
}

//! remontoire: show the keybindings documented in an i3 or sway config.
//!
//! Lines of the form `## Category // Action // Keys ##` are read from a
//! file (`-c`), standard input (`-i`) or the running window manager over
//! IPC (`-s`) and shown grouped by category in a window.

mod args;
mod categories;
mod config;
mod glibstr;
mod input;
mod ipc;
mod json;
mod window;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use input::LoadError;

const APPLICATION_ID: &str = "org.regolith.remontoire";
const SCHEMA_ID: &str = "org.regolith-linux.remontoire";
const VERSION_LINE: &str = "remontoire 1.4.4 (C) 2022 Ken Gilmer\n";
const STYLE_CSS: &[u8] = include_bytes!("../../data/style.css");
/// The usage text, byte for byte (indentation and trailing spaces included).
const USAGE: &str = concat!(
    "\n",
    "      Usage:\n",
    "        remontoire (-c <file path> | -s <socket URI> | -h | -v) [-t <file path>]\n",
    "      \n",
    "      Help Options:\n",
    "        -h, --help                           Show help options\n",
    "      \n",
    "      Application Options:\n",
    "        -v, --version                        Display version number\n",
    "        -s <Socket URI>                      Socket path for i3\n",
    "        -c <Path to config file>             Config file\n",
    "        -i <Read from standard input>        Read from standard input\n",
    "        -t <Path to style file>              CSS file\n",
    "        -p <comment line prefix>             Prefix of comment line\n",
    "        \n",
);

/// Where the config text comes from.
#[derive(Clone, Debug)]
enum Source {
    Socket(PathBuf),
    File(PathBuf),
    Stdin,
}

impl Source {
    fn read(&self) -> Result<Vec<u8>, LoadError> {
        match self {
            Self::Socket(path) => ipc::Client::connect(path)
                .and_then(|client| client.config())
                .map_err(LoadError::Failed),
            Self::File(path) => input::read_file_config(path),
            Self::Stdin => Ok(input::read_stdin_config()),
        }
    }
}

/// The message of a GLib error, as the original reported it.
pub fn error_message(error: glib::Error) -> String {
    error.message().to_owned()
}

/// A GLib warning (no log domain), as the original logged them.
pub fn warning(message: &str) {
    glib::g_log!(glib::LogLevel::Warning, "{}", message);
}

fn print_help_hint(program: &str) {
    glib::g_printerr!(
        "Run '{}' to see a full list of available command line options.\n",
        format!("{program} --help")
    );
}

fn main() -> glib::ExitCode {
    let argv: Vec<OsString> = std::env::args_os().collect();
    let program = argv
        .first()
        .map(|arg| arg.to_string_lossy().into_owned())
        .unwrap_or_default();

    let args = match args::parse(&argv) {
        Ok(args) => args,
        Err(message) => {
            glib::g_printerr!("error: {}\n", message);
            print_help_hint(&program);
            return glib::ExitCode::FAILURE;
        }
    };

    // An option followed by another option, such as "-s -v", has no value.
    if let Some(option) = args.missing_value() {
        glib::g_printerr!("error: option {} requires a value\n", option);
        print_help_hint(&program);
        return glib::ExitCode::FAILURE;
    }

    if args.has("-v") || args.has("--version") {
        glib::g_print!("{}", VERSION_LINE);
        return glib::ExitCode::SUCCESS;
    }

    if args.has("-h") || args.has("--help") {
        glib::g_print!("{}", USAGE);
        return glib::ExitCode::SUCCESS;
    }

    let source = if let Some(socket) = args.value("-s") {
        Source::Socket(PathBuf::from(socket))
    } else if let Some(file) = args.value("-c") {
        Source::File(PathBuf::from(file))
    } else if args.has("-i") {
        Source::Stdin
    } else {
        glib::g_printerr!(
            "Must specify -s <i3 Socket URI>, -c <Config file path> or -i <STDIN>.\n"
        );
        print_help_hint(&program);
        return glib::ExitCode::FAILURE;
    };
    let line_prefix = args
        .value("-p")
        .map(|prefix| prefix.to_string_lossy().into_owned())
        .unwrap_or_default();
    let style = args.value("-t").map(PathBuf::from);

    let app = gtk::Application::new(Some(APPLICATION_ID), gio::ApplicationFlags::empty());
    let settings = gio::Settings::new(SCHEMA_ID);

    app.connect_activate(move |app| {
        // A second launch activates the first instance, which reuses its
        // window.
        let window = app.active_window().unwrap_or_else(|| {
            match create_window(app, &source, &line_prefix, style.as_deref(), &settings) {
                Ok(window) => window,
                Err(LoadError::Missing(path)) => {
                    glib::g_printerr!("File '{}' doesn't exist.\n", path);
                    process::exit(1);
                }
                // A missing window manager, an unreadable file or a bad
                // stylesheet is a user error, not a crash.
                Err(LoadError::Failed(message)) => {
                    glib::g_printerr!("Failed to start: {}\n", message);
                    process::exit(1);
                }
            }
        });

        let position = window::Position::from_setting(&settings.string("window-position"));
        let padding = (
            settings.int("window-padding-width"),
            settings.int("window-padding-height"),
        );
        window::keep_placed(&window, position, padding);
        window.show_all();
    });

    app.run_with_args::<&str>(&[])
}

fn create_window(
    app: &gtk::Application,
    source: &Source,
    line_prefix: &str,
    style: Option<&Path>,
    settings: &gio::Settings,
) -> Result<gtk::Window, LoadError> {
    let config = config::to_utf8(&source.read()?);
    let bindings = config::parse(&config, line_prefix);
    let window = window::build(app, &bindings, settings);

    let provider = gtk::CssProvider::new();
    match style {
        None => provider.load_from_data(STYLE_CSS),
        Some(path) => provider.load_from_file(&input::existing_file(path)?),
    }?;
    if let Some(screen) = gtk::gdk::Screen::default() {
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
    Ok(window)
}

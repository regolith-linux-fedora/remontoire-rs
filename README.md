# remontoire-rs

A Rust port of [remontoire](https://github.com/regolith-linux/remontoire),
Regolith's keybinding viewer: a small GTK 3 window that lists the
keybindings documented in an i3 or sway config with comments of the form
`## Category // Action // Keys ##`, grouped in expandable categories. It
reads the config from the window manager over IPC (`-s`), from a file
(`-c`) or from standard input (`-i`).

## Status

**Experimental port; not a drop-in replacement yet.**

- **A specific moment in time.** The port reproduces remontoire as of
  [regolith-linux/remontoire@43ae433](https://github.com/regolith-linux/remontoire/commit/43ae433)
  (`master`, 2026-10-24, version 1.4.4): commit
  [5a44370](https://github.com/regolith-linux/remontoire/commit/5a4437086d89ca96c00bc0d704c82395ba0a031a)
  plus the fifteen fixes merged as #33–#47. Rendered on Ubuntu 26.04 with
  the Regolith i3 and sway configs, the Rust and Vala builds of that commit
  expose identical accessibility trees (categories, actions and keys) over
  i3 IPC, from a file, and over sway IPC.
- **Upstream fixes since then.** None merged on `master` as of 2026-11-03;
  the one open pull request, #31, only changes the AppStream description
  text and is not included. This list may not be complete: check upstream
  before relying on it.
- **Very deep testing is needed** before anyone considers this port as a
  replacement for the Vala remontoire. Regolith has deprecated remontoire,
  so the port exists mainly as a reference for the Rust ports of other
  Regolith components.
- **No deep code reviews yet.** The port has not yet been reviewed in depth
  by the major AI coding agents and code reviewers. Those reviews are
  planned before any replacement decision.
- **Upstreaming is undecided.** This repository is not a pull request to
  regolith-linux/remontoire; whether, how and when any of it goes upstream
  will be decided later.

## What it ports

The whole program, as a gtk-rs 0.18 crate under `rust/`: the command line
(`-v`, `-s`, `-c`, `-i`, `-t`, `-p`, usage errors), parsing of the binding
comments (with a Latin-1 fallback for text that is not UTF-8, and invalid
comments skipped with a warning), i3 and sway IPC with include resolution
(a small hand-written JSON reader; no JSON crate), the expander window with
monitor-aware placement and padding, and the expanded-category state in
GSettings (`org.regolith-linux.remontoire`). The GSettings schema, desktop
entry, AppStream metadata, stylesheet and Debian package name are
unchanged.

## History

The branch is upstream's `master` history up to `43ae433`, which already
contains the fifteen bug fixes the port was built on (#33–#47), plus one
commit:

- `Port remontoire from Vala to Rust`: the crate, the meson and Debian
  build changes, and the removal of the Vala sources.

## Differences from upstream

Behaviour matches upstream `43ae433` for well-formed input. Deliberate
differences:

- Malformed IPC replies: where the Vala program prints GLib/Json-GLib
  `CRITICAL`s and exits without a window (a reply that is not a JSON
  object), the port prints `Failed to start: Invalid reply: expected a JSON
  object` and exits 1; a reply without `config` shows `No Keybindings`; a
  non-string `loaded_config_file_name` falls back to the current directory.
  Invalid JSON is reported as `Invalid JSON reply at byte N: ...` instead
  of Json-GLib's parser message.
- Log lines have no Vala source-location tag
  (`config_parser.vala:103:`).
- A socket read error is reported with GSocket's text
  (`Error receiving data: ...`) instead of `Error reading from file
  descriptor: ...`.
- The Vala unit-test programs became crate unit tests; the two script
  tests (`startup-errors`, `option-without-value`) run against the Rust
  binary.
- The Debian package no longer depends on libgee or json-glib.

## Build

On Ubuntu 26.04, install the build dependencies from `debian/control`
(for example `sudo apt build-dep ./` from the checkout), then:

```sh
meson setup build
meson compile -C build      # runs cargo; fetches the locked crates
build/src/remontoire -c /etc/regolith/i3/config
```

or with cargo alone:

```sh
cd rust
cargo build --release --locked
```

Offline builds: vendor the crates once, and cargo (and therefore meson and
the Debian build) uses them:

```sh
cd rust
mkdir -p .cargo
cargo vendor --locked vendor > .cargo/config.toml
```

## Test

```sh
(cd rust && cargo test --locked && cargo fmt --check)
meson test -C build
```

## Package

```sh
dpkg-buildpackage -b -us -uc
```

builds `remontoire_<version>_<arch>.deb` with the same files as the Vala
package (binary, desktop entry, AppStream metadata, GSettings schema).
cargo needs network access during the build unless the crates were
vendored as above. `org.gnome.Remontoire.json` (Flatpak) is left from the
Vala project and has not been updated for the Rust build.

## Usage

Unchanged from the Vala program. From the original README:

### Model

Remontoire utilizes the concept of a `category` to group items, `action` to denote the human description, and `keybinding` to define the specific keys corresponding to the action.  The format is designed to be both easily parsable by program but also readable in it's native form by people:

```
## <category> // <action> // <keybinding> ## <reserved for user notes>
```

Text within `<category>`, `<action>`, and `<keybinding>` must not contain the sequences `##`, `//`, or line feeds.

Examples:

```
...
## Navigate // Relative Window // <Super> ↑ ↓ ← → ##
bindsym $mod+Left focus left
...
```

```
...
## Launch // Application // <Super> Space ## some extra notes that are ignored by Remontoire but maybe of interest to those reading the config file.
bindsym $mod+space exec $i3-wm.program.launcher.app
```

Any line that doesn't contain the structure listed here will be ignored.

### Usage

```
Usage:
  remontoire [OPTION?]

Help Options:
  -h, --help                           Show help options

Application Options:
  -v, --version                        Display version number
  -s, --socket=<Socket URI>            Socket path for i3
  -c <Path to config file>             Config file
  -i <Read from standard input>        Read from standard input
  -t <Path to style file>              CSS file
  -p <comment line prefix>             Prefix of comment line

```

With `-s`, Remontoire communicates with i3 via domain sockets to retrieve the active i3 config file.  To determine the socket path on a system running i3:
```bash
$ i3 --get-socketpath
```

Or altogether:
```bash
$ remontoire -s `i3 --get-socketpath`
```

Remontoire can also be passed a file path and will read from that instead of the i3 socket.  In this mode, Remontoire can be used to display keybindings from any file that utilize the comment format.

```bash
$ remontoire -c /etc/something/interesting.conf
```

As a third option to provide your config files, Remontoire can read from STDIN. Use this option
if you want to pass in the contents of multiple config files.

Once executed Remontoire will display a sticky floating window on the right-center of the primary monitor. Upon first launch, all categories are collapsed.  User selections to open categories are persisted across instantiations of the program.

#### Toggle

It is suggested to use a small shell script to allow the dialog to be toggled on and off with a hotkey.  Here is one such script from Regolith:

```
#!/bin/bash
# If remontoire is running, kill it.  Otherwise start it.

remontoire_PID=$(pidof remontoire)

if [ -z "$remontoire_PID" ]; then
    /usr/bin/remontoire -s $(printenv I3SOCK) &
else
    kill $remontoire_PID
fi
```

### Configuration

Remontoire utilizes GLib settings to store configuration using the namespace `org.regolith-linux.remontoire`.  The following settings are available for user customization:

#### Window Position
```
window-position <north|south|east|west>
```

##### Example

```bash
$ gsettings set org.regolith-linux.remontoire window-position "west"
```

#### Padding

Vertical and horizontal padding can be specified independently, allowing for bars or other UI widgets to be accounted for when placing the window.  The keys for padding are `window-padding-width` and `window-padding-height` and the value units are pixels.

##### Example

```bash
$ gsettings set org.regolith-linux.remontoire window-padding-width 10
$ gsettings set org.regolith-linux.remontoire window-padding-height 20
```

### Style

You can specify a custom CSS file to change the look of the dialog.  The built-in CSS as of version 1.3.0:

```css
.window {
  margin: 4px;
}

*:selected {
  background-color: @theme_bg_color;
  color: @theme_text_color;
}

.category {
  padding-top: 2px;
  padding-bottom: 2px;
  font-weight: bold;
  font-size: .95em;
  color: @theme_unfocused_fg_color;
}

.action {
  padding-right: 10px;
  padding-left: 15px;
  font-weight: lighter;
}

.error {
  padding: 15px;
  font-size: 1.2em;
  font-weight: bold;
}

.metakey {
  font-family: monospace;
  font-weight: normal;
  background-color: @insensitive_bg_color;
  border: 1px solid;
  border-color: @insensitive_base_color;
  color: @theme_unfocused_fg_color;

  padding: 2px;
  margin: 2px;
  font-size: .9em;
}

.rangekey {
  font-family: monospace;
  font-weight: normal;
  background-color: @insensitive_bg_color;
  border: 1px solid;
  border-color: @insensitive_base_color;
  color: @theme_unfocused_fg_color;

  padding: 2px;
  margin: 2px;
  font-size: .9em;
}

.key {
  font-family: monospace;
  font-weight: normal;
  background-color: @insensitive_bg_color;
  border: 1px solid;
  border-color: @insensitive_base_color;
  color: @theme_unfocused_fg_color;

  padding: 2px;
  margin: 2px;
  font-size: .9em;
}

.detail {
  margin: 2px;
}
```

### Using Remontoire to view keybindings for arbitrary config files

Since Remontoire parses comments and not actual keybindings, it can be used as a keybinding viewer for any app that stores keybindings in plain text and supports comments, like Sway or Vim. Use the `-c` or  `-i` options documented above to supply the config files. If config doesn't use `#` as a comment prefix, you use the `-p` option to supply comment prefix to go immediately before '##'. Here's an example of parsing a comment using Vim's quote character as a prefix:

    echo '"## Category // Description // <Super> J ##' | remontoire -i -p '"'

## License

GNU General Public License v3.0 or later, as the original program; see
[COPYING](COPYING). remontoire was written by Ken Gilmer and the Regolith
contributors.

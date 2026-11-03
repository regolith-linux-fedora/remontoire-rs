# remontoire is the Rust crate under rust/, built by cargo with the locked
# dependency graph (see rust/meson-build.sh). The stylesheet in data/style.css
# is compiled into the binary.

# The binary links these through its -sys crates; checking them here keeps a
# missing system library a configure-time error.
dependency('gio-2.0', version: '>= 2.50')
dependency('gtk+-3.0', version: '>= 3.22')

cargo = find_program('cargo')
cargo_build = find_program('../rust/meson-build.sh')

remontoire_exe = custom_target('remontoire',
  output: 'remontoire',
  command: [
    cargo_build,
    cargo,
    join_paths(meson.project_source_root(), 'rust'),
    join_paths(meson.current_build_dir(), 'cargo-target'),
    '@OUTPUT@',
  ],
  build_by_default: true,
  build_always_stale: true,
  console: true,
  install: true,
  install_dir: get_option('bindir'),
)

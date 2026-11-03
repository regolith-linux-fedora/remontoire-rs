# The Vala unit tests retired with the Vala sources; their cases live on as
# the Rust crate's unit tests (cargo test). The black-box script tests run
# against the Rust binary.
test('startup-errors', find_program('startup_errors.sh'),
  args: [remontoire_exe, join_paths(meson.current_source_dir(), '..', 'data')],
)

test('option-without-value', find_program('option_without_value.sh'),
  args: [remontoire_exe],
)

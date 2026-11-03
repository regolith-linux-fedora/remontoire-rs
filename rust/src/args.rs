//! The command line: a minimal parser that maps each option to the token
//! after it, if that token is not itself an option.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;

/// Options that need a value, in the order a missing one is reported.
pub const VALUE_OPTIONS: [&str; 4] = ["-s", "-c", "-t", "-p"];

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    options: HashMap<OsString, Option<OsString>>,
}

impl Args {
    pub fn has(&self, option: &str) -> bool {
        self.options.contains_key(OsStr::new(option))
    }

    /// The value given to `option`, if it was given one.
    pub fn value(&self, option: &str) -> Option<&OsStr> {
        self.options
            .get(OsStr::new(option))
            .and_then(|value| value.as_deref())
    }

    /// The first option in [`VALUE_OPTIONS`] that was given without a value.
    pub fn missing_value(&self) -> Option<&'static str> {
        VALUE_OPTIONS
            .into_iter()
            .find(|&option| self.has(option) && self.value(option).is_none())
    }
}

fn is_option(token: &OsStr) -> bool {
    token.as_bytes().starts_with(b"-")
}

/// Parse `args`, whose first token is the program name. A token starting
/// with `-` is an option; an option followed by a token that does not start
/// with `-` takes it as its value. A bare token with no option before it is
/// an error, whose message ends with a newline as the original's did.
pub fn parse(args: &[OsString]) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut pending: Option<&OsString> = None;
    for token in args.iter().skip(1) {
        if is_option(token) {
            if let Some(option) = pending.replace(token) {
                parsed.options.insert(option.clone(), None);
            }
        } else if let Some(option) = pending.take() {
            parsed.options.insert(option.clone(), Some(token.clone()));
        } else {
            return Err(format!("Unexpected literal: {}\n", token.to_string_lossy()));
        }
    }
    if let Some(option) = pending {
        parsed.options.insert(option.clone(), None);
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(tokens: &[&str]) -> Result<Args, String> {
        let args: Vec<OsString> = std::iter::once("remontoire")
            .chain(tokens.iter().copied())
            .map(OsString::from)
            .collect();
        parse(&args)
    }

    #[test]
    fn options_take_following_values() {
        let args = parsed(&["-c", "conf", "-v", "-p", "# "]).unwrap();
        assert_eq!(args.value("-c"), Some(OsStr::new("conf")));
        assert_eq!(args.value("-p"), Some(OsStr::new("# ")));
        assert!(args.has("-v") && args.value("-v").is_none());
        assert_eq!(args.missing_value(), None);
    }

    #[test]
    fn option_before_option_or_end_has_no_value() {
        assert_eq!(
            parsed(&["-c", "conf", "-s", "-v"]).unwrap().missing_value(),
            Some("-s")
        );
        assert_eq!(
            parsed(&["-c", "conf", "-p", "--"]).unwrap().missing_value(),
            Some("-p")
        );
        assert_eq!(parsed(&["-t", "-c"]).unwrap().missing_value(), Some("-c"));
        assert_eq!(
            parsed(&["-p", "x", "-t", "-s"]).unwrap().missing_value(),
            Some("-s")
        );
        // A repeated option keeps its last occurrence.
        assert_eq!(
            parsed(&["-p", "one", "-p"]).unwrap().missing_value(),
            Some("-p")
        );
    }

    #[test]
    fn bare_literal_is_an_error() {
        assert_eq!(
            parsed(&["literal"]),
            Err("Unexpected literal: literal\n".to_owned())
        );
        assert_eq!(
            parsed(&["-v", "value", "extra"]),
            Err("Unexpected literal: extra\n".to_owned())
        );
        assert!(parsed(&["--", "literal"]).unwrap().value("--").is_some());
    }

    #[test]
    fn no_arguments_is_empty() {
        assert_eq!(parse(&[]), Ok(Args::default()));
        assert_eq!(parsed(&[]), Ok(Args::default()));
    }
}

//! Binding comments: `## Category // Action // Keys ## anything else`.
//!
//! The config text becomes a map from category (sorted) to its bindings
//! (in config order, repeats kept).

use std::collections::BTreeMap;

use crate::glibstr;

const LINE_WRAPPER: &str = "##";
const PARAM_DELIMITER: &str = "//";
const PARAMETER_COUNT: usize = 3;
/// `"##x//y//z//##".len()`: a binding comment is longer than this.
const MIN_LINE_LENGTH: usize = 13;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keybinding {
    pub label: String,
    pub spec: String,
}

/// Categories in byte order, each with its bindings in config order.
pub type Bindings = BTreeMap<String, Vec<Keybinding>>;

/// How a key token is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStyle {
    /// `<Super>`: shown without the brackets.
    Meta,
    /// `A..Z`.
    Range,
    Plain,
}

impl KeyStyle {
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Meta => "metakey",
            Self::Range => "rangekey",
            Self::Plain => "key",
        }
    }
}

/// Config text as UTF-8. GTK can only show valid UTF-8, but config files
/// are not always UTF-8: each line that is not valid UTF-8 is read as
/// Latin-1 (ISO-8859-1), in which every byte is a character.
pub fn to_utf8(config: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(config) {
        return text.to_owned();
    }
    let lines: Vec<String> = config
        .split(|&b| b == b'\n')
        .map(|line| match std::str::from_utf8(line) {
            Ok(text) => text.to_owned(),
            Err(_) => line.iter().map(|&b| char::from(b)).collect(),
        })
        .collect();
    lines.join("\n")
}

/// The bindings in `config`. With a non-empty `line_prefix`, binding
/// comments start with `<line_prefix>##` and the prefix is dropped.
pub fn parse(config: &str, line_prefix: &str) -> Bindings {
    let mut bindings = Bindings::new();
    let prefix = format!("{line_prefix}{LINE_WRAPPER}");

    for line in config.split('\n') {
        let mut trimmed = glibstr::strip_str(line);
        if line_match(trimmed, &prefix) {
            if !line_prefix.is_empty() {
                trimmed = &trimmed[line_prefix.len()..];
            }
            parse_line(trimmed, &mut bindings);
        }
    }
    bindings
}

fn line_match(line: &str, prefix: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() > MIN_LINE_LENGTH
        && line.starts_with(prefix)
        && glibstr::contains(&bytes[LINE_WRAPPER.len() + 1..], LINE_WRAPPER.as_bytes())
        && line.contains(PARAM_DELIMITER)
}

/// Add the binding in `line`, which starts with `##`. Only the section up
/// to the next `##` is read; a comment without exactly three fields is
/// skipped with a warning.
fn parse_line(line: &str, bindings: &mut Bindings) {
    let start = LINE_WRAPPER.len();
    // Without a closing marker (possible only after a prefix that held
    // one), the section runs to the end of the line.
    let end = glibstr::find(line.as_bytes(), LINE_WRAPPER.as_bytes(), 3).unwrap_or(line.len());
    let values: Vec<&str> = match line.get(start..end) {
        Some("") | None => Vec::new(),
        Some(section) => section.split(PARAM_DELIMITER).collect(),
    };

    let [category, label, spec] = values[..] else {
        crate::warning(&format!("Ignoring invalid keybinding comment: {line}"));
        return;
    };
    debug_assert_eq!(values.len(), PARAMETER_COUNT);

    bindings
        .entry(glibstr::strip_str(category).to_owned())
        .or_default()
        .push(Keybinding {
            label: glibstr::strip_str(label).to_owned(),
            spec: glibstr::strip_str(spec).to_owned(),
        });
}

/// Split a key spec into tokens: spaces separate tokens, `<` starts one and
/// `>` ends one. `"<super><shift>a b"` gives `<super>`, `<shift>`, `a`, `b`.
pub fn parse_keybinding(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    for c in raw.chars() {
        match c {
            '<' => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
                token.push(c);
            }
            '>' => {
                token.push(c);
                tokens.push(std::mem::take(&mut token));
            }
            ' ' => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            _ => token.push(c),
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

/// The text and style a key token is shown with.
pub fn key_label(token: &str) -> (&str, KeyStyle) {
    if token.len() >= 2 && token.starts_with('<') && token.ends_with('>') {
        (&token[1..token.len() - 1], KeyStyle::Meta)
    } else if token.contains("..") {
        (token, KeyStyle::Range)
    } else {
        (token, KeyStyle::Plain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(bindings: &Bindings) -> Vec<(String, String, String)> {
        bindings
            .iter()
            .flat_map(|(category, list)| {
                list.iter()
                    .map(move |k| (category.clone(), k.label.clone(), k.spec.clone()))
            })
            .collect()
    }

    #[test]
    fn categories_sort_and_rows_keep_order() {
        let config = "## Zulu // z // Z ##\n\
                      ##   Alpha  //   first   //   <Super Shift> a   ## ignored // x\n\
                      ## Alpha // second // b ##\n \
                      ## Lead // space // k ##\n\
                      ## Alpha // second // c ##\n";
        let parsed = parse(config, "");
        assert_eq!(
            rows(&parsed),
            [
                ("Alpha".into(), "first".into(), "<Super Shift> a".into()),
                ("Alpha".into(), "second".into(), "b".into()),
                ("Alpha".into(), "second".into(), "c".into()),
                ("Lead".into(), "space".into(), "k".into()),
                ("Zulu".into(), "z".into(), "Z".into()),
            ]
        );
    }

    #[test]
    fn non_bindings_are_skipped() {
        let config = "## Ignored // no closing marker // k\n\
                      ## Ignored no field separators ##\n\
                      ## Bad // two fields ##\n\
                      ## Bad // too // many // fields ##\n\
                      ##a//b//c##\n\
                      # ## A // b // c ##\n";
        assert!(parse(config, "").is_empty());
    }

    #[test]
    fn prefix_selects_and_strips() {
        let config = "#custom ## Prefix // selected // k ##\n## Other // x // y ##\n";
        let parsed = parse(config, "#custom ");
        assert_eq!(
            rows(&parsed),
            [("Prefix".into(), "selected".into(), "k".into())]
        );
        // The prefix may hold the second marker: the section runs to the end.
        let parsed = parse("#a## ## C // A // K\n", "#a## ");
        assert_eq!(rows(&parsed), [("C".into(), "A".into(), "K".into())]);
    }

    #[test]
    fn short_prefixed_remainder_does_not_panic() {
        assert!(parse("#//## x y z w v u##", "#//## x y z w v u").is_empty());
    }

    #[test]
    fn latin1_lines_are_converted() {
        let mut config = "## Unicode // 日本語 // λ ##\n".as_bytes().to_vec();
        config.extend_from_slice(b"## Stra\xdfe // Fu\xdf // \xdf ##\n");
        assert_eq!(
            to_utf8(&config),
            "## Unicode // 日本語 // λ ##\n## Straße // Fuß // ß ##\n"
        );
        assert_eq!(to_utf8("Café".as_bytes()), "Café");
    }

    #[test]
    fn key_specs_tokenize() {
        assert_eq!(
            parse_keybinding("<Super><Shift> b"),
            ["<Super>", "<Shift>", "b"]
        );
        assert_eq!(
            parse_keybinding("<Super Shift> a"),
            ["<Super", "Shift>", "a"]
        );
        assert_eq!(parse_keybinding("Mod4+d  x>y"), ["Mod4+d", "x>", "y"]);
        assert_eq!(parse_keybinding(" "), Vec::<String>::new());
        assert_eq!(key_label("<Super>"), ("Super", KeyStyle::Meta));
        assert_eq!(key_label("<>"), ("", KeyStyle::Meta));
        assert_eq!(key_label("A..Z"), ("A..Z", KeyStyle::Range));
        assert_eq!(key_label("<Super"), ("<Super", KeyStyle::Plain));
        assert_eq!(key_label(">"), (">", KeyStyle::Plain));
    }
}

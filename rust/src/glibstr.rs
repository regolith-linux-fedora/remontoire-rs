//! Byte-string helpers with the exact semantics of the GLib functions the
//! original implementation relied on (`g_strstrip`, `g_path_get_dirname`,
//! `g_build_filename`, `g_pattern_match_simple`, ...).
//!
//! Config text and file names are not guaranteed to be UTF-8, so these work
//! on bytes, like the C strings they mirror.

/// `g_ascii_isspace`: space, `\t`, `\n`, `\f` and `\r` (not `\v`).
pub fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0c | b'\r')
}

/// `g_strstrip`: remove leading and trailing ASCII whitespace.
pub fn strip(text: &[u8]) -> &[u8] {
    let start = text
        .iter()
        .position(|&b| !is_space(b))
        .unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|&b| !is_space(b))
        .map_or(start, |last| last + 1);
    &text[start..end]
}

/// `g_strstrip` on UTF-8 text. Only ASCII bytes are removed, so the result
/// is still valid UTF-8.
pub fn strip_str(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_ascii() && is_space(c as u8))
}

/// The C string view of `bytes`: everything before the first NUL.
pub fn until_nul(bytes: &[u8]) -> &[u8] {
    bytes
        .iter()
        .position(|&b| b == 0)
        .map_or(bytes, |nul| &bytes[..nul])
}

/// Byte offset of the first `needle` at or after `from` (`strstr`).
pub fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > haystack.len() || needle.is_empty() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| offset + from)
}

/// Whether `haystack` contains `needle`.
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle, 0).is_some()
}

/// `g_path_get_dirname`.
pub fn dirname(path: &[u8]) -> Vec<u8> {
    let Some(mut base) = path.iter().rposition(|&b| b == b'/') else {
        return b".".to_vec();
    };
    while base > 0 && path[base] == b'/' {
        base -= 1;
    }
    path[..=base].to_vec()
}

/// `g_path_get_basename`.
pub fn basename(path: &[u8]) -> Vec<u8> {
    if path.is_empty() {
        return b".".to_vec();
    }
    let Some(last) = path.iter().rposition(|&b| b != b'/') else {
        return b"/".to_vec();
    };
    let start = path[..last]
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |slash| slash + 1);
    path[start..=last].to_vec()
}

/// `g_path_is_absolute`.
pub fn is_absolute(path: &[u8]) -> bool {
    path.first() == Some(&b'/')
}

/// `g_build_filename`: join elements with single separators, keeping the
/// leading separators of the first element and the trailing separators of
/// the last one, and skipping empty elements.
pub fn build_filename(elements: &[&[u8]]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut is_first = true;
    let mut have_leading = false;
    let mut single_element: Option<&[u8]> = None;
    let mut last_trailing: &[u8] = &[];

    for &element in elements.iter().filter(|e| !e.is_empty()) {
        let start = element
            .iter()
            .position(|&b| b != b'/')
            .unwrap_or(element.len());
        let end = element
            .iter()
            .rposition(|&b| b != b'/')
            .map_or(start, |last| (last + 1).max(start));
        let trailing_from = element
            .iter()
            .rposition(|&b| b != b'/')
            .map_or(0, |last| last + 1);
        last_trailing = &element[trailing_from..];

        if have_leading {
            single_element = None;
        } else {
            // Leading and trailing separators overlap: the element is all
            // separators, and is the result if nothing else follows.
            if trailing_from <= start {
                single_element = Some(element);
            }
            result.extend_from_slice(&element[..start]);
            have_leading = true;
        }

        if end == start {
            continue;
        }
        if !is_first {
            result.push(b'/');
        }
        result.extend_from_slice(&element[start..end]);
        is_first = false;
    }

    if let Some(element) = single_element {
        return element.to_vec();
    }
    result.extend_from_slice(last_trailing);
    result
}

/// `g_pattern_match_simple`: `*` matches any sequence of characters, `?`
/// exactly one character, everything else itself.
pub fn pattern_match_simple(pattern: &[u8], text: &[u8]) -> bool {
    let pattern: Vec<char> = String::from_utf8_lossy(pattern).chars().collect();
    let text: Vec<char> = String::from_utf8_lossy(text).chars().collect();

    // Iterative wildcard matching with single-star backtracking.
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(&c) if c == '?' || c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((star_p, star_t)) => {
                    p = star_p + 1;
                    t = star_t + 1;
                    star = Some((star_p, star_t + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_matches_g_strstrip() {
        assert_eq!(strip(b" \t\x0c\r\n a b \n"), b"a b");
        assert_eq!(strip(b"\x0b a \x0b"), b"\x0b a \x0b");
        assert_eq!(strip(b"   "), b"");
        assert_eq!(strip_str("\u{a0}x\u{a0}"), "\u{a0}x\u{a0}");
        assert_eq!(strip_str("\x0c x \x0c"), "x");
    }

    #[test]
    fn dirname_matches_glib() {
        assert_eq!(dirname(b"/a/b"), b"/a");
        assert_eq!(dirname(b"/a/b/"), b"/a/b");
        assert_eq!(dirname(b"/a"), b"/");
        assert_eq!(dirname(b"//a"), b"/");
        assert_eq!(dirname(b"a"), b".");
        assert_eq!(dirname(b"a//b"), b"a");
        assert_eq!(dirname(b"/"), b"/");
    }

    #[test]
    fn basename_matches_glib() {
        assert_eq!(basename(b"/a/b"), b"b");
        assert_eq!(basename(b"/a/b//"), b"b");
        assert_eq!(basename(b"///"), b"/");
        assert_eq!(basename(b""), b".");
        assert_eq!(basename(b"name"), b"name");
    }

    #[test]
    fn build_filename_matches_glib() {
        assert_eq!(build_filename(&[b"/etc", b"x.conf"]), b"/etc/x.conf");
        assert_eq!(build_filename(&[b"/", b"x"]), b"/x");
        assert_eq!(build_filename(&[b"/etc/", b"/x/"]), b"/etc/x/");
        assert_eq!(build_filename(&[b".", b"a/b"]), b"./a/b");
        assert_eq!(build_filename(&[b"/etc", b""]), b"/etc");
        assert_eq!(build_filename(&[b"//", b""]), b"//");
        assert_eq!(build_filename(&[b"//"]), b"//");
        assert_eq!(build_filename(&[b"a", b"//"]), b"a//");
    }

    #[test]
    fn pattern_matching() {
        assert!(pattern_match_simple(b"*.conf", b"10-a.conf"));
        assert!(pattern_match_simple(b"??-?.conf", b"10-a.conf"));
        assert!(!pattern_match_simple(b"??-?.conf", b"30-long.conf"));
        assert!(pattern_match_simple(b"?", "é".as_bytes()));
        assert!(pattern_match_simple(b"a*b*c", b"aXbYbZc"));
        assert!(!pattern_match_simple(b"a*b", b"ac"));
        assert!(pattern_match_simple(b"**", b""));
    }

    #[test]
    fn find_and_nul() {
        assert_eq!(find(b"## a ##", b"##", 3), Some(5));
        assert_eq!(find(b"##", b"##", 3), None);
        assert_eq!(until_nul(b"ab\0cd"), b"ab");
        assert!(contains(b"x//y", b"//"));
    }
}

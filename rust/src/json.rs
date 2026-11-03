//! A small JSON reader for i3/sway IPC replies.
//!
//! Only what the replies need: the full JSON grammar is accepted, objects
//! keep the last value of a repeated member (as json-glib does) and strings
//! are decoded with their escapes, including UTF-16 surrogate pairs.

use std::collections::BTreeMap;
use std::fmt;

/// Nesting deeper than this is rejected instead of exhausting the stack.
const MAX_DEPTH: usize = 512;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    pub fn as_object(&self) -> Option<&BTreeMap<String, Json>> {
        match self {
            Self::Object(members) => Some(members),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Self::Array(elements) => Some(elements),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    /// The string member `name` of an object (`json_object_get_string_member`).
    pub fn string_member(&self, name: &str) -> Option<&str> {
        self.as_object()?.get(name)?.as_str()
    }

    /// Whether this is an object with a member `name`.
    pub fn has_member(&self, name: &str) -> bool {
        self.as_object()
            .is_some_and(|members| members.contains_key(name))
    }
}

/// Where and why the payload is not JSON.
#[derive(Debug, PartialEq)]
pub struct ParseError {
    offset: usize,
    message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid JSON reply at byte {}: {}",
            self.offset, self.message
        )
    }
}

/// Parse a complete JSON document. A document holding only whitespace has
/// no root value: `Ok(None)`, as json-glib reports an empty reply.
pub fn parse(bytes: &[u8]) -> Result<Option<Json>, ParseError> {
    let mut reader = Reader { bytes, offset: 0 };
    reader.skip_space();
    if reader.offset == bytes.len() {
        return Ok(None);
    }
    let value = reader.value(0)?;
    reader.skip_space();
    if reader.offset != bytes.len() {
        return Err(reader.error("unexpected data after the value"));
    }
    Ok(Some(value))
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn error(&self, message: &'static str) -> ParseError {
        ParseError {
            offset: self.offset,
            message,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.offset += 1;
        }
    }

    fn expect(&mut self, byte: u8, message: &'static str) -> Result<(), ParseError> {
        if self.peek() == Some(byte) {
            self.offset += 1;
            Ok(())
        } else {
            Err(self.error(message))
        }
    }

    fn literal(&mut self, word: &[u8], value: Json) -> Result<Json, ParseError> {
        if self.bytes[self.offset..].starts_with(word) {
            self.offset += word.len();
            Ok(value)
        } else {
            Err(self.error("unknown literal"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.error("nesting too deep"));
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal(b"true", Json::Bool(true)),
            Some(b'f') => self.literal(b"false", Json::Bool(false)),
            Some(b'n') => self.literal(b"null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
            None => Err(self.error("unexpected end of data")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.offset += 1;
        let mut members = BTreeMap::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.offset += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.skip_space();
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a member name"));
            }
            let name = self.string()?;
            self.skip_space();
            self.expect(b':', "expected ':' after a member name")?;
            self.skip_space();
            let value = self.value(depth + 1)?;
            members.insert(name, value);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.offset += 1,
                Some(b'}') => {
                    self.offset += 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(self.error("expected ',' or '}' in an object")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, ParseError> {
        self.offset += 1;
        let mut elements = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.offset += 1;
            return Ok(Json::Array(elements));
        }
        loop {
            self.skip_space();
            elements.push(self.value(depth + 1)?);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.offset += 1,
                Some(b']') => {
                    self.offset += 1;
                    return Ok(Json::Array(elements));
                }
                _ => return Err(self.error("expected ',' or ']' in an array")),
            }
        }
    }

    fn number(&mut self) -> Result<Json, ParseError> {
        let start = self.offset;
        if self.peek() == Some(b'-') {
            self.offset += 1;
        }
        let digits = |reader: &mut Self| {
            let from = reader.offset;
            while matches!(reader.peek(), Some(b'0'..=b'9')) {
                reader.offset += 1;
            }
            reader.offset > from
        };
        if !digits(self) {
            return Err(self.error("expected digits"));
        }
        if self.peek() == Some(b'.') {
            self.offset += 1;
            if !digits(self) {
                return Err(self.error("expected digits after '.'"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.offset += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.offset += 1;
            }
            if !digits(self) {
                return Err(self.error("expected exponent digits"));
            }
        }
        std::str::from_utf8(&self.bytes[start..self.offset])
            .ok()
            .and_then(|text| text.parse().ok())
            .map(Json::Number)
            .ok_or_else(|| self.error("invalid number"))
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let digits = self
            .bytes
            .get(self.offset..self.offset + 4)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| self.error("invalid \\u escape"))?;
        let value =
            u32::from_str_radix(digits, 16).map_err(|_| self.error("invalid \\u escape"))?;
        self.offset += 4;
        Ok(value)
    }

    fn unicode_escape(&mut self) -> Result<char, ParseError> {
        let first = self.hex4()?;
        let code = match first {
            0xD800..=0xDBFF if self.bytes[self.offset..].starts_with(b"\\u") => {
                let resume = self.offset;
                self.offset += 2;
                let second = self.hex4()?;
                if (0xDC00..=0xDFFF).contains(&second) {
                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                } else {
                    self.offset = resume;
                    0xFFFD
                }
            }
            0xD800..=0xDFFF => 0xFFFD,
            code => code,
        };
        Ok(char::from_u32(code).unwrap_or('\u{FFFD}'))
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.offset += 1;
        let mut text = Vec::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(self.error("unterminated string"));
            };
            self.offset += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(escape) = self.peek() else {
                        return Err(self.error("unterminated escape"));
                    };
                    self.offset += 1;
                    let decoded = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode_escape()?,
                        _ => return Err(self.error("invalid escape")),
                    };
                    let mut buffer = [0; 4];
                    text.extend_from_slice(decoded.encode_utf8(&mut buffer).as_bytes());
                }
                0x00..=0x1f => return Err(self.error("control character in a string")),
                _ => text.push(byte),
            }
        }
        String::from_utf8(text).map_err(|_| self.error("invalid UTF-8 in a string"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Json {
        parse(text.as_bytes()).unwrap().unwrap()
    }

    #[test]
    fn reads_nested_values() {
        let value = parsed(r#"{"a": [1, -2.5e3, true, false, null], "b": {"c": "d"}, "e": []}"#);
        let object = value.as_object().unwrap();
        assert_eq!(
            object["a"],
            Json::Array(vec![
                Json::Number(1.0),
                Json::Number(-2500.0),
                Json::Bool(true),
                Json::Bool(false),
                Json::Null,
            ])
        );
        assert_eq!(object["b"].string_member("c"), Some("d"));
        assert!(value.has_member("e"));
        assert!(!value.has_member("c"));
    }

    #[test]
    fn decodes_escapes() {
        let value = parsed(r#""q\" b\\ s\/ \b\f\n\r\t é 😀 \ud800x""#);
        assert_eq!(
            value.as_str(),
            Some("q\" b\\ s/ \u{8}\u{c}\n\r\t é 😀 \u{fffd}x")
        );
    }

    #[test]
    fn keeps_raw_utf8() {
        assert_eq!(parsed("\"Café 日本語\"").as_str(), Some("Café 日本語"));
    }

    #[test]
    fn repeated_member_keeps_last() {
        assert_eq!(
            parsed(r#"{"k": "1", "k": "2"}"#).string_member("k"),
            Some("2")
        );
    }

    #[test]
    fn empty_document_has_no_root() {
        assert_eq!(parse(b"  \n"), Ok(None));
    }

    #[test]
    fn rejects_malformed_documents() {
        for text in [
            "{",
            "[1,]",
            "{\"a\" 1}",
            "\"open",
            "tru",
            "01x",
            "{} {}",
            "\"\x01\"",
            "-",
        ] {
            assert!(parse(text.as_bytes()).is_err(), "{text:?}");
        }
        assert!(parse(b"\"\xff\"").is_err());
        let deep = "[".repeat(MAX_DEPTH + 2);
        assert!(parse(deep.as_bytes()).is_err());
    }
}

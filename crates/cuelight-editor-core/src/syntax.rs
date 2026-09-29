//! JSON cut into tokens for colouring: keys, strings, numbers, the three
//! literals and punctuation, each with its place in the text. Nothing is
//! checked: text that is not JSON still comes out whole, in tokens as
//! near as can be told, so a document half typed colours too.
//!
//! No token spans a line, so a widget that colours one line at a time
//! can cut each line on its own. A key is a string with a colon after it
//! on the same line.

use std::ops::Range;

/// What a piece of the text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    /// An object's key, quotes included.
    Key,
    /// A string value, quotes included.
    String,
    Number,
    /// `true`, `false` or `null`.
    Literal,
    /// Brackets, braces, commas and colons.
    Punctuation,
    /// Whitespace, and whatever is not JSON.
    Plain,
}

/// `text` in tokens, in order, covering every byte of it: adjacent
/// pieces of one kind are joined.
pub fn tokens(text: &str) -> Vec<(Token, Range<usize>)> {
    let bytes = text.as_bytes();
    let mut out: Vec<(Token, Range<usize>)> = Vec::new();
    let mut push = |token: Token, range: Range<usize>| match out.last_mut() {
        Some((last, prior)) if *last == token && prior.end == range.start => prior.end = range.end,
        _ => out.push((token, range)),
    };
    let byte = |at: usize| bytes.get(at).copied();
    let mut at = 0;
    while let Some(first) = byte(at) {
        let start = at;
        let token = match first {
            b'"' => {
                at += 1;
                while let Some(b) = byte(at)
                    && b != b'"'
                    && b != b'\n'
                {
                    at += if b == b'\\' { 2 } else { 1 };
                }
                if byte(at) == Some(b'"') {
                    at += 1;
                }
                at = at.min(bytes.len());
                let after = bytes
                    .get(at..)
                    .unwrap_or_default()
                    .iter()
                    .copied()
                    .find(|b| !matches!(b, b' ' | b'\t' | b'\r'));
                if after == Some(b':') {
                    Token::Key
                } else {
                    Token::String
                }
            }
            b'{' | b'}' | b'[' | b']' | b',' | b':' => {
                at += 1;
                Token::Punctuation
            }
            b'-' | b'0'..=b'9' => {
                at += 1;
                while matches!(
                    byte(at),
                    Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                ) {
                    at += 1;
                }
                Token::Number
            }
            b if b.is_ascii_alphabetic() => {
                while byte(at).is_some_and(|b| b.is_ascii_alphanumeric()) {
                    at += 1;
                }
                match text.get(start..at) {
                    Some("true" | "false" | "null") => Token::Literal,
                    _ => Token::Plain,
                }
            }
            _ => {
                // One character, whole, so a range never cuts one apart.
                at += text
                    .get(at..)
                    .and_then(|rest| rest.chars().next())
                    .map_or(1, char::len_utf8);
                Token::Plain
            }
        };
        push(token, start..at);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut(text: &str) -> Vec<(Token, &str)> {
        tokens(text)
            .into_iter()
            .map(|(token, range)| (token, &text[range]))
            .collect()
    }

    #[test]
    fn each_kind_of_token_is_told_apart() {
        use Token::*;
        assert_eq!(
            cut(r#"{"x": -1.5e3, "on": true, "s": "a\"b", "n": null}"#),
            [
                (Punctuation, "{"),
                (Key, r#""x""#),
                (Punctuation, ":"),
                (Plain, " "),
                (Number, "-1.5e3"),
                (Punctuation, ","),
                (Plain, " "),
                (Key, r#""on""#),
                (Punctuation, ":"),
                (Plain, " "),
                (Literal, "true"),
                (Punctuation, ","),
                (Plain, " "),
                (Key, r#""s""#),
                (Punctuation, ":"),
                (Plain, " "),
                (String, r#""a\"b""#),
                (Punctuation, ","),
                (Plain, " "),
                (Key, r#""n""#),
                (Punctuation, ":"),
                (Plain, " "),
                (Literal, "null"),
                (Punctuation, "}"),
            ]
        );
    }

    #[test]
    fn a_string_in_an_array_is_a_value_and_brackets_join() {
        use Token::*;
        assert_eq!(
            cut("[[\"a\" , \"b\"]]"),
            [
                (Punctuation, "[["),
                (String, "\"a\""),
                (Plain, " "),
                (Punctuation, ","),
                (Plain, " "),
                (String, "\"b\""),
                (Punctuation, "]]"),
            ]
        );
    }

    /// Half-typed text still comes out whole: an open string stops at the
    /// end of its line, and a word that is not a literal is plain.
    #[test]
    fn text_that_is_not_json_is_covered_all_the_same() {
        use Token::*;
        let text = "{\"open: tru\n  \"é\": nope}";
        let tokens = tokens(text);
        let mut end = 0;
        for (_, range) in &tokens {
            assert_eq!(range.start, end, "no gaps");
            end = range.end;
        }
        assert_eq!(end, text.len());
        assert_eq!(
            cut(text),
            [
                (Punctuation, "{"),
                (String, "\"open: tru"),
                (Plain, "\n  "),
                (Key, "\"é\""),
                (Punctuation, ":"),
                (Plain, " nope"),
                (Punctuation, "}"),
            ]
        );
        assert_eq!(cut("\"a\\"), [(String, "\"a\\")], "a trailing escape");
    }
}

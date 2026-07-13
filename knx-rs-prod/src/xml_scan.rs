// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Minimal, quote-aware XML open-tag scanner shared by [`renumber`](crate::renumber)
//! and [`sanity`](crate::sanity).
//!
//! KNX product XML keeps each element's attributes on one line, so a byte scan of
//! `<tag …>` open tags is exact — provided quoting is respected: a literal `>` or the
//! other quote character may appear inside an attribute value, and either `"` or `'`
//! may delimit a value. This module centralises that so both consumers agree.

/// Find the byte index of the `>` that closes the tag opened at `start` (which must
/// point at a `<`), treating `"`/`'`-quoted attribute values as opaque. Returns `None`
/// if the tag is unterminated.
pub fn find_tag_end(xml: &str, start: usize) -> Option<usize> {
    let b = xml.as_bytes();
    let mut i = start + 1;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                b'"' | b'\'' => quote = Some(c),
                b'>' => return Some(i),
                _ => {}
            },
        }
        i += 1;
    }
    None
}

/// An open element tag: its name and body (the text strictly between `<` and `>`).
pub struct OpenTag<'a> {
    pub tag: &'a str,
    pub body: &'a str,
}

/// Scan `xml` and return every open element tag in document order. Comments, CDATA,
/// closing tags, declarations and processing instructions are skipped.
pub fn open_tags(xml: &str) -> Vec<OpenTag<'_>> {
    let b = xml.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        // Comments / CDATA: skip to their own terminator so a `<`/`>` inside cannot
        // be mistaken for a tag.
        if xml[i..].starts_with("<!--") {
            i = xml[i..].find("-->").map_or(b.len(), |e| i + e + 3);
            continue;
        }
        if xml[i..].starts_with("<![CDATA[") {
            i = xml[i..].find("]]>").map_or(b.len(), |e| i + e + 3);
            continue;
        }
        // Closing tag / declaration / PI — not an open element.
        if matches!(b.get(i + 1), Some(b'/' | b'!' | b'?')) {
            i = find_tag_end(xml, i).map_or(b.len(), |e| e + 1);
            continue;
        }
        let Some(end) = find_tag_end(xml, i) else {
            break;
        };
        let body = &xml[i + 1..end];
        out.push(OpenTag {
            tag: tag_name(body),
            body,
        });
        i = end + 1;
    }
    out
}

/// The element name at the start of an open-tag body (leading run of non-whitespace,
/// trailing `/` of a self-closing tag stripped).
pub fn tag_name(body: &str) -> &str {
    let body = body.strip_suffix('/').unwrap_or(body);
    body.split(|c: char| c.is_whitespace()).next().unwrap_or("")
}

/// Parse `name="value"` / `name='value'` attribute pairs from an open-tag `body`,
/// in document order. Values may contain `>` or the other quote character.
pub fn parse_attrs(body: &str) -> Vec<(&str, &str)> {
    let body = body.strip_suffix('/').unwrap_or(body);
    let b = body.as_bytes();
    let mut out = Vec::new();
    // Skip the tag name.
    let mut i = 0;
    while i < b.len() && !(b[i] as char).is_whitespace() {
        i += 1;
    }
    while i < b.len() {
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < b.len() && b[i] != b'=' && !(b[i] as char).is_whitespace() {
            i += 1;
        }
        let name = &body[name_start..i];
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            break;
        }
        i += 1; // '='
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() || (b[i] != b'"' && b[i] != b'\'') {
            break;
        }
        let quote = b[i];
        i += 1;
        let val_start = i;
        while i < b.len() && b[i] != quote {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let value = &body[val_start..i];
        i += 1; // closing quote
        if !name.is_empty() {
            out.push((name, value));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn find_tag_end_skips_gt_inside_value() {
        let xml = r#"<a b="x > y" c='z'>tail"#;
        let end = find_tag_end(xml, 0).unwrap();
        assert_eq!(&xml[..=end], r#"<a b="x > y" c='z'>"#);
    }

    #[test]
    fn parse_attrs_handles_both_quote_styles() {
        let attrs = parse_attrs(r#"tag a="1" b='2' c="x>y"/"#);
        assert_eq!(attrs, vec![("a", "1"), ("b", "2"), ("c", "x>y")]);
    }

    #[test]
    fn open_tags_skips_comments_cdata_and_closing() {
        let tags = open_tags(r#"<a x="1"/><!-- <b y="2"/> --><![CDATA[<c/>]]></a>"#);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].tag, "a");
        assert_eq!(parse_attrs(tags[0].body), vec![("x", "1")]);
    }
}

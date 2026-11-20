//! Lossless `.env` document.
//!
//! Every comment, blank line, and entry keeps the exact source slice it was
//! parsed from. Serialization writes that slice back until something mutates
//! the entry. A round-trip of an untouched document is byte-for-byte.
//!
//! Logical values (what the editor shows) decode quotes and `\\n`-style
//! escapes. `${VAR}` is never expanded. Editing one entry rewrites only that
//! entry; every other slice is copied through.

use std::fmt;

/// How a value was quoted in the source, or how a rewritten value should be quoted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteStyle {
    Bare,
    Double,
    Single,
}

/// One assignment. `raw` is the original source, including its trailing newline
/// when it had one. It is cleared the moment the entry is edited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub(crate) raw: Option<String>,
    pub key: String,
    pub value: String,
    pub exported: bool,
    pub quote: QuoteStyle,
    /// Whitespace before `export` / the key.
    pub leading: String,
    /// Whitespace between the key and `=`.
    pub before_eq: String,
    /// Whitespace between `=` and the value.
    pub after_eq: String,
    /// Text after the value on its last line (inline comment, leftover spaces).
    pub suffix: String,
    pub had_trailing_newline: bool,
}

impl Entry {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        let value = value.into();
        let quote = if needs_quotes(&value) {
            QuoteStyle::Double
        } else {
            QuoteStyle::Bare
        };
        Self {
            raw: None,
            key: key.into(),
            value,
            exported: false,
            quote,
            leading: String::new(),
            before_eq: String::new(),
            after_eq: String::new(),
            suffix: String::new(),
            had_trailing_newline: true,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.raw.is_none()
    }

    /// Replace the logical value and drop the original slice so the next save
    /// rewrites this entry only.
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.quote = compatible_quote(self.quote, &self.value);
        self.raw = None;
    }

    pub fn set_key(&mut self, key: impl Into<String>) {
        self.key = key.into();
        self.raw = None;
    }

    fn render(&self, newline: &str) -> String {
        let mut out = String::new();
        out.push_str(&self.leading);
        if self.exported {
            out.push_str("export ");
        }
        out.push_str(&self.key);
        out.push_str(&self.before_eq);
        out.push('=');
        out.push_str(&self.after_eq);
        out.push_str(&encode_value(&self.value, self.quote));
        out.push_str(&self.suffix);
        if self.had_trailing_newline {
            out.push_str(newline);
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    /// Comment, blank, or a line this parser does not treat as an assignment.
    /// Stored exactly, including its newline.
    Verbatim(String),
    Entry(Entry),
}

/// A `.env` file. Order is the source order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvDocument {
    pub nodes: Vec<Node>,
    /// Newline sequence used when a rewritten or new entry needs one.
    pub newline: String,
}

impl Default for EnvDocument {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            newline: "\n".to_string(),
        }
    }
}

impl EnvDocument {
    pub fn parse(input: &str) -> Self {
        let newline = detect_newline(input).to_string();
        let mut nodes = Vec::new();
        let mut i = 0;
        if let Some(rest) = input.strip_prefix('\u{feff}') {
            let bom_len = input.len() - rest.len();
            nodes.push(Node::Verbatim(input[..bom_len].to_string()));
            i = bom_len;
        }
        while i < input.len() {
            if let Some((entry, next)) = try_parse_entry(input, i) {
                nodes.push(Node::Entry(entry));
                i = next;
            } else {
                let next = end_of_physical_line(input, i);
                nodes.push(Node::Verbatim(input[i..next].to_string()));
                i = next;
            }
        }
        Self { nodes, newline }
    }

    /// Serialize. Untouched nodes are copied from the source.
    pub fn serialize(&self) -> String {
        let mut out = String::with_capacity(self.nodes.len() * 16);
        for node in &self.nodes {
            match node {
                Node::Verbatim(text) => out.push_str(text),
                Node::Entry(entry) => {
                    if let Some(raw) = &entry.raw {
                        out.push_str(raw);
                    } else {
                        out.push_str(&entry.render(&self.newline));
                    }
                }
            }
        }
        out
    }

    pub fn entries(&self) -> impl Iterator<Item = (usize, &Entry)> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(i, node)| match node {
                Node::Entry(entry) => Some((i, entry)),
                Node::Verbatim(_) => None,
            })
    }

    pub fn entries_mut(&mut self) -> impl Iterator<Item = (usize, &mut Entry)> {
        self.nodes
            .iter_mut()
            .enumerate()
            .filter_map(|(i, node)| match node {
                Node::Entry(entry) => Some((i, entry)),
                Node::Verbatim(_) => None,
            })
    }

    /// Last-wins `(key, value)` pairs in the order each key first appeared.
    pub fn pairs(&self) -> Vec<(String, String)> {
        let mut order: Vec<String> = Vec::new();
        let mut values: Vec<String> = Vec::new();
        for (_, entry) in self.entries() {
            if let Some(pos) = order.iter().position(|k| k == &entry.key) {
                values[pos].clone_from(&entry.value);
            } else {
                order.push(entry.key.clone());
                values.push(entry.value.clone());
            }
        }
        order.into_iter().zip(values).collect()
    }

    pub fn key_names(&self) -> Vec<String> {
        self.pairs().into_iter().map(|(k, _)| k).collect()
    }

    /// Last assignment with this key, if any.
    pub fn value_of(&self, key: &str) -> Option<&str> {
        self.entries()
            .filter(|(_, entry)| entry.key == key)
            .map(|(_, entry)| entry.value.as_str())
            .last()
    }

    /// Update every assignment with this key. Returns how many were updated.
    pub fn set_value(&mut self, key: &str, value: &str) -> usize {
        let mut n = 0;
        for node in &mut self.nodes {
            if let Node::Entry(entry) = node {
                if entry.key == key {
                    entry.set_value(value);
                    n += 1;
                }
            }
        }
        n
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.entries().any(|(_, entry)| entry.key == key)
    }

    pub fn remove_entry_at(&mut self, node_index: usize) -> Option<Entry> {
        if node_index >= self.nodes.len() {
            return None;
        }
        match self.nodes.remove(node_index) {
            Node::Entry(entry) => Some(entry),
            other => {
                self.nodes.insert(node_index, other);
                None
            }
        }
    }

    /// Append an assignment. Inserts a newline first when the file does not
    /// already end on one, so the new key lands on its own line.
    pub fn push_entry(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.ensure_trailing_newline();
        self.nodes.push(Node::Entry(Entry::new(key, value)));
    }

    pub fn ensure_trailing_newline(&mut self) {
        let serialized_empty = self.nodes.is_empty();
        if serialized_empty {
            return;
        }
        let ends_with_newline = match self.nodes.last_mut() {
            Some(Node::Verbatim(text)) => text.ends_with('\n'),
            Some(Node::Entry(entry)) => match &entry.raw {
                Some(raw) => raw.ends_with('\n'),
                None => entry.had_trailing_newline,
            },
            None => true,
        };
        if ends_with_newline {
            return;
        }
        match self.nodes.last_mut() {
            Some(Node::Verbatim(text)) => text.push_str(&self.newline),
            Some(Node::Entry(entry)) => {
                if let Some(raw) = &mut entry.raw {
                    raw.push_str(&self.newline);
                }
                entry.had_trailing_newline = true;
            }
            None => {}
        }
    }

    /// Node index of the entry with this key (first match), if any.
    pub fn first_index_of(&self, key: &str) -> Option<usize> {
        self.entries()
            .find(|(_, entry)| entry.key == key)
            .map(|(index, _)| index)
    }
}

impl fmt::Display for EnvDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialize())
    }
}

fn detect_newline(input: &str) -> &'static str {
    if input.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn end_of_physical_line(input: &str, start: usize) -> usize {
    let bytes = input.as_bytes();
    let mut i = start;
    while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'\r' {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'\n' {
        i += 1;
    }
    i
}

fn line_content_end(input: &str, start: usize) -> usize {
    let bytes = input.as_bytes();
    let mut i = start;
    while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
        i += 1;
    }
    i
}

fn try_parse_entry(input: &str, start: usize) -> Option<(Entry, usize)> {
    let content_end = line_content_end(input, start);
    let line = &input[start..content_end];
    let (leading, rest) = split_ascii_ws(line);
    if rest.is_empty() || rest.starts_with('#') {
        return None;
    }

    let (exported, rest) = strip_export(rest)?;
    let (key, rest) = split_key(rest)?;
    let (before_eq, rest) = split_ascii_ws(rest);
    if !rest.starts_with('=') {
        return None;
    }
    let after_eq_and_value = &rest[1..];
    let trimmed_value = after_eq_and_value.trim_start_matches([' ', '\t']);
    // `KEY=` and `KEY= # comment` are empty assignments. Spaces stay in the
    // suffix so a later rewrite of this entry still matches the source.
    if trimmed_value.is_empty() || trimmed_value.starts_with('#') {
        let record_end = end_of_physical_line(input, start);
        let raw = input[start..record_end].to_string();
        let had_trailing_newline = raw.ends_with('\n');
        return Some((
            Entry {
                raw: Some(raw),
                key: key.to_string(),
                value: String::new(),
                exported,
                quote: QuoteStyle::Bare,
                leading: leading.to_string(),
                before_eq: before_eq.to_string(),
                after_eq: String::new(),
                suffix: after_eq_and_value.to_string(),
                had_trailing_newline,
            },
            record_end,
        ));
    }

    let (after_eq, value_src_in_line) = split_ascii_ws(after_eq_and_value);
    let value_rel = line.len() - value_src_in_line.len();
    let value_at = start + value_rel;
    let (value, quote, suffix, record_end) = parse_value(input, value_at, content_end)?;

    // `before_eq` was measured on the first line only. That's the whitespace
    // between the key and `=`, which never crosses a line.
    let raw = input[start..record_end].to_string();
    let had_trailing_newline = raw.ends_with('\n');
    Some((
        Entry {
            raw: Some(raw),
            key: key.to_string(),
            value,
            exported,
            quote,
            leading: leading.to_string(),
            before_eq: before_eq.to_string(),
            after_eq: after_eq.to_string(),
            suffix,
            had_trailing_newline,
        },
        record_end,
    ))
}

fn strip_export(rest: &str) -> Option<(bool, &str)> {
    if let Some(after) = rest.strip_prefix("export") {
        if after.starts_with(' ') || after.starts_with('\t') {
            let (_, trimmed) = split_ascii_ws(after);
            if trimmed.is_empty() {
                return None;
            }
            return Some((true, trimmed));
        }
    }
    Some((false, rest))
}

fn split_key(rest: &str) -> Option<(&str, &str)> {
    let end = rest
        .find(|c: char| c == '=' || c == ' ' || c == '\t' || c == '#' || c == '\r' || c == '\n')
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let key = &rest[..end];
    if !is_env_key(key) {
        return None;
    }
    Some((key, &rest[end..]))
}

pub fn is_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}

/// `value_at` is the first byte of the value in `input`.
/// `first_line_end` is the end of the physical line that holds the `=`.
fn parse_value(
    input: &str,
    value_at: usize,
    first_line_end: usize,
) -> Option<(String, QuoteStyle, String, usize)> {
    let bytes = input.as_bytes();
    if value_at >= bytes.len() || bytes[value_at] == b'\n' || bytes[value_at] == b'\r' {
        let record_end = if value_at >= input.len() {
            input.len()
        } else {
            end_of_physical_line(input, value_at)
        };
        return Some((String::new(), QuoteStyle::Bare, String::new(), record_end));
    }

    match bytes[value_at] {
        b'"' => parse_quoted(input, value_at, QuoteStyle::Double),
        b'\'' => parse_quoted(input, value_at, QuoteStyle::Single),
        _ => {
            let line = &input[value_at..first_line_end];
            let (value, suffix) = split_bare(line);
            let record_end = end_of_physical_line(input, value_at);
            Some((value, QuoteStyle::Bare, suffix, record_end))
        }
    }
}

fn parse_quoted(
    input: &str,
    value_at: usize,
    style: QuoteStyle,
) -> Option<(String, QuoteStyle, String, usize)> {
    let bytes = input.as_bytes();
    let mut i = value_at + 1;
    let mut closed = None;
    while i < bytes.len() {
        let b = bytes[i];
        if style == QuoteStyle::Double && b == b'\\' {
            i += 1;
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        let closer = if style == QuoteStyle::Double {
            b'"'
        } else {
            b'\''
        };
        if b == closer {
            closed = Some(i);
            break;
        }
        i += 1;
    }

    let (inner, after_quote, record_end) = if let Some(close_at) = closed {
        let line_end = line_content_end(input, close_at);
        let suffix_start = close_at + 1;
        let record_end = end_of_physical_line(input, close_at);
        (
            &input[value_at + 1..close_at],
            input[suffix_start..line_end].to_string(),
            record_end,
        )
    } else {
        // Unclosed quote: keep the rest of the file as the value body so the
        // original slice still round-trips. The decoded value omits the opener.
        (&input[value_at + 1..], String::new(), input.len())
    };

    let decoded = match style {
        QuoteStyle::Double => decode_double(inner),
        QuoteStyle::Single => inner.to_string(),
        QuoteStyle::Bare => inner.to_string(),
    };
    Some((decoded, style, after_quote, record_end))
}

fn split_bare(line: &str) -> (String, String) {
    if let Some(index) = find_inline_comment(line) {
        let value = line[..index].trim_end_matches([' ', '\t']);
        let suffix = line[value.len()..].to_string();
        (value.to_string(), suffix)
    } else {
        let trimmed = line.trim_end_matches([' ', '\t']);
        let suffix = line[trimmed.len()..].to_string();
        (trimmed.to_string(), suffix)
    }
}

/// Index of the whitespace that introduces ` #` or `\t#`.
fn find_inline_comment(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if (bytes[i] == b' ' || bytes[i] == b'\t') && bytes[i + 1] == b'#' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn decode_double(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('\n') => {}
            Some('\r') => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            Some(other) => {
                // Keep the backslash. `${VAR}` has no backslash; unknown
                // escapes stay visible instead of being silently eaten.
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn encode_value(value: &str, quote: QuoteStyle) -> String {
    match quote {
        QuoteStyle::Bare => value.to_string(),
        QuoteStyle::Single => {
            let mut out = String::with_capacity(value.len() + 2);
            out.push('\'');
            out.push_str(value);
            out.push('\'');
            out
        }
        QuoteStyle::Double => encode_double(value),
    }
}

fn encode_double(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn needs_quotes(value: &str) -> bool {
    value
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '#' | '\\' | '='))
}

fn compatible_quote(current: QuoteStyle, value: &str) -> QuoteStyle {
    match current {
        QuoteStyle::Double => QuoteStyle::Double,
        QuoteStyle::Single if !value.contains('\'') => QuoteStyle::Single,
        QuoteStyle::Bare if !needs_quotes(value) => QuoteStyle::Bare,
        _ => QuoteStyle::Double,
    }
}

fn split_ascii_ws(s: &str) -> (&str, &str) {
    let rest = s.trim_start_matches([' ', '\t']);
    let leading_len = s.len() - rest.len();
    (&s[..leading_len], rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Database\n\
DB_HOST=localhost\n\
DB_PORT=5432\n\
\n\
export DB_PASSWORD=\"s3cret\"\n\
WELCOME=\"hello\\nworld\"\n\
MULTILINE=\"line1\n\
line2\"\n\
REF=${DB_HOST}:${DB_PORT}\n\
QUOTED_REF=\"${OTHER}\"\n\
EMPTY=\n\
SINGLE='raw ${not expanded}'\n\
SPACED=\"value with spaces\" # note\n\
BARE=hello # inline\n\
  LEADING=1\n\
KEY_EQ = value\n\
";

    #[test]
    fn round_trip_is_byte_for_byte() {
        let doc = EnvDocument::parse(SAMPLE);
        assert_eq!(doc.serialize(), SAMPLE);
    }

    #[test]
    fn crlf_round_trip() {
        let sample = SAMPLE.replace('\n', "\r\n");
        let doc = EnvDocument::parse(&sample);
        assert_eq!(doc.newline, "\r\n");
        assert_eq!(doc.serialize(), sample);
    }

    #[test]
    fn no_trailing_newline_round_trip() {
        let sample = "A=1\nB=2";
        let doc = EnvDocument::parse(sample);
        assert_eq!(doc.serialize(), sample);
    }

    #[test]
    fn decoded_values_keep_escapes_and_references() {
        let doc = EnvDocument::parse(SAMPLE);
        let map: Vec<_> = doc.pairs();
        let get = |k: &str| {
            map.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
                .unwrap()
        };
        assert_eq!(get("DB_HOST"), "localhost");
        assert_eq!(get("DB_PASSWORD"), "s3cret");
        assert_eq!(get("WELCOME"), "hello\nworld");
        assert_eq!(get("MULTILINE"), "line1\nline2");
        assert_eq!(get("REF"), "${DB_HOST}:${DB_PORT}");
        assert_eq!(get("QUOTED_REF"), "${OTHER}");
        assert_eq!(get("EMPTY"), "");
        assert_eq!(get("SINGLE"), "raw ${not expanded}");
        assert_eq!(get("SPACED"), "value with spaces");
        assert_eq!(get("BARE"), "hello");
        assert_eq!(get("LEADING"), "1");
        assert_eq!(get("KEY_EQ"), "value");
    }

    #[test]
    fn editing_one_value_preserves_every_other_byte() {
        let mut doc = EnvDocument::parse(SAMPLE);
        assert_eq!(doc.set_value("DB_PORT", "5433"), 1);
        let out = doc.serialize();
        let expected = SAMPLE.replace("DB_PORT=5432\n", "DB_PORT=5433\n");
        assert_eq!(out, expected);
        // The multiline entry and the export line are still the original slices.
        assert!(out.contains("export DB_PASSWORD=\"s3cret\"\n"));
        assert!(out.contains("MULTILINE=\"line1\nline2\"\n"));
        assert!(out.contains("REF=${DB_HOST}:${DB_PORT}\n"));
        assert!(out.contains("# Database\n"));
        assert!(out.contains("\n\nexport"));
    }

    #[test]
    fn comments_and_unknown_lines_stay_verbatim() {
        let sample = "# c\n\nnot an assignment\nFOO=bar\n";
        let doc = EnvDocument::parse(sample);
        assert_eq!(doc.serialize(), sample);
        assert_eq!(doc.pairs(), vec![("FOO".into(), "bar".into())]);
    }

    #[test]
    fn push_entry_appends_without_rewriting_neighbors() {
        let mut doc = EnvDocument::parse("A=1\n# keep\n");
        doc.push_entry("B", "2");
        assert_eq!(doc.serialize(), "A=1\n# keep\nB=2\n");
    }

    #[test]
    fn push_entry_onto_file_without_trailing_newline() {
        let mut doc = EnvDocument::parse("A=1");
        doc.push_entry("B", "two words");
        assert_eq!(doc.serialize(), "A=1\nB=\"two words\"\n");
    }

    #[test]
    fn duplicate_keys_last_wins_in_pairs_and_set_updates_all() {
        let mut doc = EnvDocument::parse("A=1\nA=2\n");
        assert_eq!(doc.pairs(), vec![("A".into(), "2".into())]);
        assert_eq!(doc.set_value("A", "3"), 2);
        assert_eq!(doc.serialize(), "A=3\nA=3\n");
    }
}

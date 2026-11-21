//! Lightweight `.env` syntax highlighter for Envy's plain-text editor mode.
//!
//! Uses `envy_core::EnvDocument` rules so active keys, values, comments, and
//! disabled (`# KEY=value`) lines match table view classification exactly.

use std::ops::Range;

use envy_core::{is_env_key, EnvDocument, Node};
use gpui_kit::component::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, Rope,
};
use gpui_kit::{Context, FontStyle, HighlightStyle, SharedString, Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenKind {
    Comment,
    DisabledLine,
    DisabledKey,
    Keyword,
    Key,
    Operator,
    String,
    Number,
    Boolean,
    Interpolation,
}

pub struct DotenvHighlighter {
    tokens: Vec<(Range<usize>, TokenKind)>,
}

impl DotenvHighlighter {
    pub fn new() -> Self {
        Self { tokens: Vec::new() }
    }
}

impl InputHighlighter for DotenvHighlighter {
    fn language(&self) -> SharedString {
        SharedString::from("dotenv")
    }

    fn update(
        &mut self,
        _edit: Option<InputEdit>,
        text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut Context<EditorState>,
    ) {
        let src = text.to_string();
        self.tokens = tokenize_dotenv(&src);
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        build_style_runs(&self.tokens, range, resolver)
    }

    fn fold_ranges(&self, _text: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

fn style_for(kind: TokenKind, resolver: &dyn HighlightStyleResolver) -> HighlightStyle {
    match kind {
        TokenKind::Comment => {
            let mut s = resolver.style("comment").unwrap_or_default();
            s.font_style = Some(FontStyle::Italic);
            s
        }
        TokenKind::DisabledLine => {
            let mut s = resolver.style("comment").unwrap_or_default();
            s.font_style = Some(FontStyle::Normal);
            s
        }
        TokenKind::DisabledKey => {
            let mut s = resolver
                .style("comment.doc")
                .or_else(|| resolver.style("comment"))
                .unwrap_or_default();
            s.font_style = Some(FontStyle::Normal);
            s
        }
        TokenKind::Keyword => resolver.style("keyword").unwrap_or_default(),
        TokenKind::Key => resolver
            .style("property")
            .or_else(|| resolver.style("variable"))
            .unwrap_or_default(),
        TokenKind::Operator => resolver
            .style("operator")
            .or_else(|| resolver.style("punctuation"))
            .unwrap_or_default(),
        TokenKind::String => resolver.style("string").unwrap_or_default(),
        TokenKind::Number => resolver.style("number").unwrap_or_default(),
        TokenKind::Boolean => resolver
            .style("boolean")
            .or_else(|| resolver.style("constant"))
            .unwrap_or_default(),
        TokenKind::Interpolation => resolver
            .style("variable.special")
            .or_else(|| resolver.style("constant"))
            .unwrap_or_default(),
    }
}

fn build_style_runs(
    tokens: &[(Range<usize>, TokenKind)],
    range: &Range<usize>,
    resolver: &dyn HighlightStyleResolver,
) -> Vec<(Range<usize>, HighlightStyle)> {
    if range.start >= range.end {
        return Vec::new();
    }
    let mut runs = Vec::new();
    let mut cursor = range.start;

    // Binary search for the first token that may overlap `range`.
    let start_ix = tokens.partition_point(|(r, _)| r.end <= range.start);
    for (tok_range, kind) in &tokens[start_ix..] {
        if tok_range.start >= range.end {
            break;
        }
        let s = tok_range.start.max(cursor);
        let e = tok_range.end.min(range.end);
        if s >= e {
            continue;
        }
        if s > cursor {
            runs.push((cursor..s, HighlightStyle::default()));
        }
        runs.push((s..e, style_for(*kind, resolver)));
        cursor = e;
    }

    if cursor < range.end {
        runs.push((cursor..range.end, HighlightStyle::default()));
    }
    runs
}

fn tokenize_dotenv(src: &str) -> Vec<(Range<usize>, TokenKind)> {
    let doc = EnvDocument::parse(src);
    let mut out = Vec::new();
    let mut offset = 0usize;

    for node in &doc.nodes {
        match node {
            Node::Verbatim(text) => {
                let len = text.len();
                let line_end = strip_eol_len(text);
                let trimmed = text[..line_end].trim_start_matches([' ', '\t']);
                if trimmed.starts_with('#') {
                    let ws = line_end - trimmed.len();
                    out.push(((offset + ws)..(offset + line_end), TokenKind::Comment));
                }
                offset += len;
            }
            Node::Entry(entry) => {
                let raw = entry
                    .raw_str()
                    .map(str::len)
                    .unwrap_or(0)
                    .min(src.len().saturating_sub(offset));
                let slice = &src[offset..offset + raw];
                if entry.disabled {
                    tokenize_disabled_entry(slice, offset, &mut out);
                } else {
                    tokenize_active_entry(slice, offset, &mut out);
                }
                offset += raw;
            }
        }
    }

    out
}

fn strip_eol_len(s: &str) -> usize {
    let mut n = s.len();
    if n > 0 && s.as_bytes()[n - 1] == b'\n' {
        n -= 1;
    }
    if n > 0 && s.as_bytes()[n - 1] == b'\r' {
        n -= 1;
    }
    n
}

fn tokenize_disabled_entry(slice: &str, base: usize, out: &mut Vec<(Range<usize>, TokenKind)>) {
    let line_end = strip_eol_len(slice);
    let line = &slice[..line_end];
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'#' {
        return;
    }
    let hash_start = i;
    i += 1;
    if i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    out.push(((base + hash_start)..(base + i), TokenKind::DisabledLine));

    if let Some(eq_rel) = line[i..].find('=') {
        let key_end = i + eq_rel;
        if key_end > i {
            out.push(((base + i)..(base + key_end), TokenKind::DisabledKey));
        }
        out.push(((base + key_end)..(base + line_end), TokenKind::DisabledLine));
    } else if i < line_end {
        out.push(((base + i)..(base + line_end), TokenKind::DisabledLine));
    }
}

fn tokenize_active_entry(slice: &str, base: usize, out: &mut Vec<(Range<usize>, TokenKind)>) {
    let total_end = strip_eol_len(slice);
    let body = &slice[..total_end];
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }

    // Optional `export ` keyword.
    if body[i..].starts_with("export") {
        let after = i + 6;
        if after < bytes.len() && (bytes[after] == b' ' || bytes[after] == b'\t') {
            out.push(((base + i)..(base + after), TokenKind::Keyword));
            i = after;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
        }
    }

    // Key name up to `=` or whitespace.
    let key_start = i;
    while i < bytes.len() && bytes[i] != b'=' && bytes[i] != b' ' && bytes[i] != b'\t' {
        i += 1;
    }
    if i > key_start && is_env_key(&body[key_start..i]) {
        out.push(((base + key_start)..(base + i), TokenKind::Key));
    }

    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'=' {
        return;
    }
    out.push(((base + i)..(base + i + 1), TokenKind::Operator));
    i += 1;

    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= bytes.len() {
        return;
    }

    if bytes[i] == b'#' {
        out.push(((base + i)..(base + total_end), TokenKind::Comment));
        return;
    }

    if bytes[i] == b'"' || bytes[i] == b'\'' {
        let quote = bytes[i];
        let val_start = i;
        i += 1;
        let mut close_at = None;
        while i < bytes.len() {
            if quote == b'"' && bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == quote {
                close_at = Some(i + 1);
                break;
            }
            i += 1;
        }
        let val_end = close_at.unwrap_or(total_end);
        if quote == b'"' {
            push_with_interpolations(body, val_start, val_end, base, TokenKind::String, out);
        } else {
            out.push(((base + val_start)..(base + val_end), TokenKind::String));
        }
        if val_end < total_end {
            let tail = &body[val_end..];
            if let Some(hash_rel) = tail.find('#') {
                let c_start = val_end + hash_rel;
                out.push(((base + c_start)..(base + total_end), TokenKind::Comment));
            }
        }
        return;
    }

    // Bare value (possibly followed by inline ` # comment`).
    let rest = &body[i..];
    let (val_end, comment_start) = find_bare_inline_comment(rest)
        .map(|rel| {
            let trimmed_val = rest[..rel].trim_end_matches([' ', '\t']).len();
            (i + trimmed_val, Some(i + rel + 1))
        })
        .unwrap_or_else(|| {
            let trimmed_val = rest.trim_end_matches([' ', '\t']).len();
            (i + trimmed_val, None)
        });

    if val_end > i {
        let val_str = &body[i..val_end];
        let kind = classify_bare_value(val_str);
        if kind == TokenKind::String {
            push_with_interpolations(body, i, val_end, base, TokenKind::String, out);
        } else {
            out.push(((base + i)..(base + val_end), kind));
        }
    }

    if let Some(c_start) = comment_start {
        if c_start < total_end {
            out.push(((base + c_start)..(base + total_end), TokenKind::Comment));
        }
    }
}

fn find_bare_inline_comment(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if (bytes[i] == b' ' || bytes[i] == b'\t') && bytes[i + 1] == b'#' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn classify_bare_value(val: &str) -> TokenKind {
    if matches!(
        val.to_ascii_lowercase().as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off"
    ) {
        return TokenKind::Boolean;
    }
    let mut chars = val.chars().peekable();
    if chars.peek() == Some(&'-') {
        chars.next();
    }
    let mut digits = 0usize;
    let mut dots = 0usize;
    let mut valid_num = true;
    for c in chars {
        if c.is_ascii_digit() {
            digits += 1;
        } else if c == '.' && dots == 0 {
            dots += 1;
        } else {
            valid_num = false;
            break;
        }
    }
    if valid_num && digits > 0 {
        TokenKind::Number
    } else {
        TokenKind::String
    }
}

fn push_with_interpolations(
    body: &str,
    start: usize,
    end: usize,
    base: usize,
    default_kind: TokenKind,
    out: &mut Vec<(Range<usize>, TokenKind)>,
) {
    let mut cursor = start;
    let bytes = body.as_bytes();
    let mut i = start;
    while i + 2 < end {
        if bytes[i] == b'$' && bytes[i + 1] == b'{' {
            if let Some(close_rel) = body[i + 2..end].find('}') {
                let interp_end = i + 2 + close_rel + 1;
                if i > cursor {
                    out.push(((base + cursor)..(base + i), default_kind));
                }
                out.push(((base + i)..(base + interp_end), TokenKind::Interpolation));
                cursor = interp_end;
                i = interp_end;
                continue;
            }
        }
        i += 1;
    }
    if cursor < end {
        out.push(((base + cursor)..(base + end), default_kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestResolver;
    impl HighlightStyleResolver for TestResolver {
        fn style(&self, _: &str) -> Option<HighlightStyle> {
            Some(HighlightStyle::default())
        }
    }

    #[test]
    fn tokenizes_comments_disabled_keys_and_active_values_without_overlap() {
        let src = "\
# --- Database ---
DATABASE_URL=\"postgres://${DB_HOST}:5432/app\" # primary
DB_POOL=12
DEBUG=true
# API_URL=http://localhost:8080
broken line
";
        let tokens = tokenize_dotenv(src);
        for pair in tokens.windows(2) {
            assert!(
                pair[0].0.end <= pair[1].0.start,
                "overlapping tokens: {:?} and {:?}",
                pair[0],
                pair[1]
            );
        }
        assert!(tokens.iter().any(|(_, k)| *k == TokenKind::Comment));
        assert!(tokens.iter().any(|(_, k)| *k == TokenKind::Key));
        assert!(tokens.iter().any(|(_, k)| *k == TokenKind::Operator));
        assert!(tokens.iter().any(|(_, k)| *k == TokenKind::String));
        assert!(tokens.iter().any(|(_, k)| *k == TokenKind::Interpolation));
        assert!(tokens.iter().any(|(_, k)| *k == TokenKind::Number));
        assert!(tokens.iter().any(|(_, k)| *k == TokenKind::Boolean));
        assert!(tokens.iter().any(|(_, k)| *k == TokenKind::DisabledKey));

        // Style runs always cover the requested range continuously.
        let runs = build_style_runs(&tokens, &(0..src.len()), &TestResolver);
        assert_eq!(runs.first().unwrap().0.start, 0);
        assert_eq!(runs.last().unwrap().0.end, src.len());
        for pair in runs.windows(2) {
            assert_eq!(pair[0].0.end, pair[1].0.start);
        }
    }
}

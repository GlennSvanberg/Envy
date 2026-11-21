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

/// One assignment (active or commented-out). `raw` is the original source,
/// including its trailing newline when it had one. It is cleared the moment
/// the entry is edited or toggled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub(crate) raw: Option<String>,
    pub key: String,
    pub value: String,
    pub exported: bool,
    pub disabled: bool,
    /// Exact comment prefix when `disabled` is true (e.g. `"#"` or `"# "`).
    pub comment_prefix: String,
    pub quote: QuoteStyle,
    /// Whitespace before `#` / `export` / the key.
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
            disabled: false,
            comment_prefix: String::new(),
            quote,
            leading: String::new(),
            before_eq: String::new(),
            after_eq: String::new(),
            suffix: String::new(),
            had_trailing_newline: true,
        }
    }

    pub fn new_disabled(key: impl Into<String>, value: impl Into<String>) -> Self {
        let mut entry = Self::new(key, value);
        entry.disabled = true;
        entry.comment_prefix = "# ".to_string();
        entry
    }

    pub fn is_dirty(&self) -> bool {
        self.raw.is_none()
    }

    /// Original unmodified source slice when this entry has not been edited.
    pub fn raw_str(&self) -> Option<&str> {
        self.raw.as_deref()
    }

    /// Replace the logical value and drop the original slice so the next save
    /// rewrites this entry only.
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.quote = compatible_quote(self.quote, &self.value);
        if self.disabled && (self.value.contains('\n') || self.value.contains('\r')) {
            self.quote = QuoteStyle::Double;
        }
        self.raw = None;
    }

    pub fn set_key(&mut self, key: impl Into<String>) {
        self.key = key.into();
        self.raw = None;
    }

    /// Toggle whether this assignment is commented out. Drops `raw` only when
    /// the state actually changes, so untouched entries stay byte-for-byte.
    pub fn set_disabled(&mut self, disabled: bool) {
        if self.disabled == disabled {
            return;
        }
        self.disabled = disabled;
        if disabled {
            if self.comment_prefix.is_empty() {
                self.comment_prefix = "# ".to_string();
            }
            if self.value.contains('\n') || self.value.contains('\r') {
                // Keep a commented-out entry on one physical line so reload
                // parses it back as a single disabled entry.
                self.quote = QuoteStyle::Double;
            }
        }
        self.raw = None;
    }

    pub fn toggle_disabled(&mut self) {
        self.set_disabled(!self.disabled);
    }

    fn render(&self, newline: &str) -> String {
        let mut out = String::new();
        out.push_str(&self.leading);
        if self.disabled {
            let prefix = if self.comment_prefix.is_empty() {
                "# "
            } else {
                &self.comment_prefix
            };
            out.push_str(prefix);
        }
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

/// Classification of a [`Node::Verbatim`] slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerbatimKind {
    Bom,
    Blank,
    Comment,
    /// Neither a comment, blank line, nor valid assignment.
    Unknown,
}

/// One line Envy could not parse as an assignment, comment, or blank line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnreadableLine {
    /// 1-based physical line number in the document.
    pub line_number: usize,
    /// Raw line text without trailing `\r`/`\n`.
    pub text: String,
    /// Index into `EnvDocument::nodes`.
    pub node_index: usize,
}

/// One item in the structured table view of an [`EnvDocument`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutItem {
    /// Standalone comment block or decorated section header.
    Section {
        title: String,
        node_indices: Vec<usize>,
    },
    /// Active or disabled entry, with any immediately preceding description
    /// comments attached.
    Entry {
        node_index: usize,
        description: Option<String>,
        description_nodes: Vec<usize>,
        /// True when this disabled entry is grouped beneath an active entry
        /// for the same key.
        related_to_active: bool,
    },
}

/// Structured view of an [`EnvDocument`] for display, search, and inspection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentLayout {
    pub items: Vec<LayoutItem>,
    pub unreadable: Vec<UnreadableLine>,
}

/// Resolved state of one key in a document for diffing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySnapshot {
    pub key: String,
    pub value: String,
    pub disabled: bool,
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
            } else if let Some((entry, next)) = try_parse_disabled_entry(input, i) {
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

    /// Enabled assignments as `KEY=value` lines, in file order.
    ///
    /// Comments, blank lines, disabled entries, and an `export` prefix are
    /// left out. Values that contain spaces or other special characters are
    /// double-quoted so the block can be pasted into an env import.
    pub fn export_enabled(&self) -> String {
        let mut out = String::new();
        for (_, entry) in self.entries() {
            if entry.disabled {
                continue;
            }
            out.push_str(&entry.key);
            out.push('=');
            let style = if needs_quotes(&entry.value) {
                QuoteStyle::Double
            } else {
                QuoteStyle::Bare
            };
            out.push_str(&encode_value(&entry.value, style));
            out.push('\n');
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

    /// Last-wins active `(key, value)` pairs in the order each key first appeared.
    pub fn pairs(&self) -> Vec<(String, String)> {
        let mut order: Vec<String> = Vec::new();
        let mut values: Vec<String> = Vec::new();
        for (_, entry) in self.entries().filter(|(_, e)| !e.disabled) {
            if let Some(pos) = order.iter().position(|k| k == &entry.key) {
                values[pos].clone_from(&entry.value);
            } else {
                order.push(entry.key.clone());
                values.push(entry.value.clone());
            }
        }
        order.into_iter().zip(values).collect()
    }

    /// Resolved state of every unique key (active or disabled) in file order.
    /// When both an active and a disabled entry exist for the same key, the
    /// last active entry wins. Otherwise the last disabled entry wins.
    pub fn resolved_entries(&self) -> Vec<KeySnapshot> {
        let mut order: Vec<String> = Vec::new();
        let mut snapshots: Vec<KeySnapshot> = Vec::new();
        for (_, entry) in self.entries() {
            if let Some(pos) = order.iter().position(|k| k == &entry.key) {
                let current = &mut snapshots[pos];
                if !entry.disabled || current.disabled {
                    current.value.clone_from(&entry.value);
                    current.disabled = entry.disabled;
                }
            } else {
                order.push(entry.key.clone());
                snapshots.push(KeySnapshot {
                    key: entry.key.clone(),
                    value: entry.value.clone(),
                    disabled: entry.disabled,
                });
            }
        }
        snapshots
    }

    /// All unique key names in file order (active and disabled).
    pub fn key_names(&self) -> Vec<String> {
        self.resolved_entries()
            .into_iter()
            .map(|snap| snap.key)
            .collect()
    }

    /// Active (non-disabled) unique key names in file order.
    pub fn active_key_names(&self) -> Vec<String> {
        self.pairs().into_iter().map(|(k, _)| k).collect()
    }

    /// Last active assignment with this key, if any.
    pub fn value_of(&self, key: &str) -> Option<&str> {
        self.entries()
            .filter(|(_, entry)| !entry.disabled && entry.key == key)
            .map(|(_, entry)| entry.value.as_str())
            .last()
    }

    /// Update every active assignment with this key. Returns how many were updated.
    pub fn set_value(&mut self, key: &str, value: &str) -> usize {
        let mut n = 0;
        for node in &mut self.nodes {
            if let Node::Entry(entry) = node {
                if !entry.disabled && entry.key == key {
                    entry.set_value(value);
                    n += 1;
                }
            }
        }
        n
    }

    /// Copy an entry (preserving active/disabled state) into this document.
    pub fn upsert_entry(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
        disabled: bool,
    ) {
        let key = key.into();
        let value = value.into();
        if !disabled {
            if self.set_value(&key, &value) > 0 {
                return;
            }
            if let Some(pos) = self
                .entries()
                .filter(|(_, e)| e.disabled && e.key == key)
                .map(|(i, _)| i)
                .last()
            {
                if let Some(Node::Entry(entry)) = self.nodes.get_mut(pos) {
                    entry.set_value(value);
                    entry.set_disabled(false);
                }
                return;
            }
            self.push_entry(key, value);
        } else {
            let active_indices: Vec<usize> = self
                .entries()
                .filter(|(_, e)| !e.disabled && e.key == key)
                .map(|(i, _)| i)
                .collect();
            if !active_indices.is_empty() {
                for ix in active_indices {
                    if let Some(Node::Entry(entry)) = self.nodes.get_mut(ix) {
                        entry.set_value(value.clone());
                        entry.set_disabled(true);
                    }
                }
                return;
            }
            if let Some(pos) = self
                .entries()
                .filter(|(_, e)| e.disabled && e.key == key)
                .map(|(i, _)| i)
                .last()
            {
                if let Some(Node::Entry(entry)) = self.nodes.get_mut(pos) {
                    entry.set_value(value);
                }
                return;
            }
            self.push_entry_disabled(key, value);
        }
    }

    /// Toggle whether the entry at `node_index` is commented out.
    /// Returns the new disabled state, or `None` if `node_index` is not an entry.
    pub fn toggle_disabled_at(&mut self, node_index: usize) -> Option<bool> {
        match self.nodes.get_mut(node_index) {
            Some(Node::Entry(entry)) => {
                entry.toggle_disabled();
                Some(entry.disabled)
            }
            _ => None,
        }
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

    /// Append an active assignment. Inserts a newline first when the file does not
    /// already end on one, so the new key lands on its own line.
    pub fn push_entry(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.ensure_trailing_newline();
        self.nodes.push(Node::Entry(Entry::new(key, value)));
    }

    /// Append a commented-out assignment.
    pub fn push_entry_disabled(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.ensure_trailing_newline();
        self.nodes
            .push(Node::Entry(Entry::new_disabled(key, value)));
    }

    /// Build the structured table layout: section headers, entries with
    /// attached descriptions, active+disabled pairing, and unreadable lines.
    pub fn layout(&self) -> DocumentLayout {
        let mut raw_items: Vec<RawItem> = Vec::new();
        let mut unreadable: Vec<UnreadableLine> = Vec::new();
        let mut pending: Vec<(usize, CommentLine)> = Vec::new();
        let mut current_line: usize = 1;

        for (node_index, node) in self.nodes.iter().enumerate() {
            match node {
                Node::Verbatim(text) => {
                    let kind = classify_verbatim(text);
                    let line_number = current_line;
                    current_line += count_newlines(text);
                    match kind {
                        VerbatimKind::Bom => {}
                        VerbatimKind::Blank => {
                            flush_pending_sections(&mut pending, &mut raw_items);
                        }
                        VerbatimKind::Comment => {
                            pending.push((node_index, parse_comment_line(text)));
                        }
                        VerbatimKind::Unknown => {
                            flush_pending_sections(&mut pending, &mut raw_items);
                            unreadable.push(UnreadableLine {
                                line_number,
                                text: strip_line_ending(text).to_string(),
                                node_index,
                            });
                        }
                    }
                }
                Node::Entry(entry) => {
                    let rendered = match &entry.raw {
                        Some(raw) => raw.as_str(),
                        None => "",
                    };
                    if entry.raw.is_some() {
                        current_line += count_newlines(rendered);
                    } else {
                        current_line += count_newlines(&entry.render(&self.newline));
                    }

                    // If the pending comment block starts with (or contains) a
                    // decorated section header or rule line, emit everything up
                    // to the last header/rule as section divider(s) and attach
                    // only the remaining plain comment lines as the entry's
                    // description.
                    let split_at = pending
                        .iter()
                        .rposition(|(_, c)| c.is_header || c.is_rule)
                        .map(|ix| ix + 1)
                        .unwrap_or(0);
                    if split_at > 0 {
                        let mut head: Vec<(usize, CommentLine)> =
                            pending.drain(..split_at).collect();
                        flush_pending_sections(&mut head, &mut raw_items);
                    }

                    let (description, description_nodes) = extract_description(&pending);
                    pending.clear();
                    raw_items.push(RawItem::Entry {
                        node_index,
                        description,
                        description_nodes,
                    });
                }
            }
        }
        flush_pending_sections(&mut pending, &mut raw_items);

        // Pair disabled entries with active entries of the same key (disabled
        // shown directly below active, without merging or altering source order
        // on disk).
        let mut active_keys = std::collections::HashSet::new();
        for (_, entry) in self.entries() {
            if !entry.disabled {
                active_keys.insert(entry.key.as_str());
            }
        }

        let mut paired_disabled: std::collections::HashMap<String, Vec<RawEntry>> =
            std::collections::HashMap::new();
        for item in &raw_items {
            if let RawItem::Entry {
                node_index,
                description,
                description_nodes,
            } = item
            {
                if let Some(Node::Entry(entry)) = self.nodes.get(*node_index) {
                    if entry.disabled && active_keys.contains(entry.key.as_str()) {
                        paired_disabled
                            .entry(entry.key.clone())
                            .or_default()
                            .push(RawEntry {
                                node_index: *node_index,
                                description: description.clone(),
                                description_nodes: description_nodes.clone(),
                            });
                    }
                }
            }
        }

        let mut items = Vec::with_capacity(raw_items.len());
        for item in raw_items {
            match item {
                RawItem::Section {
                    title,
                    node_indices,
                } => {
                    items.push(LayoutItem::Section {
                        title,
                        node_indices,
                    });
                }
                RawItem::Entry {
                    node_index,
                    description,
                    description_nodes,
                } => {
                    let Some(Node::Entry(entry)) = self.nodes.get(node_index) else {
                        continue;
                    };
                    if entry.disabled && active_keys.contains(entry.key.as_str()) {
                        // Emitted directly under its active counterpart.
                        continue;
                    }
                    items.push(LayoutItem::Entry {
                        node_index,
                        description,
                        description_nodes,
                        related_to_active: false,
                    });
                    if !entry.disabled {
                        if let Some(siblings) = paired_disabled.remove(&entry.key) {
                            for sibling in siblings {
                                items.push(LayoutItem::Entry {
                                    node_index: sibling.node_index,
                                    description: sibling.description,
                                    description_nodes: sibling.description_nodes,
                                    related_to_active: true,
                                });
                            }
                        }
                    }
                }
            }
        }

        DocumentLayout { items, unreadable }
    }

    pub fn unreadable_lines(&self) -> Vec<UnreadableLine> {
        self.layout().unreadable
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
                disabled: false,
                comment_prefix: String::new(),
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
            disabled: false,
            comment_prefix: String::new(),
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

/// Parse a commented-out assignment such as `#API_URL=https://x` or `# DEBUG=true`.
///
/// Only a single optional ASCII space is allowed after `#`, and the entire
/// remainder of the line must be a clean assignment with a valid env key name.
/// Prose comments (e.g. `# Note: set MODE=fast for dev`), double-hash headers
/// (`## KEY=1`), indented comment bodies (`#   KEY=1`), or unclosed quotes stay
/// verbatim comments.
fn try_parse_disabled_entry(input: &str, start: usize) -> Option<(Entry, usize)> {
    let content_end = line_content_end(input, start);
    let line = &input[start..content_end];
    let (leading, rest) = split_ascii_ws(line);
    let after_hash = rest.strip_prefix('#')?;
    let (comment_prefix, candidate) = if let Some(after_space) = after_hash.strip_prefix(' ') {
        ("# ", after_space)
    } else {
        ("#", after_hash)
    };
    // Reject extra leading whitespace, `##`, or empty comment lines.
    if candidate.is_empty()
        || candidate.starts_with(' ')
        || candidate.starts_with('\t')
        || candidate.starts_with('#')
    {
        return None;
    }

    let (exported, after_export) = strip_export(candidate)?;
    let (key, after_key) = split_key(after_export)?;
    let (before_eq, after_ws) = split_ascii_ws(after_key);
    if !after_ws.starts_with('=') {
        return None;
    }

    let record_end = end_of_physical_line(input, start);
    let raw = input[start..record_end].to_string();
    let had_trailing_newline = raw.ends_with('\n');

    let after_eq_and_value = &after_ws[1..];
    let trimmed_value = after_eq_and_value.trim_start_matches([' ', '\t']);
    if trimmed_value.is_empty() || trimmed_value.starts_with('#') {
        return Some((
            Entry {
                raw: Some(raw),
                key: key.to_string(),
                value: String::new(),
                exported,
                disabled: true,
                comment_prefix: comment_prefix.to_string(),
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

    let (after_eq, value_src) = split_ascii_ws(after_eq_and_value);
    let (value, quote, suffix) = parse_disabled_value(value_src)?;
    Some((
        Entry {
            raw: Some(raw),
            key: key.to_string(),
            value,
            exported,
            disabled: true,
            comment_prefix: comment_prefix.to_string(),
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

/// Single-line value parser for disabled entries. Quoted values must close on
/// the same line and any trailing text after the closing quote must be
/// whitespace or an inline `#` comment; otherwise the line is treated as prose.
fn parse_disabled_value(value_src: &str) -> Option<(String, QuoteStyle, String)> {
    let bytes = value_src.as_bytes();
    if bytes.is_empty() {
        return Some((String::new(), QuoteStyle::Bare, String::new()));
    }
    match bytes[0] {
        b'"' | b'\'' => {
            let style = if bytes[0] == b'"' {
                QuoteStyle::Double
            } else {
                QuoteStyle::Single
            };
            let closer = bytes[0];
            let mut i = 1;
            let mut close_at = None;
            while i < bytes.len() {
                if style == QuoteStyle::Double && bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == closer {
                    close_at = Some(i);
                    break;
                }
                i += 1;
            }
            let close_at = close_at?;
            let after_quote = &value_src[close_at + 1..];
            let trimmed_after = after_quote.trim_start_matches([' ', '\t']);
            if !trimmed_after.is_empty() && !trimmed_after.starts_with('#') {
                return None;
            }
            let inner = &value_src[1..close_at];
            let decoded = match style {
                QuoteStyle::Double => decode_double(inner),
                QuoteStyle::Single | QuoteStyle::Bare => inner.to_string(),
            };
            Some((decoded, style, after_quote.to_string()))
        }
        _ => {
            let (value, suffix) = split_bare(value_src);
            Some((value, QuoteStyle::Bare, suffix))
        }
    }
}

/// Classify a [`Node::Verbatim`] slice.
pub fn classify_verbatim(raw: &str) -> VerbatimKind {
    if raw == "\u{feff}" {
        return VerbatimKind::Bom;
    }
    let line = strip_line_ending(raw);
    let trimmed = line.trim();
    if trimmed.is_empty() {
        VerbatimKind::Blank
    } else if trimmed.starts_with('#') {
        VerbatimKind::Comment
    } else {
        VerbatimKind::Unknown
    }
}

fn strip_line_ending(s: &str) -> &str {
    let s = s.strip_suffix('\n').unwrap_or(s);
    s.strip_suffix('\r').unwrap_or(s)
}

fn count_newlines(s: &str) -> usize {
    s.as_bytes().iter().filter(|&&b| b == b'\n').count()
}

#[derive(Clone, Debug)]
struct CommentLine {
    text: String,
    is_header: bool,
    is_rule: bool,
}

enum RawItem {
    Section {
        title: String,
        node_indices: Vec<usize>,
    },
    Entry {
        node_index: usize,
        description: Option<String>,
        description_nodes: Vec<usize>,
    },
}

struct RawEntry {
    node_index: usize,
    description: Option<String>,
    description_nodes: Vec<usize>,
}

fn is_decoration_char(c: char) -> bool {
    matches!(c, '-' | '=' | '*' | '#' | '─' | '━' | '~')
}

fn parse_comment_line(raw: &str) -> CommentLine {
    let trimmed = strip_line_ending(raw).trim();
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    let after_hashes = trimmed[hashes..].trim();

    if after_hashes.is_empty() {
        return CommentLine {
            text: String::new(),
            is_header: false,
            is_rule: hashes >= 3,
        };
    }

    if after_hashes.len() >= 3
        && after_hashes
            .chars()
            .all(|c| is_decoration_char(c) || c.is_whitespace())
    {
        return CommentLine {
            text: String::new(),
            is_header: false,
            is_rule: true,
        };
    }

    let leading_dec = after_hashes
        .chars()
        .take_while(|&c| is_decoration_char(c))
        .count();
    let decorated = hashes >= 2 || leading_dec >= 2;
    if decorated {
        let stripped =
            after_hashes.trim_matches(|c: char| is_decoration_char(c) || c.is_whitespace());
        if !stripped.is_empty() {
            return CommentLine {
                text: stripped.to_string(),
                is_header: true,
                is_rule: false,
            };
        }
        return CommentLine {
            text: String::new(),
            is_header: false,
            is_rule: true,
        };
    }

    CommentLine {
        text: after_hashes.to_string(),
        is_header: false,
        is_rule: false,
    }
}

fn flush_pending_sections(pending: &mut Vec<(usize, CommentLine)>, out: &mut Vec<RawItem>) {
    if pending.is_empty() {
        return;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut indices: Vec<usize> = Vec::new();
    for (ix, comment) in pending.drain(..) {
        indices.push(ix);
        if !comment.is_rule && !comment.text.is_empty() {
            lines.push(comment.text);
        }
    }
    if !lines.is_empty() {
        out.push(RawItem::Section {
            title: lines.join(" — "),
            node_indices: indices,
        });
    }
}

fn extract_description(pending: &[(usize, CommentLine)]) -> (Option<String>, Vec<usize>) {
    let mut lines: Vec<String> = Vec::new();
    let mut indices: Vec<usize> = Vec::new();
    for (ix, comment) in pending {
        if comment.is_rule {
            continue;
        }
        if !comment.text.is_empty() {
            lines.push(comment.text.clone());
            indices.push(*ix);
        }
    }
    if lines.is_empty() {
        (None, Vec::new())
    } else {
        (Some(lines.join("\n")), indices)
    }
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
    fn export_enabled_is_key_equals_value_without_comments_or_disabled() {
        let doc = EnvDocument::parse(
            "# keep me out\n\
             export HOST=localhost\n\
             # disabled below\n\
             # PORT=1\n\
             NAME=\"Ada Lovelace\"\n\
             EMPTY=\n\
             TOKEN=plain\n",
        );
        assert_eq!(
            doc.export_enabled(),
            "HOST=localhost\nNAME=\"Ada Lovelace\"\nEMPTY=\nTOKEN=plain\n"
        );
        let empty = EnvDocument::parse("# only a comment\n# GONE=1\n");
        assert_eq!(empty.export_enabled(), "");
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

    #[test]
    fn classifies_disabled_prose_header_and_unknown() {
        let input = "\
# --- Database ---
# Primary Postgres connection string
DATABASE_URL=postgres://localhost/app
#API_URL=https://x
# DEBUG=true
# export TRACE=1
# QUOTED=\"hello world\" # inline
# Note: set MODE=fast for dev
#   INDENTED_COMMENT=1
## HEADER_LIKE=1
# UNCLOSED=\"oops
not a valid line
another_broken_token
";
        let doc = EnvDocument::parse(input);
        assert_eq!(doc.serialize(), input);

        // Active + disabled entries parsed cleanly; prose/headers/unknowns stay Verbatim.
        let entries: Vec<(&str, &str, bool)> = doc
            .entries()
            .map(|(_, e)| (e.key.as_str(), e.value.as_str(), e.disabled))
            .collect();
        assert_eq!(
            entries,
            vec![
                ("DATABASE_URL", "postgres://localhost/app", false),
                ("API_URL", "https://x", true),
                ("DEBUG", "true", true),
                ("TRACE", "1", true),
                ("QUOTED", "hello world", true),
            ]
        );

        // `pairs()` only exposes active entries; `key_names()` includes disabled keys.
        assert_eq!(
            doc.pairs(),
            vec![("DATABASE_URL".into(), "postgres://localhost/app".into())]
        );
        assert_eq!(
            doc.key_names(),
            vec!["DATABASE_URL", "API_URL", "DEBUG", "TRACE", "QUOTED"]
        );

        let layout = doc.layout();
        // First item is the decorated section header.
        assert!(matches!(
            &layout.items[0],
            LayoutItem::Section { title, .. } if title == "Database"
        ));
        // Second item is DATABASE_URL with description attached.
        assert!(matches!(
            &layout.items[1],
            LayoutItem::Entry {
                description: Some(desc),
                related_to_active: false,
                ..
            } if desc == "Primary Postgres connection string"
        ));
        // Unreadable lines reported with exact 1-based line numbers.
        assert_eq!(layout.unreadable.len(), 2);
        assert_eq!(layout.unreadable[0].line_number, 12);
        assert_eq!(layout.unreadable[0].text, "not a valid line");
        assert_eq!(layout.unreadable[1].line_number, 13);
        assert_eq!(layout.unreadable[1].text, "another_broken_token");
    }

    #[test]
    fn toggle_round_trips_byte_exact_except_toggled_line() {
        let input = "\
# Header comment

#API_URL=https://x
# DEBUG=true
PORT=3000 # keep inline
untouched garbage line
";
        let mut doc = EnvDocument::parse(input);
        let api_ix = doc
            .entries()
            .find(|(_, e)| e.key == "API_URL")
            .map(|(i, _)| i)
            .unwrap();
        let debug_ix = doc
            .entries()
            .find(|(_, e)| e.key == "DEBUG")
            .map(|(i, _)| i)
            .unwrap();
        let port_ix = doc
            .entries()
            .find(|(_, e)| e.key == "PORT")
            .map(|(i, _)| i)
            .unwrap();

        // Turn `#API_URL=https://x` ON -> removes `#`, touches nothing else.
        assert_eq!(doc.toggle_disabled_at(api_ix), Some(false));
        assert_eq!(
            doc.serialize(),
            input.replace("#API_URL=https://x\n", "API_URL=https://x\n")
        );

        // Turn back OFF -> restores exact `#` prefix!
        assert_eq!(doc.toggle_disabled_at(api_ix), Some(true));
        assert_eq!(doc.serialize(), input);

        // Turn `# DEBUG=true` ON -> removes `# `, touches nothing else.
        assert_eq!(doc.toggle_disabled_at(debug_ix), Some(false));
        assert_eq!(
            doc.serialize(),
            input.replace("# DEBUG=true\n", "DEBUG=true\n")
        );
        // Turn back OFF -> restores `# ` prefix.
        assert_eq!(doc.toggle_disabled_at(debug_ix), Some(true));
        assert_eq!(doc.serialize(), input);

        // Turn active `PORT=3000 # keep inline` OFF -> comments it out instead of deleting.
        assert_eq!(doc.toggle_disabled_at(port_ix), Some(true));
        assert_eq!(
            doc.serialize(),
            input.replace("PORT=3000 # keep inline\n", "# PORT=3000 # keep inline\n")
        );
        // Turn back ON -> exact original bytes.
        assert_eq!(doc.toggle_disabled_at(port_ix), Some(false));
        assert_eq!(doc.serialize(), input);
    }

    #[test]
    fn toggle_preserves_crlf_byte_exact() {
        let input = "# --- Section ---\r\n# API_URL=https://x\r\nPORT=3000\r\nbad line\r\n";
        let mut doc = EnvDocument::parse(input);
        assert_eq!(doc.newline, "\r\n");
        assert_eq!(doc.serialize(), input);

        let api_ix = doc
            .entries()
            .find(|(_, e)| e.key == "API_URL")
            .map(|(i, _)| i)
            .unwrap();
        doc.toggle_disabled_at(api_ix);
        assert_eq!(
            doc.serialize(),
            "# --- Section ---\r\nAPI_URL=https://x\r\nPORT=3000\r\nbad line\r\n"
        );

        let port_ix = doc
            .entries()
            .find(|(_, e)| e.key == "PORT")
            .map(|(i, _)| i)
            .unwrap();
        doc.toggle_disabled_at(port_ix);
        assert_eq!(
            doc.serialize(),
            "# --- Section ---\r\nAPI_URL=https://x\r\n# PORT=3000\r\nbad line\r\n"
        );

        // Line numbers on CRLF files are accurate too.
        let layout = doc.layout();
        assert_eq!(layout.unreadable.len(), 1);
        assert_eq!(layout.unreadable[0].line_number, 4);
        assert_eq!(layout.unreadable[0].text, "bad line");
    }

    #[test]
    fn description_attachment_and_standalone_sections() {
        let input = "\
# Standalone intro block

# --- Database ---
# Line 1 of description
# Line 2 of description
DATABASE_URL=postgres://localhost/db

# Detached comment block
# with two lines

PORT=3000
# Direct description on disabled entry
# DEBUG=true
";
        let doc = EnvDocument::parse(input);
        let layout = doc.layout();
        assert_eq!(layout.unreadable.len(), 0);

        // Verify order and classification:
        // 0: Section "Standalone intro block"
        // 1: Section "Database"
        // 2: Entry DATABASE_URL (description = "Line 1...\nLine 2...")
        // 3: Section "Detached comment block — with two lines"
        // 4: Entry PORT (no description)
        // 5: Entry DEBUG (description = "Direct description on disabled entry")
        assert_eq!(layout.items.len(), 6);
        assert!(matches!(
            &layout.items[0],
            LayoutItem::Section { title, .. } if title == "Standalone intro block"
        ));
        assert!(matches!(
            &layout.items[1],
            LayoutItem::Section { title, .. } if title == "Database"
        ));
        assert!(matches!(
            &layout.items[2],
            LayoutItem::Entry { description: Some(d), .. }
                if d == "Line 1 of description\nLine 2 of description"
        ));
        assert!(matches!(
            &layout.items[3],
            LayoutItem::Section { title, .. } if title == "Detached comment block — with two lines"
        ));
        assert!(matches!(
            &layout.items[4],
            LayoutItem::Entry {
                description: None,
                ..
            }
        ));
        assert!(matches!(
            &layout.items[5],
            LayoutItem::Entry { description: Some(d), .. }
                if d == "Direct description on disabled entry"
        ));
    }

    #[test]
    fn active_and_disabled_same_key_are_paired_without_merging() {
        let input = "\
# API_URL=https://staging.example.com
API_URL=https://prod.example.com
PORT=3000
";
        let doc = EnvDocument::parse(input);
        // Untouched file stays byte-for-byte identical.
        assert_eq!(doc.serialize(), input);

        let layout = doc.layout();
        assert_eq!(layout.items.len(), 3);
        let entry_at = |item: &LayoutItem| match item {
            LayoutItem::Entry {
                node_index,
                related_to_active,
                ..
            } => {
                let Node::Entry(e) = &doc.nodes[*node_index] else {
                    panic!("expected entry")
                };
                (
                    e.key.as_str(),
                    e.value.as_str(),
                    e.disabled,
                    *related_to_active,
                )
            }
            _ => panic!("expected entry"),
        };
        assert_eq!(
            entry_at(&layout.items[0]),
            ("API_URL", "https://prod.example.com", false, false)
        );
        assert_eq!(
            entry_at(&layout.items[1]),
            ("API_URL", "https://staging.example.com", true, true)
        );
        assert_eq!(entry_at(&layout.items[2]), ("PORT", "3000", false, false));
    }

    #[test]
    fn unreadable_line_numbers_account_for_multiline_values() {
        let input = "\
CERT=\"line1
line2
line3\"
broken_line_on_4
";
        let doc = EnvDocument::parse(input);
        let unreadable = doc.unreadable_lines();
        assert_eq!(unreadable.len(), 1);
        assert_eq!(unreadable[0].line_number, 4);
        assert_eq!(unreadable[0].text, "broken_line_on_4");
    }
}

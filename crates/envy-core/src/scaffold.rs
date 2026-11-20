//! Create a new `.env` from an example file.
//!
//! Comments, order, blank lines, `export`, and spacing survive. Values are
//! cleared unless they are obviously a placeholder or a non-secret default
//! (booleans, numbers, localhost, `example.com`). Real-looking secrets become
//! empty. The result is a new document; the example file is not modified.

use crate::document::{EnvDocument, Node, QuoteStyle};

/// True when an example value is safe to copy into a new file.
pub fn is_safe_default(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let lower = value.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "none"
    ) {
        return true;
    }
    if value.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    const HINTS: &[&str] = &[
        "localhost",
        "127.0.0.1",
        "0.0.0.0",
        "example.com",
        "example.org",
        "example",
        "changeme",
        "change-me",
        "change_me",
        "placeholder",
        "replace-me",
        "replace_me",
        "your-",
        "your_",
        "<your",
        "todo",
        "sample",
        "xxx",
    ];
    HINTS.iter().any(|hint| lower.contains(hint))
}

pub fn from_example(example: &EnvDocument) -> EnvDocument {
    let nodes = example
        .nodes
        .iter()
        .map(|node| match node {
            Node::Verbatim(text) => Node::Verbatim(text.clone()),
            Node::Entry(entry) => {
                let mut next = entry.clone();
                if is_safe_default(&entry.value) {
                    // Keep the original slice, value included.
                    next.raw = entry.raw.clone();
                } else {
                    next.value.clear();
                    next.quote = QuoteStyle::Bare;
                    next.raw = None;
                }
                Node::Entry(next)
            }
        })
        .collect();
    EnvDocument {
        nodes,
        newline: example.newline.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empties_secrets_and_keeps_comments_order_and_defaults() {
        let example = EnvDocument::parse(
            "\
# App\n\
APP_NAME=example-service\n\
PORT=3000\n\
DEBUG=true\n\
\n\
# Secrets — replace these\n\
export API_TOKEN=\"super-secret\"\n\
DATABASE_URL=\"postgres://user:pass@db/app\"\n\
HOST=localhost\n\
PLACEHOLDER=your-api-key\n\
",
        );
        let doc = from_example(&example);
        let text = doc.serialize();
        assert!(text.starts_with("# App\nAPP_NAME=example-service\nPORT=3000\nDEBUG=true\n"));
        assert!(text.contains("# Secrets — replace these\n"));
        assert!(text.contains("export API_TOKEN=\n") || text.contains("export API_TOKEN=\"\"\n"));
        assert!(text.contains("HOST=localhost\n"));
        assert!(text.contains("PLACEHOLDER=your-api-key\n"));
        assert_eq!(doc.value_of("API_TOKEN"), Some(""));
        assert_eq!(doc.value_of("DATABASE_URL"), Some(""));
        assert_eq!(doc.value_of("PORT"), Some("3000"));
        assert!(!text.contains("super-secret"));
        assert!(!text.contains("postgres://user:pass"));
        // Untouched safe lines are the original bytes, including the blank line.
        assert!(text.contains("DEBUG=true\n\n# Secrets"));
    }

    #[test]
    fn preserves_crlf_on_kept_lines() {
        let example = EnvDocument::parse("PORT=1\r\nTOKEN=sekrit\r\n");
        let text = from_example(&example).serialize();
        assert_eq!(text, "PORT=1\r\nTOKEN=\r\n");
    }
}

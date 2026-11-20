//! `envy://` links.
//!
//! ```text
//! envy://project/<repo-name>
//! envy://project/<repo-name>/<file>
//! envy://project/<repo-name>/<file>?key=KEY
//! ```
//!
//! `<repo-name>` and `<file>` are percent-encoded path segments. A file may
//! itself contain slashes (`subdir/.env`); everything after the repo segment
//! is the file path.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deeplink {
    pub repo: String,
    /// File name or relative path inside the repo. `None` opens the project.
    pub file: Option<String>,
    pub key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeeplinkError {
    pub message: String,
}

impl fmt::Display for DeeplinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DeeplinkError {}

impl Deeplink {
    pub fn parse(input: &str) -> Result<Self, DeeplinkError> {
        let input = input.trim();
        let rest = input
            .strip_prefix("envy://")
            .or_else(|| input.strip_prefix("envy:"))
            .ok_or_else(|| DeeplinkError {
                message: "link must start with envy://".into(),
            })?;
        let (path, query) = split_query(rest);
        let path = path.trim_start_matches('/');
        let mut segments = path.split('/').filter(|segment| !segment.is_empty());
        let kind = segments.next().ok_or_else(|| DeeplinkError {
            message: "link is missing a path".into(),
        })?;
        if kind != "project" {
            return Err(DeeplinkError {
                message: format!("unknown envy link '{kind}'"),
            });
        }
        let repo = segments.next().ok_or_else(|| DeeplinkError {
            message: "link is missing a project name".into(),
        })?;
        let repo = percent_decode(repo)?;
        if repo.is_empty() {
            return Err(DeeplinkError {
                message: "project name is empty".into(),
            });
        }
        let file_segments: Vec<&str> = segments.collect();
        let file = if file_segments.is_empty() {
            None
        } else {
            let decoded: Result<Vec<_>, _> =
                file_segments.iter().map(|s| percent_decode(s)).collect();
            let decoded = decoded?;
            Some(decoded.join("/"))
        };
        let key = query.and_then(|q| query_param(q, "key"));
        Ok(Self { repo, file, key })
    }
}

fn split_query(input: &str) -> (&str, Option<&str>) {
    match input.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (input, None),
    }
}

fn query_param(query: &str, name: &str) -> Option<String> {
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key == name {
            return percent_decode(value).ok();
        }
    }
    None
}

fn percent_decode(input: &str) -> Result<String, DeeplinkError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(DeeplinkError {
                        message: "truncated percent-escape".into(),
                    });
                }
                let hex = &input[i + 1..i + 3];
                let value = u8::from_str_radix(hex, 16).map_err(|_| DeeplinkError {
                    message: format!("bad percent-escape '%{hex}'"),
                })?;
                out.push(value);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| DeeplinkError {
        message: "percent-decoded text is not utf-8".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_file_and_key() {
        let link = Deeplink::parse("envy://project/my-repo/.env.local?key=DATABASE_URL").unwrap();
        assert_eq!(link.repo, "my-repo");
        assert_eq!(link.file.as_deref(), Some(".env.local"));
        assert_eq!(link.key.as_deref(), Some("DATABASE_URL"));
    }

    #[test]
    fn project_only() {
        let link = Deeplink::parse("envy://project/billing").unwrap();
        assert_eq!(link.repo, "billing");
        assert!(link.file.is_none());
        assert!(link.key.is_none());
    }

    #[test]
    fn encoded_repo_and_nested_file() {
        let link =
            Deeplink::parse("envy://project/my%20repo/apps%2Fweb/.env?key=FOO%20BAR").unwrap();
        assert_eq!(link.repo, "my repo");
        assert_eq!(link.file.as_deref(), Some("apps/web/.env"));
        assert_eq!(link.key.as_deref(), Some("FOO BAR"));
    }

    #[test]
    fn rejects_other_schemes() {
        assert!(Deeplink::parse("https://example.com").is_err());
        assert!(Deeplink::parse("envy://settings").is_err());
    }
}

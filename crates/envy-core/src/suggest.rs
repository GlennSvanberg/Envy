//! Key-name autocomplete suggestions.
//!
//! Suggestions are ranked in memory from discovered project key indexes (never
//! reading values from disk):
//! 1. Keys present in sibling env files of the same project but missing in the
//!    open file.
//! 2. Keys present in other discovered projects.
//! 3. Common well-known environment variable names.

use std::collections::HashSet;
use std::path::Path;

use crate::discover::Project;

const COMMON_KEYS: &[&str] = &[
    "DATABASE_URL",
    "REDIS_URL",
    "PORT",
    "HOST",
    "NODE_ENV",
    "LOG_LEVEL",
    "DEBUG",
    "API_URL",
    "API_KEY",
    "APP_URL",
    "APP_ENV",
    "JWT_SECRET",
    "SESSION_SECRET",
    "SECRET_KEY",
    "SENTRY_DSN",
    "NEXT_PUBLIC_API_URL",
    "VITE_API_URL",
    "CORS_ORIGIN",
    "STRIPE_SECRET_KEY",
    "STRIPE_WEBHOOK_SECRET",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_REGION",
    "S3_BUCKET",
    "SMTP_HOST",
    "SMTP_PORT",
    "SMTP_USER",
    "SMTP_PASSWORD",
    "POSTGRES_USER",
    "POSTGRES_PASSWORD",
    "POSTGRES_DB",
    "PGHOST",
    "PGPORT",
    "TZ",
];

pub fn common_env_keys() -> &'static [&'static str] {
    COMMON_KEYS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SuggestionSource {
    /// Missing key from a sibling env file in the same project.
    Sibling,
    /// Key found in another discovered project.
    OtherProject,
    /// Well-known environment variable name.
    Common,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySuggestion {
    pub key: String,
    pub source: SuggestionSource,
    pub score: i32,
}

/// Rank autocomplete suggestions for a key name being added or renamed.
///
/// `existing_keys` are keys already present in the current file (excluded from
/// results). Never reads or returns values.
pub fn suggest_keys(
    projects: &[Project],
    project_ix: Option<usize>,
    current_file: Option<&Path>,
    existing_keys: &[String],
    query: &str,
    limit: usize,
) -> Vec<KeySuggestion> {
    if limit == 0 {
        return Vec::new();
    }

    let query = query.trim();
    let existing: HashSet<&str> = existing_keys.iter().map(String::as_str).collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut candidates: Vec<(String, SuggestionSource)> = Vec::new();

    // Tier 0: sibling files in the same project.
    if let Some(project) = project_ix.and_then(|ix| projects.get(ix)) {
        for file in &project.files {
            if current_file.is_some_and(|cur| file.path == cur) {
                continue;
            }
            for key in &file.keys {
                if existing.contains(key.as_str()) || !seen.insert(key.clone()) {
                    continue;
                }
                candidates.push((key.clone(), SuggestionSource::Sibling));
            }
        }
    }

    // Tier 1: keys from other discovered projects.
    for (ix, project) in projects.iter().enumerate() {
        if project_ix == Some(ix) {
            continue;
        }
        for file in &project.files {
            for key in &file.keys {
                if existing.contains(key.as_str()) || !seen.insert(key.clone()) {
                    continue;
                }
                candidates.push((key.clone(), SuggestionSource::OtherProject));
            }
        }
    }

    // Tier 2: common well-known env key names.
    for &key in COMMON_KEYS {
        if existing.contains(key) || !seen.insert(key.to_string()) {
            continue;
        }
        candidates.push((key.to_string(), SuggestionSource::Common));
    }

    let mut ranked: Vec<KeySuggestion> = candidates
        .into_iter()
        .filter_map(|(key, source)| {
            let match_score = if query.is_empty() {
                0
            } else {
                score_key(&key, query)?
            };
            let tier_bonus = match source {
                SuggestionSource::Sibling => 300,
                SuggestionSource::OtherProject => 150,
                SuggestionSource::Common => 0,
            };
            Some(KeySuggestion {
                key,
                source,
                score: match_score + tier_bonus,
            })
        })
        .collect();

    if query.is_empty() {
        ranked.sort_by(|a, b| a.source.cmp(&b.source).then(a.key.cmp(&b.key)));
    } else {
        ranked.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(a.source.cmp(&b.source))
                .then(a.key.cmp(&b.key))
        });
    }
    ranked.truncate(limit);
    ranked
}

/// Byte range of the first contiguous case-insensitive match of `query` in `key`,
/// used to lightly emphasize the matched prefix or substring in the UI.
pub fn match_span(key: &str, query: &str) -> Option<(usize, usize)> {
    let query = query.trim();
    if query.is_empty() || !key.is_ascii() || !query.is_ascii() {
        return None;
    }
    let hay = key.to_ascii_lowercase();
    let needle = query.to_ascii_lowercase();
    let start = hay.find(&needle)?;
    Some((start, start + needle.len()))
}

fn score_key(key: &str, query: &str) -> Option<i32> {
    let hay = key.to_ascii_lowercase();
    let needle = query.to_ascii_lowercase();
    if hay == needle {
        return Some(1_000);
    }
    if hay.starts_with(&needle) {
        let len_penalty = (hay.len().saturating_sub(needle.len())).min(150) as i32;
        return Some(850 - len_penalty);
    }
    if let Some(pos) = hay.find(&needle) {
        return Some(600 - pos.min(300) as i32);
    }
    subsequence_score(&hay, &needle)
}

fn subsequence_score(hay: &str, needle: &str) -> Option<i32> {
    let hay_chars: Vec<char> = hay.chars().collect();
    let mut at = 0;
    let mut streak = 0;
    let mut best_streak = 0;
    let mut score = 0;
    for needle_char in needle.chars() {
        let mut found = None;
        for (offset, hay_char) in hay_chars.iter().enumerate().skip(at) {
            if *hay_char == needle_char {
                found = Some(offset);
                break;
            }
        }
        let offset = found?;
        if offset == at {
            streak += 1;
            best_streak = best_streak.max(streak);
        } else {
            streak = 1;
        }
        at = offset + 1;
        score += 8;
    }
    Some(100 + score + best_streak * 15)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::EnvFile;
    use std::path::PathBuf;

    fn sample_projects() -> Vec<Project> {
        vec![
            Project {
                root: PathBuf::from("/work/api"),
                name: "api".into(),
                branch: "main".into(),
                missing_vs_example: true,
                files: vec![
                    EnvFile {
                        path: PathBuf::from("/work/api/.env.example"),
                        name: ".env.example".into(),
                        keys: vec![
                            "DATABASE_URL".into(),
                            "SENTRY_DSN".into(),
                            "STRIPE_KEY".into(),
                        ],
                        active_keys: vec![
                            "DATABASE_URL".into(),
                            "SENTRY_DSN".into(),
                            "STRIPE_KEY".into(),
                        ],
                        prod: false,
                        missing_vs_example: false,
                    },
                    EnvFile {
                        path: PathBuf::from("/work/api/.env.local"),
                        name: ".env.local".into(),
                        keys: vec!["DATABASE_URL".into()],
                        active_keys: vec!["DATABASE_URL".into()],
                        prod: false,
                        missing_vs_example: true,
                    },
                    EnvFile {
                        path: PathBuf::from("/work/api/.env.production"),
                        name: ".env.production".into(),
                        keys: vec!["DATABASE_URL".into(), "PROD_FLAG".into()],
                        active_keys: vec!["DATABASE_URL".into(), "PROD_FLAG".into()],
                        prod: true,
                        missing_vs_example: true,
                    },
                ],
            },
            Project {
                root: PathBuf::from("/work/worker"),
                name: "worker".into(),
                branch: "main".into(),
                missing_vs_example: false,
                files: vec![EnvFile {
                    path: PathBuf::from("/work/worker/.env"),
                    name: ".env".into(),
                    keys: vec!["REDIS_URL".into(), "QUEUE_NAME".into()],
                    active_keys: vec!["REDIS_URL".into(), "QUEUE_NAME".into()],
                    prod: false,
                    missing_vs_example: false,
                }],
            },
        ]
    }

    #[test]
    fn empty_query_ranks_sibling_then_other_project_then_common() {
        let projects = sample_projects();
        let existing = vec!["DATABASE_URL".to_string()];
        let results = suggest_keys(
            &projects,
            Some(0),
            Some(Path::new("/work/api/.env.local")),
            &existing,
            "",
            10,
        );
        // Existing key is excluded.
        assert!(!results.iter().any(|s| s.key == "DATABASE_URL"));

        // First 3 are missing sibling keys (PROD_FLAG, SENTRY_DSN, STRIPE_KEY).
        assert_eq!(results[0].source, SuggestionSource::Sibling);
        assert_eq!(results[1].source, SuggestionSource::Sibling);
        assert_eq!(results[2].source, SuggestionSource::Sibling);
        assert!(results.iter().any(|s| s.key == "SENTRY_DSN"));
        assert!(results.iter().any(|s| s.key == "PROD_FLAG"));

        // Next 2 are from the other project (worker).
        assert_eq!(results[3].source, SuggestionSource::OtherProject);
        assert_eq!(results[4].source, SuggestionSource::OtherProject);

        // Remaining come from common well-known keys.
        assert_eq!(results[5].source, SuggestionSource::Common);
    }

    #[test]
    fn query_matches_prefix_and_fuzzy_while_favoring_sibling_keys() {
        let projects = sample_projects();
        let existing = vec!["DATABASE_URL".to_string()];
        let results = suggest_keys(
            &projects,
            Some(0),
            Some(Path::new("/work/api/.env.local")),
            &existing,
            "str",
            5,
        );
        // STRIPE_KEY (Sibling) outranks STRIPE_SECRET_KEY (Common).
        assert_eq!(results[0].key, "STRIPE_KEY");
        assert_eq!(results[0].source, SuggestionSource::Sibling);
        assert!(results.iter().any(|s| s.key == "STRIPE_SECRET_KEY"));
        assert_eq!(match_span("STRIPE_KEY", "str"), Some((0, 3)));
        assert_eq!(match_span("MY_STRIPE_KEY", "str"), Some((3, 6)));

        // Fuzzy subsequence matches QUEUE_NAME from other project.
        let fuzzy = suggest_keys(
            &projects,
            Some(0),
            Some(Path::new("/work/api/.env.local")),
            &existing,
            "qnm",
            5,
        );
        assert_eq!(fuzzy[0].key, "QUEUE_NAME");
        assert_eq!(fuzzy[0].source, SuggestionSource::OtherProject);
    }
}

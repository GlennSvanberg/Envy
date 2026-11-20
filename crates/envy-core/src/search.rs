//! In-memory search across projects, files, and key names.
//!
//! The index is the [`crate::discover::Project`] list, which stores key names
//! and not values. A query does not touch the disk.

use crate::discover::Project;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchKind {
    Command,
    Project,
    File,
    Key,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchCommand {
    pub id: String,
    pub label: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub kind: SearchKind,
    pub label: String,
    pub detail: String,
    pub score: i32,
    pub command: Option<String>,
    pub project: Option<usize>,
    pub file: Option<usize>,
    pub key: Option<String>,
}

pub fn search(
    projects: &[Project],
    commands: &[SearchCommand],
    query: &str,
    limit: usize,
) -> Vec<SearchHit> {
    let query = query.trim();
    let mut hits = Vec::new();
    if query.is_empty() {
        for command in commands {
            hits.push(SearchHit {
                kind: SearchKind::Command,
                label: command.label.clone(),
                detail: command.detail.clone(),
                score: 1,
                command: Some(command.id.clone()),
                project: None,
                file: None,
                key: None,
            });
        }
        for (index, project) in projects.iter().enumerate() {
            hits.push(project_hit(index, project, 1));
        }
        hits.truncate(limit);
        return hits;
    }

    for command in commands {
        if let Some(score) = score_text(&command.label, query) {
            hits.push(SearchHit {
                kind: SearchKind::Command,
                label: command.label.clone(),
                detail: command.detail.clone(),
                score: score + 20,
                command: Some(command.id.clone()),
                project: None,
                file: None,
                key: None,
            });
        }
    }
    for (project_index, project) in projects.iter().enumerate() {
        if let Some(score) = score_text(&project.name, query) {
            hits.push(project_hit(project_index, project, score + 30));
        }
        for (file_index, file) in project.files.iter().enumerate() {
            if let Some(score) = score_text(&file.name, query) {
                hits.push(SearchHit {
                    kind: SearchKind::File,
                    label: file.name.clone(),
                    detail: format!("{} · {}", project.name, project.branch),
                    score: score + 10,
                    command: None,
                    project: Some(project_index),
                    file: Some(file_index),
                    key: None,
                });
            }
            for key in &file.keys {
                if let Some(score) = score_text(key, query) {
                    hits.push(SearchHit {
                        kind: SearchKind::Key,
                        label: key.clone(),
                        detail: format!("{} · {}", project.name, file.name),
                        score,
                        command: None,
                        project: Some(project_index),
                        file: Some(file_index),
                        key: Some(key.clone()),
                    });
                }
            }
        }
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.label.cmp(&b.label)));
    hits.truncate(limit);
    hits
}

fn project_hit(index: usize, project: &Project, score: i32) -> SearchHit {
    SearchHit {
        kind: SearchKind::Project,
        label: project.name.clone(),
        detail: format!("{} · {} files", project.branch, project.files.len()),
        score,
        command: None,
        project: Some(index),
        file: None,
        key: None,
    }
}

/// Higher is better. `None` when the query is not a subsequence.
fn score_text(haystack: &str, needle: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(1);
    }
    let hay = haystack.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    if hay == needle {
        return Some(1_000);
    }
    if hay.starts_with(&needle) {
        return Some(800);
    }
    if let Some(pos) = hay.find(&needle) {
        return Some(600 - pos.min(400) as i32);
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

    fn project() -> Project {
        Project {
            root: PathBuf::from("/work/billing"),
            name: "billing".into(),
            branch: "main".into(),
            missing_vs_example: true,
            files: vec![EnvFile {
                path: PathBuf::from("/work/billing/.env.local"),
                name: ".env.local".into(),
                keys: vec!["DATABASE_URL".into(), "STRIPE_KEY".into()],
                prod: false,
                missing_vs_example: true,
            }],
        }
    }

    #[test]
    fn finds_a_key_without_scanning_values() {
        let hits = search(&[project()], &[], "data", 10);
        assert_eq!(hits[0].label, "DATABASE_URL");
        assert_eq!(hits[0].kind, SearchKind::Key);
        assert!(hits[0].detail.contains("billing"));
    }

    #[test]
    fn empty_query_lists_commands_then_projects() {
        let commands = [SearchCommand {
            id: "add-folder".into(),
            label: "Add folder".into(),
            detail: "Scan a directory".into(),
        }];
        let hits = search(&[project()], &commands, "", 10);
        assert_eq!(hits[0].kind, SearchKind::Command);
        assert_eq!(hits[1].kind, SearchKind::Project);
    }

    #[test]
    fn exact_key_outranks_a_fuzzy_project() {
        let hits = search(&[project()], &[], "stripe_key", 10);
        assert_eq!(hits[0].label, "STRIPE_KEY");
    }
}

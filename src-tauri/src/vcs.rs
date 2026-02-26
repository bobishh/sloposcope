use crate::jj;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub id: String,
    pub description: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bookmark {
    pub name: String,
    pub id: String,
}

pub enum VCSEngine {
    JJ,
    Git,
    None,
}

pub fn detect_engine(repo: &Path) -> VCSEngine {
    // Prefer Git when both metadata dirs exist; many repos are primarily Git
    // and may have experimental JJ metadata present.
    if repo.join(".git").is_dir() {
        VCSEngine::Git
    } else if repo.join(".jj").is_dir() {
        VCSEngine::JJ
    } else {
        VCSEngine::None
    }
}

pub fn get_changes(repo: &Path, limit: usize, before_id: Option<String>) -> Vec<Change> {
    let started = Instant::now();
    let engine = detect_engine(repo);
    let changes = match engine {
        VCSEngine::JJ => jj::changes(repo, limit, before_id),
        VCSEngine::Git => get_git_changes(repo, limit, before_id),
        VCSEngine::None => vec![],
    };
    if crate::debug_enabled() {
        let engine_label = match engine {
            VCSEngine::JJ => "jj",
            VCSEngine::Git => "git",
            VCSEngine::None => "none",
        };
        println!(
            "[BACKEND][vcs] get_changes engine={} count={} took={}ms",
            engine_label,
            changes.len(),
            started.elapsed().as_millis()
        );
    }
    changes
}

pub fn get_bookmarks(repo: &Path) -> Vec<Bookmark> {
    let started = Instant::now();
    let engine = detect_engine(repo);
    let bookmarks = match engine {
        VCSEngine::JJ => jj::bookmarks(repo),
        VCSEngine::Git => get_git_branches(repo),
        VCSEngine::None => vec![],
    };
    if crate::debug_enabled() {
        let engine_label = match engine {
            VCSEngine::JJ => "jj",
            VCSEngine::Git => "git",
            VCSEngine::None => "none",
        };
        println!(
            "[BACKEND][vcs] get_bookmarks engine={} count={} took={}ms",
            engine_label,
            bookmarks.len(),
            started.elapsed().as_millis()
        );
    }
    bookmarks
}

pub fn get_current_branch(repo: &Path) -> String {
    let started = Instant::now();
    let engine = detect_engine(repo);
    let branch = match engine {
        VCSEngine::JJ => {
            if crate::debug_enabled() {
                println!("[BACKEND][vcs] running jj current-branch command");
            }
            let output = std::process::Command::new("jj")
                .args([
                    "--no-pager",
                    "log",
                    "--no-graph",
                    "-r",
                    "@",
                    "-T",
                    "bookmarks.join(', ')",
                ])
                .current_dir(repo)
                .output();
            if let Ok(o) = output {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() {
                    "@".to_string()
                } else {
                    s
                }
            } else {
                "@".into()
            }
        }
        VCSEngine::Git => {
            if crate::debug_enabled() {
                println!("[BACKEND][vcs] running git current-branch command");
            }
            let output = std::process::Command::new("git")
                .args(["--no-pager", "rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(repo)
                .output();
            if let Ok(o) = output {
                String::from_utf8_lossy(&o.stdout).trim().to_string()
            } else {
                "HEAD".into()
            }
        }
        VCSEngine::None => "".into(),
    };
    if crate::debug_enabled() {
        let engine_label = match engine {
            VCSEngine::JJ => "jj",
            VCSEngine::Git => "git",
            VCSEngine::None => "none",
        };
        println!(
            "[BACKEND][vcs] get_current_branch engine={} value='{}' took={}ms",
            engine_label,
            branch,
            started.elapsed().as_millis()
        );
    }
    branch
}

pub fn get_current_revision(repo: &Path) -> String {
    let started = Instant::now();
    let engine = detect_engine(repo);
    let revision = match engine {
        VCSEngine::JJ => {
            if crate::debug_enabled() {
                println!("[BACKEND][vcs] running jj current-revision command");
            }
            let output = std::process::Command::new("jj")
                .args([
                    "--no-pager",
                    "log",
                    "--no-graph",
                    "-r",
                    "@",
                    "-T",
                    "commit_id.short()",
                ])
                .current_dir(repo)
                .output();
            if let Ok(o) = output {
                String::from_utf8_lossy(&o.stdout).trim().to_string()
            } else {
                String::new()
            }
        }
        VCSEngine::Git => {
            if crate::debug_enabled() {
                println!("[BACKEND][vcs] running git current-revision command");
            }
            let output = std::process::Command::new("git")
                .args(["--no-pager", "rev-parse", "--verify", "HEAD"])
                .current_dir(repo)
                .output();
            if let Ok(o) = output {
                String::from_utf8_lossy(&o.stdout).trim().to_string()
            } else {
                String::new()
            }
        }
        VCSEngine::None => String::new(),
    };
    if crate::debug_enabled() {
        let engine_label = match engine {
            VCSEngine::JJ => "jj",
            VCSEngine::Git => "git",
            VCSEngine::None => "none",
        };
        println!(
            "[BACKEND][vcs] get_current_revision engine={} value='{}' took={}ms",
            engine_label,
            if revision.len() > 12 {
                &revision[..12]
            } else {
                &revision
            },
            started.elapsed().as_millis()
        );
    }
    revision
}

pub fn get_changed_files(repo: &Path, since: &str) -> HashMap<String, String> {
    let started = Instant::now();
    let engine = detect_engine(repo);
    let mut changed = match engine {
        VCSEngine::JJ => jj::changed_files(repo, since),
        VCSEngine::Git => get_git_changed_files(repo, since),
        VCSEngine::None => HashMap::new(),
    };
    let before_filter = changed.len();
    changed.retain(|path, _| !should_ignore_graph_path(path));
    if crate::debug_enabled() {
        let engine_label = match engine {
            VCSEngine::JJ => "jj",
            VCSEngine::Git => "git",
            VCSEngine::None => "none",
        };
        println!(
            "[BACKEND][vcs] get_changed_files engine={} since='{}' count={} filtered_out={} took={}ms",
            engine_label,
            since,
            changed.len(),
            before_filter.saturating_sub(changed.len()),
            started.elapsed().as_millis()
        );
    }
    changed
}

fn should_ignore_graph_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").trim_start_matches("./").to_string();
    if normalized.is_empty() {
        return true;
    }

    for segment in normalized.split('/') {
        match segment {
            ".git" | ".jj" | "node_modules" | "target" | "_build" | "deps" | "dist"
            | ".svelte-kit" | ".output" => return true,
            _ => {}
        }
    }

    normalized.starts_with("src-tauri/gen/")
}

pub fn get_file_diff(repo: &Path, file: &str, since: Option<&str>) -> String {
    let started = Instant::now();
    let engine = detect_engine(repo);
    let diff = match engine {
        VCSEngine::JJ => jj::file_diff(repo, file, since),
        VCSEngine::Git => get_git_file_diff(repo, file, since),
        VCSEngine::None => String::new(),
    };
    if crate::debug_enabled() {
        let engine_label = match engine {
            VCSEngine::JJ => "jj",
            VCSEngine::Git => "git",
            VCSEngine::None => "none",
        };
        println!(
            "[BACKEND][vcs] get_file_diff engine={} file={} bytes={} took={}ms",
            engine_label,
            file,
            diff.len(),
            started.elapsed().as_millis()
        );
    }
    diff
}

// --- Git Implementation ---

fn get_git_changes(repo: &Path, limit: usize, before_id: Option<String>) -> Vec<Change> {
    let started = Instant::now();
    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/git] get_git_changes start limit={} before_id={:?}",
            limit, before_id
        );
        println!("[BACKEND][vcs/git] running git log for commit history");
    }
    let output = std::process::Command::new("git")
        .args(["--no-pager", "log", "--pretty=format:%h\t%s\t%ai"])
        .current_dir(repo)
        .output();

    let all_commits = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter_map(|l| {
                let mut parts = l.split('\t');
                let id = parts.next()?.trim().to_string();
                if id.is_empty() {
                    return None;
                }
                let description = parts.next().unwrap_or("").trim().to_string();
                let full_ts = parts.next().unwrap_or("").trim();
                let ts_parts: Vec<&str> = full_ts.split(' ').collect();
                let timestamp = if ts_parts.len() >= 2 {
                    format!("{} {}", ts_parts[0], ts_parts[1])
                } else {
                    ts_parts.first().copied().unwrap_or("").to_string()
                };
                Some(Change {
                    id,
                    description,
                    timestamp,
                })
            })
            .collect::<Vec<_>>(),
        Err(_) => vec![],
    };

    if all_commits.is_empty() {
        if crate::debug_enabled() {
            println!(
                "[BACKEND][vcs/git] get_git_changes empty history took={}ms",
                started.elapsed().as_millis()
            );
        }
        return vec![];
    }

    let total_commits = all_commits.len();
    let page = match before_id {
        None => {
            let mut page = vec![Change {
                id: "@".to_string(),
                description: "(working copy)".to_string(),
                timestamp: "now".to_string(),
            }];
            page.extend(all_commits.into_iter().take(limit.saturating_sub(1)));
            page.truncate(limit);
            page
        }
        Some(id) if id == "@" => all_commits.into_iter().take(limit).collect(),
        Some(id) => {
            let cursor = all_commits
                .iter()
                .position(|c| c.id == id || c.id.starts_with(&id));
            match cursor {
                Some(index) => all_commits
                    .into_iter()
                    .skip(index + 1)
                    .take(limit)
                    .collect(),
                None => vec![],
            }
        }
    };

    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/git] get_git_changes done commits_total={} returned={} took={}ms",
            total_commits,
            page.len(),
            started.elapsed().as_millis()
        );
    }
    page
}

fn get_git_branches(repo: &Path) -> Vec<Bookmark> {
    let started = Instant::now();
    if crate::debug_enabled() {
        println!("[BACKEND][vcs/git] running git branch for bookmarks");
    }
    let output = std::process::Command::new("git")
        .args([
            "--no-pager",
            "branch",
            "--format=%(refname:short)\t%(objectname:short)",
        ])
        .current_dir(repo)
        .output();

    let bookmarks = if let Ok(o) = output {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter_map(|l| {
                let mut parts = l.split('\t');
                let name = parts.next()?.trim().to_string();
                let id = parts.next()?.trim().to_string();
                if name.is_empty() {
                    return None;
                }
                Some(Bookmark { name, id })
            })
            .collect()
    } else {
        vec![]
    };

    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/git] get_git_branches count={} took={}ms",
            bookmarks.len(),
            started.elapsed().as_millis()
        );
    }
    bookmarks
}

fn get_git_changed_files(repo: &Path, since: &str) -> HashMap<String, String> {
    let started = Instant::now();
    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/git] get_git_changed_files start since='{}'",
            since
        );
    }
    let mut combined_map = HashMap::new();

    // 1. Get tracked changes (staged and committed)
    for part in since.split(" | ") {
        let rev = if part == "@" || part == "HEAD" {
            "HEAD".to_string()
        } else if part.contains("..") {
            part.to_string()
        } else {
            format!("{}~1..{}", part, part)
        };

        if crate::debug_enabled() {
            println!("[BACKEND][vcs/git] running git diff --name-status {}", rev);
        }
        if let Ok(o) = std::process::Command::new("git")
            .args(["--no-pager", "diff", "--name-status", &rev])
            .current_dir(repo)
            .output()
        {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    let mut parts = line.split_whitespace();
                    if let (Some(status), Some(file)) = (parts.next(), parts.next()) {
                        let status_str = match status.chars().next() {
                            Some('A') => "added",
                            Some('M') => "modified",
                            Some('D') => "deleted",
                            _ => "modified",
                        };
                        combined_map.insert(file.to_string(), status_str.to_string());
                    }
                }
            }
        }
    }

    // 2. If @ or HEAD is involved, also include untracked files
    if since.contains("@") || since.contains("HEAD") {
        if crate::debug_enabled() {
            println!("[BACKEND][vcs/git] running git ls-files for untracked");
        }
        if let Ok(o) = std::process::Command::new("git")
            .args(["--no-pager", "ls-files", "--others", "--exclude-standard"])
            .current_dir(repo)
            .output()
        {
            if o.status.success() {
                for file in String::from_utf8_lossy(&o.stdout).lines() {
                    if !file.trim().is_empty() {
                        combined_map.insert(file.trim().to_string(), "added".to_string());
                    }
                }
            }
        }
    }

    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/git] get_git_changed_files done count={} took={}ms",
            combined_map.len(),
            started.elapsed().as_millis()
        );
    }
    combined_map
}

fn get_git_file_diff(repo: &Path, file: &str, since: Option<&str>) -> String {
    let started = Instant::now();
    let mut args = vec!["--no-pager".to_string(), "diff".to_string()];
    if let Some(s) = since {
        let rev = if s == "@" || s == "HEAD" {
            "HEAD".to_string()
        } else if s.contains("..") {
            s.to_string()
        } else {
            format!("{}~1..{}", s, s)
        };
        args.push(rev);
    }
    args.push("--".to_string());
    args.push(file.to_string());

    if crate::debug_enabled() {
        println!("[BACKEND][vcs/git] running git diff for file={}", file);
    }
    let output = std::process::Command::new("git")
        .args(&args)
        .current_dir(repo)
        .output();

    let diff = if let Ok(o) = output {
        String::from_utf8_lossy(&o.stdout).to_string()
    } else {
        String::new()
    };

    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/git] get_git_file_diff bytes={} took={}ms",
            diff.len(),
            started.elapsed().as_millis()
        );
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_detect_jj() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".jj")).unwrap();
        assert!(matches!(detect_engine(dir.path()), VCSEngine::JJ));
    }

    #[test]
    fn test_detect_git() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        assert!(matches!(detect_engine(dir.path()), VCSEngine::Git));
    }

    #[test]
    fn test_detect_none() {
        let dir = tempdir().unwrap();
        assert!(matches!(detect_engine(dir.path()), VCSEngine::None));
    }
}

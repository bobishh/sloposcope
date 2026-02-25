use std::collections::HashMap;
use std::path::Path;
use crate::jj;
use serde::Serialize;

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
    if repo.join(".jj").is_dir() {
        VCSEngine::JJ
    } else if repo.join(".git").is_dir() {
        VCSEngine::Git
    } else {
        VCSEngine::None
    }
}

pub fn get_changes(repo: &Path, limit: usize) -> Vec<Change> {
    match detect_engine(repo) {
        VCSEngine::JJ => jj::changes(repo, limit),
        VCSEngine::Git => get_git_changes(repo, limit),
        VCSEngine::None => vec![],
    }
}

pub fn get_bookmarks(repo: &Path) -> Vec<Bookmark> {
    match detect_engine(repo) {
        VCSEngine::JJ => jj::bookmarks(repo),
        VCSEngine::Git => get_git_branches(repo),
        VCSEngine::None => vec![],
    }
}

pub fn get_current_branch(repo: &Path) -> String {
    match detect_engine(repo) {
        VCSEngine::JJ => {
            let output = std::process::Command::new("jj")
                .args(["--no-pager", "log", "--no-graph", "-r", "@", "-T", "bookmarks.join(', ')"])
                .current_dir(repo)
                .output();
            if let Ok(o) = output {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() { "@".to_string() } else { s }
            } else { "@".into() }
        },
        VCSEngine::Git => {
            let output = std::process::Command::new("git")
                .args(["--no-pager", "rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(repo)
                .output();
            if let Ok(o) = output {
                String::from_utf8_lossy(&o.stdout).trim().to_string()
            } else { "HEAD".into() }
        },
        VCSEngine::None => "".into(),
    }
}

pub fn get_changed_files(repo: &Path, since: &str) -> HashMap<String, String> {
    match detect_engine(repo) {
        VCSEngine::JJ => jj::changed_files(repo, since),
        VCSEngine::Git => get_git_changed_files(repo, since),
        VCSEngine::None => HashMap::new(),
    }
}

/// Get a list of ALL tracked files in the repo using the VCS index.
pub fn get_all_files(repo: &Path) -> Vec<String> {
    match detect_engine(repo) {
        VCSEngine::JJ => {
            let output = std::process::Command::new("jj")
                .args(["--no-pager", "file", "list"])
                .current_dir(repo)
                .output();
            if let Ok(o) = output {
                String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).collect()
            } else { vec![] }
        },
        VCSEngine::Git => {
            let output = std::process::Command::new("git")
                .args(["--no-pager", "ls-files"])
                .current_dir(repo)
                .output();
            if let Ok(o) = output {
                String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).collect()
            } else { vec![] }
        },
        VCSEngine::None => vec![],
    }
}

pub fn get_file_diff(repo: &Path, file: &str, since: Option<&str>) -> String {
    match detect_engine(repo) {
        VCSEngine::JJ => jj::file_diff(repo, file, since),
        VCSEngine::Git => get_git_file_diff(repo, file, since),
        VCSEngine::None => String::new(),
    }
}

// --- Git Implementation ---

fn get_git_changes(repo: &Path, limit: usize) -> Vec<Change> {
    let output = std::process::Command::new("git")
        .args(["--no-pager", "log", "-n", &limit.to_string(), "--pretty=format:%h\t%s\t%ai"])
        .current_dir(repo)
        .output();

    if let Ok(o) = output {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter_map(|l| {
                let mut parts = l.split('\t');
                let id = parts.next()?.to_string();
                let description = parts.next()?.to_string();
                let full_ts = parts.next()?;
                let ts_parts: Vec<&str> = full_ts.split(' ').collect();
                let timestamp = if ts_parts.len() >= 2 {
                    format!("{} {}", ts_parts[0], ts_parts[1])
                } else {
                    ts_parts[0].to_string()
                };
                Some(Change { id, description, timestamp })
            })
            .collect()
    } else {
        vec![]
    }
}

fn get_git_branches(repo: &Path) -> Vec<Bookmark> {
    let output = std::process::Command::new("git")
        .args(["--no-pager", "branch", "--format=%(refname:short)\t%(objectname:short)"])
        .current_dir(repo)
        .output();

    if let Ok(o) = output {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter_map(|l| {
                let mut parts = l.split('\t');
                let name = parts.next()?.trim().to_string();
                let id = parts.next()?.trim().to_string();
                if name.is_empty() { return None; }
                Some(Bookmark { name, id })
            })
            .collect()
    } else {
        vec![]
    }
}

fn get_git_changed_files(repo: &Path, since: &str) -> HashMap<String, String> {
    let mut combined_map = HashMap::new();
    
    // Split on ' | ' to support aggregate revsets
    for part in since.split(" | ") {
        // If it looks like a single commit (not a range), show its changes
        let rev = if part == "@" || part == "HEAD" {
            "HEAD".to_string()
        } else if part.contains("..") {
            part.to_string()
        } else {
            format!("{}~1..{}", part, part)
        };
        
        let output = std::process::Command::new("git")
            .args(["--no-pager", "diff", "--name-status", &rev])
            .current_dir(repo)
            .output();

        if let Ok(o) = output {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let mut parts = line.split_whitespace();
                let status = parts.next().unwrap_or("");
                let file = parts.next().unwrap_or("").to_string();
                if file.is_empty() { continue; }
                
                let status_str = match status.chars().next() {
                    Some('A') => "added",
                    Some('M') => "modified",
                    Some('D') => "deleted",
                    _ => "modified",
                };
                combined_map.insert(file, status_str.to_string());
            }
        }
    }
    combined_map
}

fn get_git_file_diff(repo: &Path, file: &str, since: Option<&str>) -> String {
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

    let output = std::process::Command::new("git")
        .args(&args)
        .current_dir(repo)
        .output();

    if let Ok(o) = output {
        String::from_utf8_lossy(&o.stdout).to_string()
    } else {
        String::new()
    }
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

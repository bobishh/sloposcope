use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use crate::vcs::{Bookmark, Change};

/// List recent changes from jj log.
pub fn changes(repo: &Path, limit: usize) -> Vec<Change> {
    let output = Command::new("jj")
        .args([
            "--no-pager",
            "log",
            "--no-graph",
            "-r",
            &format!("(ancestors(@, {}) ~ root())", limit),
            "-T",
            "commit_id.short(8) ++ \"\\t\" ++ description.first_line() ++ \"\\t\" ++ committer.timestamp().format(\"%Y-%m-%d %H:%M\") ++ \"\\n\"",
        ])
        .current_dir(repo)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return vec![],
    };

    output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut parts = line.split('\t');
            let id = parts.next().unwrap_or("").trim().to_string();
            let description = parts
                .next()
                .unwrap_or("(working copy)")
                .trim()
                .to_string();
            let timestamp = parts.next().unwrap_or("").trim().to_string();
            Change { id, description, timestamp }
        })
        .collect()
}

/// List all bookmarks (branches) in the repo.
pub fn bookmarks(repo: &Path) -> Vec<Bookmark> {
    let output = Command::new("jj")
        .args(["--no-pager", "bookmark", "list"])
        .current_dir(repo)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return vec![],
    };

    output
        .lines()
        .filter_map(|line| {
            // Default format: "name: id" or "name (type): id"
            let mut parts = line.split(':');
            let name_part = parts.next()?.trim();
            let id = parts.next()?.trim().to_string();
            
            // Clean name (remove "(local)" etc)
            let name = name_part.split(' ').next()?.to_string();
            
            if name.is_empty() { return None; }
            Some(Bookmark { name, id: id.chars().take(8).collect() })
        })
        .collect()
}

/// Get files changed since a revset, with their status.
pub fn changed_files(repo: &Path, revset: &str) -> HashMap<String, String> {
    let mut args = vec!["--no-pager".to_string(), "diff".to_string(), "--summary".to_string()];
    
    if revset == "00000000" || revset == "zzzzzzzz" || revset == "root()" {
        return HashMap::new();
    }

    // Use -r to show changes introduced BY the revset
    // If it contains complex logic or is just '@', -r is usually what's wanted for a "view commit" action
    args.extend(["-r".to_string(), revset.to_string()]);

    let output = Command::new("jj")
        .args(&args)
        .current_dir(repo)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return HashMap::new(),
    };

    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let status = parts.next()?;
            let path = parts.next()?.trim().to_string();
            let status = match status {
                "A" => "added",
                "M" => "modified",
                "D" => "deleted",
                _ => return None,
            };
            Some((path, status.to_string()))
        })
        .collect()
}

/// Get the diff for a single file, optionally since a revset.
pub fn file_diff(repo: &Path, file: &str, since: Option<&str>) -> String {
    let mut args = vec!["--no-pager".to_string(), "diff".to_string(), "--git".to_string()];
    if let Some(s) = since {
        if s == "00000000" || s == "zzzzzzzz" || s == "root()" {
            // Root has no diff
            return String::new();
        }
        args.extend(["-r".to_string(), s.to_string()]);
    }
    args.extend(["--".to_string(), file.to_string()]);

    let output = Command::new("jj").args(&args).current_dir(repo).output();

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => String::new(),
    }
}

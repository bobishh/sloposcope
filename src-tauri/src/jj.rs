use crate::vcs::{Bookmark, Change};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

fn parse_changes(output: &str) -> Vec<Change> {
    // JJ templates can emit literal "\n" markers instead of real line breaks.
    output
        .split("\\n")
        .flat_map(|chunk| chunk.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let id = parts.next()?.trim().to_string();
            if id.is_empty() {
                return None;
            }
            let description = parts.next().unwrap_or("(working copy)").trim().to_string();
            let timestamp = parts.next().unwrap_or("").trim().to_string();
            Some(Change {
                id,
                description,
                timestamp,
            })
        })
        .collect()
}

fn fetch_changes(repo: &Path, revset: &str) -> Vec<Change> {
    let started = Instant::now();
    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/jj] running jj log for history revset='{}'",
            revset
        );
    }
    let output = Command::new("jj")
        .args([
            "--no-pager",
            "log",
            "--no-graph",
            "-r",
            revset,
            "-T",
            "commit_id.short(8) ++ \"\\t\" ++ description.first_line() ++ \"\\t\" ++ committer.timestamp().format(\"%Y-%m-%d %H:%M\") ++ \"\\n\"",
        ])
        .current_dir(repo)
        .output();

    let changes = match output {
        Ok(o) if o.status.success() => parse_changes(&String::from_utf8_lossy(&o.stdout)),
        _ => vec![],
    };
    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/jj] fetch_changes revset='{}' count={} took={}ms",
            revset,
            changes.len(),
            started.elapsed().as_millis()
        );
    }
    changes
}

/// List recent changes from jj log.
pub fn changes(repo: &Path, limit: usize, before_id: Option<String>) -> Vec<Change> {
    let started = Instant::now();
    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/jj] changes start limit={} before_id={:?}",
            limit, before_id
        );
    }
    // Basic check to see if we can even run jj
    if crate::debug_enabled() {
        println!("[BACKEND][vcs/jj] running jj availability check");
    }
    let check = Command::new("jj")
        .args(["--no-pager", "log", "-r", "root()", "-T", "commit_id"])
        .current_dir(repo)
        .output();
    if check.is_err() || !check.unwrap().status.success() {
        println!(
            "[BACKEND][vcs/jj] jj check failed, returning empty history ({}ms)",
            started.elapsed().as_millis()
        );
        return vec![];
    }

    // Fetch full linear ancestry once and paginate in Rust. This avoids overlap
    // or cursor issues from revset arithmetic across different JJ versions.
    let all = fetch_changes(repo, "(ancestors(@) ~ root())");
    if all.is_empty() {
        println!(
            "[BACKEND][vcs/jj] history fetch failed or returned nothing ({}ms)",
            started.elapsed().as_millis()
        );
        return vec![];
    }

    let total_changes = all.len();
    let page = match before_id.as_ref() {
        None => all.into_iter().take(limit).collect::<Vec<_>>(),
        Some(id) if id.is_empty() => all.into_iter().take(limit).collect::<Vec<_>>(),
        Some(id) => {
            let cursor = all.iter().position(|c| c.id == *id || c.id.starts_with(id));
            match cursor {
                Some(index) => all
                    .into_iter()
                    .skip(index + 1)
                    .take(limit)
                    .collect::<Vec<_>>(),
                None => vec![],
            }
        }
    };

    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/jj] changes done total={} returned={} took={}ms",
            total_changes,
            page.len(),
            started.elapsed().as_millis()
        );
    }
    page
}

/// List all bookmarks (branches) in the repo.
pub fn bookmarks(repo: &Path) -> Vec<Bookmark> {
    let started = Instant::now();
    if crate::debug_enabled() {
        println!("[BACKEND][vcs/jj] running jj bookmark list");
    }
    let output = Command::new("jj")
        .args(["--no-pager", "bookmark", "list"])
        .current_dir(repo)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return vec![],
    };

    let bookmarks = output
        .lines()
        .filter_map(|line| {
            // Default format: "name: id" or "name (type): id"
            let mut parts = line.split(':');
            let name_part = parts.next()?.trim();
            let id = parts.next()?.trim().to_string();

            // Clean name (remove "(local)" etc)
            let name = name_part.split(' ').next()?.to_string();

            if name.is_empty() {
                return None;
            }
            Some(Bookmark {
                name,
                id: id.chars().take(8).collect(),
            })
        })
        .collect::<Vec<_>>();

    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/jj] bookmarks count={} took={}ms",
            bookmarks.len(),
            started.elapsed().as_millis()
        );
    }
    bookmarks
}

/// Get files changed since a revset, with their status.
pub fn changed_files(repo: &Path, revset: &str) -> HashMap<String, String> {
    let started = Instant::now();
    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/jj] changed_files start revset='{}'",
            revset
        );
    }
    let mut results = HashMap::new();

    if revset == "00000000" || revset == "zzzzzzzz" || revset == "root()" {
        if crate::debug_enabled() {
            println!(
                "[BACKEND][vcs/jj] changed_files revset short-circuit count=0 took={}ms",
                started.elapsed().as_millis()
            );
        }
        return results;
    }

    // 1. Get tracked changes via diff --summary
    let mut diff_args = vec![
        "--no-pager".to_string(),
        "diff".to_string(),
        "--summary".to_string(),
    ];
    diff_args.extend(["-r".to_string(), revset.to_string()]);

    if crate::debug_enabled() {
        println!("[BACKEND][vcs/jj] running jj diff --summary");
    }
    if let Ok(o) = Command::new("jj")
        .args(&diff_args)
        .current_dir(repo)
        .output()
    {
        if o.status.success() {
            let output = String::from_utf8_lossy(&o.stdout);
            for line in output.lines() {
                let mut parts = line.splitn(2, ' ');
                if let (Some(status_code), Some(path)) = (parts.next(), parts.next()) {
                    let status = match status_code {
                        "A" => "added",
                        "M" => "modified",
                        "D" => "deleted",
                        _ => continue,
                    };
                    results.insert(path.trim().to_string(), status.to_string());
                }
            }
        }
    }

    // 2. If we are looking at the working copy (@), also include untracked files
    if revset == "@" {
        if crate::debug_enabled() {
            println!("[BACKEND][vcs/jj] running jj status for untracked");
        }
        if let Ok(o) = Command::new("jj")
            .args(["--no-pager", "status"])
            .current_dir(repo)
            .output()
        {
            if o.status.success() {
                let output = String::from_utf8_lossy(&o.stdout);
                // jj status output format for untracked:
                // "U path/to/file"
                for line in output.lines() {
                    let mut parts = line.split_whitespace();
                    if let (Some(status_code), Some(path)) = (parts.next(), parts.next()) {
                        if status_code == "U" {
                            results.insert(path.trim().to_string(), "added".to_string());
                        }
                    }
                }
            }
        }
    }

    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/jj] changed_files done count={} took={}ms",
            results.len(),
            started.elapsed().as_millis()
        );
    }
    results
}

/// Get the diff for a single file, optionally since a revset.
pub fn file_diff(repo: &Path, file: &str, since: Option<&str>) -> String {
    let started = Instant::now();
    let mut args = vec![
        "--no-pager".to_string(),
        "diff".to_string(),
        "--git".to_string(),
    ];
    if let Some(s) = since {
        if s == "00000000" || s == "zzzzzzzz" || s == "root()" {
            // Root has no diff
            return String::new();
        }
        args.extend(["-r".to_string(), s.to_string()]);
    }
    args.extend(["--".to_string(), file.to_string()]);

    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/jj] running jj diff for file={} since={:?}",
            file, since
        );
    }
    let output = Command::new("jj").args(&args).current_dir(repo).output();

    let diff = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    };
    if crate::debug_enabled() {
        println!(
            "[BACKEND][vcs/jj] file_diff file={} bytes={} took={}ms",
            file,
            diff.len(),
            started.elapsed().as_millis()
        );
    }
    diff
}

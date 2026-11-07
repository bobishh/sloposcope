//! One bounded Git history index per resolved repository endpoint.
//!
//! The index is built with one batched `git log` traversal (including rename
//! records), then reused across requested files at that immutable revision.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CACHED_REVISIONS: usize = 4;
const MAX_INDEXED_PATHS: usize = 150_000;
const MAX_HISTORY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_HISTORY_COMMITS: usize = 50_000;

#[derive(Debug, Clone)]
pub struct FirstAppearance {
    pub at: String,
    pub epoch: i64,
}

#[derive(Debug)]
pub struct GitHistoryIndex {
    pub revision_at: String,
    pub revision_epoch: i64,
    pub first_seen: HashMap<String, FirstAppearance>,
    tracked_paths: HashSet<String>,
    shallow_first_seen: HashSet<String>,
    is_shallow: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    repo: PathBuf,
    revision: String,
}

struct CacheEntry {
    key: CacheKey,
    index: Arc<GitHistoryIndex>,
}

fn cache() -> &'static Mutex<VecDeque<CacheEntry>> {
    static CACHE: OnceLock<Mutex<VecDeque<CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub fn for_revision(repo: &Path, revision: &str) -> Result<Arc<GitHistoryIndex>, String> {
    let canonical = repo
        .canonicalize()
        .map_err(|error| format!("Could not resolve repository for Git history: {error}"))?;
    let key = CacheKey {
        repo: canonical.clone(),
        revision: revision.to_string(),
    };
    {
        let mut entries = cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(position) = entries.iter().position(|entry| entry.key == key) {
            let entry = entries
                .remove(position)
                .expect("cache entry position exists");
            let index = entry.index.clone();
            entries.push_front(entry);
            return Ok(index);
        }
    }

    let index = Arc::new(build_index(&canonical, revision)?);
    let mut entries = cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = entries.iter().find(|entry| entry.key == key) {
        return Ok(existing.index.clone());
    }
    entries.push_front(CacheEntry {
        key,
        index: index.clone(),
    });
    while entries.len() > MAX_CACHED_REVISIONS {
        entries.pop_back();
    }
    Ok(index)
}

fn build_index(repo: &Path, revision: &str) -> Result<GitHistoryIndex, String> {
    let count = Command::new("git")
        .args(["--no-pager", "rev-list", "--count", revision])
        .current_dir(repo)
        .output()
        .map_err(|error| format!("Could not count Git history: {error}"))?;
    if !count.status.success() {
        return Err("Could not count Git history".into());
    }
    let commit_count = String::from_utf8_lossy(&count.stdout)
        .trim()
        .parse::<usize>()
        .map_err(|_| "Git returned an invalid history count".to_string())?;
    if commit_count > MAX_HISTORY_COMMITS {
        return Err(format!(
            "Git history has {commit_count} commits; age index limit is {MAX_HISTORY_COMMITS}"
        ));
    }

    let history_bytes = bounded_git_output(
        repo,
        &[
            "--no-pager",
            "log",
            "--reverse",
            "--format=@%H%x09%ct%x09%cI",
            "-z",
            "--name-status",
            "--find-renames",
            "--diff-filter=AR",
            "--max-count=50000",
            revision,
        ],
        "Could not build Git file history index",
    )?;
    let (revision_epoch, revision_at) = commit_time(repo, revision)?;
    let shallow_boundaries = shallow_boundaries(repo)?;
    let is_shallow = !shallow_boundaries.is_empty();
    let mut first_seen = HashMap::<String, FirstAppearance>::new();
    let mut shallow_first_seen = HashSet::new();
    let tokens = history_bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut current_commit = String::new();
    let mut current_epoch = None;
    let mut current_at = None;
    let mut cursor = 0;
    while cursor < tokens.len() {
        let token = tokens[cursor];
        if token.starts_with(b"@") {
            let header = std::str::from_utf8(token)
                .map_err(|_| "Git returned non-UTF-8 history metadata".to_string())?;
            let (commit, metadata) = header[1..]
                .split_once('\t')
                .ok_or_else(|| "Git returned malformed history metadata".to_string())?;
            let (epoch, at) = metadata
                .split_once('\t')
                .ok_or_else(|| "Git returned malformed history metadata".to_string())?;
            current_commit = commit.to_string();
            current_epoch = Some(
                epoch
                    .parse::<i64>()
                    .map_err(|_| "Git returned malformed commit timestamp".to_string())?,
            );
            current_at = Some(at.to_string());
            cursor += 1;
            continue;
        }
        let status = std::str::from_utf8(token)
            .unwrap_or("")
            .trim_start_matches(['\n', '\r']);
        if status.is_empty() || cursor + 1 >= tokens.len() {
            cursor += 1;
            continue;
        }
        let Some(epoch) = current_epoch else {
            cursor += 1;
            continue;
        };
        let at = current_at.clone().unwrap_or_default();
        if status.starts_with('R') {
            let Some(old_path) = utf8_path(tokens[cursor + 1]) else {
                cursor += 3;
                continue;
            };
            let Some(new_path) = tokens.get(cursor + 2).and_then(|path| utf8_path(path)) else {
                cursor += 3;
                continue;
            };
            if let Some(origin) = first_seen.get(&old_path).cloned() {
                first_seen.entry(new_path.clone()).or_insert(origin.clone());
                if shallow_first_seen.contains(&old_path) {
                    shallow_first_seen.insert(new_path);
                }
            }
            cursor += 3;
            continue;
        }
        let Some(path) = utf8_path(tokens[cursor + 1]) else {
            cursor += 2;
            continue;
        };
        if status == "A" {
            first_seen
                .entry(path.clone())
                .or_insert_with(|| FirstAppearance { at, epoch });
            if shallow_boundaries.contains(&current_commit) {
                shallow_first_seen.insert(path);
            }
        }
        cursor += 2;
        if first_seen.len() > MAX_INDEXED_PATHS {
            return Err(format!(
                "Git history index exceeds the {MAX_INDEXED_PATHS} path limit"
            ));
        }
    }
    let tracked_paths = bounded_git_output(
        repo,
        &[
            "--no-pager",
            "ls-tree",
            "-r",
            "--name-only",
            "-z",
            "--full-tree",
            revision,
        ],
        "Could not list Git endpoint paths",
    )?
    .split(|byte| *byte == 0)
    .filter_map(utf8_path)
    .collect();
    Ok(GitHistoryIndex {
        revision_at,
        revision_epoch,
        first_seen,
        tracked_paths,
        shallow_first_seen,
        is_shallow,
    })
}

fn bounded_git_output(repo: &Path, args: &[&str], error: &str) -> Result<Vec<u8>, String> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|failure| format!("{error}: {failure}"))?;
    let stdout = child.stdout.take().ok_or_else(|| error.to_string())?;
    let mut bytes = Vec::new();
    stdout
        .take(MAX_HISTORY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|failure| format!("{error}: {failure}"))?;
    if bytes.len() as u64 > MAX_HISTORY_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!(
            "{error}: index exceeds the {MAX_HISTORY_BYTES} byte limit"
        ));
    }
    let status = child
        .wait()
        .map_err(|failure| format!("{error}: {failure}"))?;
    if !status.success() {
        return Err(error.to_string());
    }
    Ok(bytes)
}

fn commit_time(repo: &Path, revision: &str) -> Result<(i64, String), String> {
    let output = Command::new("git")
        .args(["--no-pager", "show", "-s", "--format=%ct%x09%cI", revision])
        .current_dir(repo)
        .output()
        .map_err(|error| format!("Could not read Git endpoint timestamp: {error}"))?;
    if !output.status.success() {
        return Err("Could not read Git endpoint timestamp".into());
    }
    let value = String::from_utf8_lossy(&output.stdout);
    let (epoch, at) = value
        .trim()
        .split_once('\t')
        .ok_or_else(|| "Git returned malformed endpoint timestamp".to_string())?;
    let epoch = epoch
        .parse::<i64>()
        .map_err(|_| "Git returned malformed endpoint timestamp".to_string())?;
    Ok((epoch, at.to_string()))
}

fn shallow_boundaries(repo: &Path) -> Result<HashSet<String>, String> {
    let output = Command::new("git")
        .args(["--no-pager", "rev-parse", "--git-path", "shallow"])
        .current_dir(repo)
        .output()
        .map_err(|error| format!("Could not inspect Git shallow history: {error}"))?;
    if !output.status.success() {
        return Err("Could not inspect Git shallow history".into());
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let path = if path.is_absolute() {
        path
    } else {
        repo.join(path)
    };
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(contents.lines().map(str::to_string).collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(error) => Err(format!("Could not read Git shallow history: {error}")),
    }
}

fn utf8_path(bytes: &[u8]) -> Option<String> {
    let value = std::str::from_utf8(bytes).ok()?;
    (!value.is_empty()).then(|| value.to_string())
}

impl GitHistoryIndex {
    pub fn contains_path(&self, path: &str) -> bool {
        self.tracked_paths.contains(path)
    }
    pub fn first_appearance(&self, path: &str) -> Result<&FirstAppearance, String> {
        if self.shallow_first_seen.contains(path) {
            return Err("Git history is shallow at the file's first recorded appearance".into());
        }
        self.first_seen.get(path).ok_or_else(|| {
            if self.is_shallow {
                "File is untracked or predates the shallow Git history boundary".into()
            } else {
                "File is untracked or has no recorded Git first appearance".into()
            }
        })
    }
}

pub fn current_utc_time() -> Result<(i64, String), String> {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("Could not read system clock: {error}"))?
        .as_secs() as i64;
    let days = epoch.div_euclid(86_400);
    let day_seconds = epoch.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let hour = day_seconds / 3_600;
    let minute = day_seconds % 3_600 / 60;
    let second = day_seconds % 60;
    Ok((
        epoch,
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"),
    ))
}

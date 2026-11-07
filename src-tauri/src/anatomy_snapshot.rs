use crate::parser::{physical_line_count, FileAnatomy, Parser};
#[path = "anatomy_history.rs"]
mod anatomy_history;
use crate::vcs::{self, VCSEngine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::{Command, Output};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnatomySelection {
    #[serde(default = "default_selection_mode")]
    pub mode: String,
    #[serde(default)]
    pub since: Option<String>,
}

fn default_selection_mode() -> String {
    "none".to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnatomyEndpoint {
    pub label: Option<String>,
    pub source_id: Option<String>,
    pub status: String,
    pub anatomy: Option<FileAnatomy>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnatomyComparison {
    pub file: String,
    pub comparison_id: String,
    pub has_baseline: bool,
    pub before: AnatomyEndpoint,
    pub after: AnatomyEndpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SnapshotRef {
    WorkingCopy,
    Git(String),
    Jujutsu(String),
}

#[derive(Debug, Clone)]
struct ResolvedSelection {
    id: String,
    before: Option<SnapshotRef>,
    after: SnapshotRef,
    has_baseline: bool,
}

fn valid_file_path(file: &str) -> bool {
    let path = Path::new(file);
    !file.trim().is_empty()
        && !path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
}

fn command_output(repo: &Path, program: &str, args: &[&str]) -> Result<Output, String> {
    Command::new(program)
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|error| format!("Failed to run {program}: {error}"))
}

fn git_revision(repo: &Path, rev: &str) -> Result<String, String> {
    let commit = format!("{rev}^{{commit}}");
    let output = command_output(
        repo,
        "git",
        &[
            "--no-pager",
            "rev-parse",
            "--verify",
            "--end-of-options",
            &commit,
        ],
    )?;
    if !output.status.success() {
        return Err(format!("Git revision could not be resolved: {rev}"));
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        Err(format!("Git revision could not be resolved: {rev}"))
    } else {
        Ok(value)
    }
}

fn jj_revision(repo: &Path, rev: &str) -> Result<String, String> {
    let output = command_output(
        repo,
        "jj",
        &[
            "--no-pager",
            "log",
            "--no-graph",
            "-r",
            rev,
            "-T",
            "commit_id ++ \"\\n\"",
        ],
    )?;
    if !output.status.success() {
        return Err(format!("Jujutsu revision could not be resolved: {rev}"));
    }
    let revisions = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if revisions.len() != 1 {
        Err(format!("Jujutsu revision could not be resolved: {rev}"))
    } else {
        Ok(revisions[0].to_string())
    }
}

fn split_range(since: &str) -> Option<(&str, &str, bool)> {
    let (before, after, merge_base) = if let Some((before, after)) = since.split_once("...") {
        (before, after, true)
    } else {
        let (before, after) = since.split_once("..")?;
        (before, after, false)
    };
    if before.is_empty() || after.is_empty() || after.contains("..") {
        return None;
    }
    Some((before, after, merge_base))
}

fn git_parent(repo: &Path, revision: &str) -> Result<Option<String>, String> {
    let output = command_output(
        repo,
        "git",
        &["--no-pager", "rev-list", "--parents", "-n", "1", revision],
    )?;
    if !output.status.success() {
        return Err(format!(
            "Could not resolve parent of Git revision {revision}"
        ));
    }
    let fields = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return Err(format!("Git returned no commit identity for {revision}"));
    }
    Ok(fields.get(1).cloned())
}

fn git_head(repo: &Path) -> Result<Option<String>, String> {
    match git_revision(repo, "HEAD") {
        Ok(revision) => Ok(Some(revision)),
        Err(error) => {
            let output = command_output(
                repo,
                "git",
                &["--no-pager", "rev-list", "--all", "--max-count=1"],
            )?;
            if !output.status.success() {
                return Err(format!("Could not resolve Git HEAD: {error}"));
            }
            if String::from_utf8_lossy(&output.stdout).trim().is_empty() {
                Ok(None)
            } else {
                Err(format!("Git has commits but HEAD is unresolved: {error}"))
            }
        }
    }
}

fn git_merge_base(repo: &Path, before: &str, after: &str) -> Result<String, String> {
    let output = command_output(repo, "git", &["merge-base", before, after])?;
    if !output.status.success() {
        return Err(format!("Git range has no merge base: {before}...{after}"));
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        Err(format!("Git range has no merge base: {before}...{after}"))
    } else {
        Ok(value)
    }
}

fn jj_parent(repo: &Path, revision: &str) -> Result<Option<String>, String> {
    let parent_revset = format!("{revision}-");
    let output = command_output(
        repo,
        "jj",
        &[
            "--no-pager",
            "log",
            "--no-graph",
            "-r",
            &parent_revset,
            "-T",
            "commit_id ++ \"\\n\"",
        ],
    )?;
    if !output.status.success() {
        return Err(format!(
            "Could not resolve parent of Jujutsu revision {revision}"
        ));
    }
    let parents = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    match parents.as_slice() {
        [] => Ok(None),
        [parent] => Ok(Some(parent.clone())),
        _ => Err(format!(
            "Jujutsu revision {revision} has multiple parents; comparison endpoint is ambiguous"
        )),
    }
}

fn resolve_selection(repo: &Path, request: &AnatomySelection) -> Result<ResolvedSelection, String> {
    let since = request.since.as_deref().unwrap_or("@").trim();
    if request.mode == "unsupported" {
        return Err("The selected graph request does not define one After endpoint".into());
    }
    match vcs::detect_engine(repo) {
        VCSEngine::Git => {
            if request.mode == "none" {
                return Ok(ResolvedSelection {
                    id: "working-copy-only".into(),
                    before: None,
                    after: SnapshotRef::WorkingCopy,
                    has_baseline: false,
                });
            }
            if since == "@" || since == "HEAD" || request.mode == "working_copy" {
                let after = SnapshotRef::WorkingCopy;
                let before = git_head(repo)?.map(SnapshotRef::Git);
                return Ok(ResolvedSelection {
                    id: "HEAD..working-copy".into(),
                    has_baseline: before.is_some(),
                    before,
                    after,
                });
            }
            if let Some((before_rev, after_rev, use_merge_base)) = split_range(since) {
                let before = git_revision(repo, before_rev)?;
                let after = git_revision(repo, after_rev)?;
                let before = if use_merge_base {
                    git_merge_base(repo, &before, &after)?
                } else {
                    before
                };
                return Ok(ResolvedSelection {
                    id: format!("{before}..{after}"),
                    before: Some(SnapshotRef::Git(before)),
                    after: SnapshotRef::Git(after),
                    has_baseline: true,
                });
            }
            let after = git_revision(repo, since)?;
            let before = git_parent(repo, &after)?;
            let has_baseline = before.is_some();
            Ok(ResolvedSelection {
                id: format!("{}..{after}", before.as_deref().unwrap_or("root")),
                before: before.map(SnapshotRef::Git),
                after: SnapshotRef::Git(after),
                has_baseline,
            })
        }
        VCSEngine::JJ => {
            if request.mode == "none" {
                return Ok(ResolvedSelection {
                    id: "working-copy-only".into(),
                    before: None,
                    after: SnapshotRef::WorkingCopy,
                    has_baseline: false,
                });
            }
            if since == "@" || request.mode == "working_copy" {
                let after = jj_revision(repo, "@")?;
                let before = jj_parent(repo, &after)?;
                let has_baseline = before.is_some();
                return Ok(ResolvedSelection {
                    id: format!("{}..{after}", before.as_deref().unwrap_or("root")),
                    before: before.map(SnapshotRef::Jujutsu),
                    after: SnapshotRef::Jujutsu(after),
                    has_baseline,
                });
            }
            if let Some((before_rev, after_rev, use_merge_base)) = split_range(since) {
                if use_merge_base {
                    return Err(
                        "Jujutsu anatomy ranges use `before..after`, not Git merge-base syntax"
                            .into(),
                    );
                }
                let before = jj_revision(repo, before_rev)?;
                let after = jj_revision(repo, after_rev)?;
                return Ok(ResolvedSelection {
                    id: format!("{before}..{after}"),
                    before: Some(SnapshotRef::Jujutsu(before)),
                    after: SnapshotRef::Jujutsu(after),
                    has_baseline: true,
                });
            }
            let after = jj_revision(repo, since)?;
            let before = jj_parent(repo, &after)?;
            let has_baseline = before.is_some();
            Ok(ResolvedSelection {
                id: format!("{}..{after}", before.as_deref().unwrap_or("root")),
                before: before.map(SnapshotRef::Jujutsu),
                after: SnapshotRef::Jujutsu(after),
                has_baseline,
            })
        }
        VCSEngine::None
            if request.mode == "none" || request.mode == "working_copy" || since == "@" =>
        {
            Ok(ResolvedSelection {
                id: "working-copy-only".into(),
                before: None,
                after: SnapshotRef::WorkingCopy,
                has_baseline: false,
            })
        }
        VCSEngine::None => Err("Historical snapshot reads require Git or Jujutsu".into()),
    }
}

fn sha256(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn read_working_copy_file(repo: &Path, file: &str) -> Result<Option<Vec<u8>>, String> {
    if !valid_file_path(file) {
        return Err("invalid repository path".into());
    }
    let root = repo
        .canonicalize()
        .map_err(|error| format!("Could not resolve repository root: {error}"))?;
    let requested = root.join(file);
    match std::fs::symlink_metadata(&requested) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Could not inspect working copy file {file}: {error}"
            ))
        }
    }
    let canonical = requested
        .canonicalize()
        .map_err(|error| format!("Could not resolve working copy file {file}: {error}"))?;
    if !canonical.starts_with(&root) {
        return Err("working copy path resolves outside repository".into());
    }
    let metadata = std::fs::metadata(&canonical)
        .map_err(|error| format!("Could not inspect working copy file {file}: {error}"))?;
    if !metadata.is_file() {
        return Err(format!("Working copy path is not a regular file: {file}"));
    }
    std::fs::read(canonical)
        .map(Some)
        .map_err(|error| format!("Could not read working copy file {file}: {error}"))
}

fn read_git(repo: &Path, revision: &str, file: &str) -> Result<Option<Vec<u8>>, String> {
    let listing = command_output(
        repo,
        "git",
        &[
            "--literal-pathspecs",
            "--no-pager",
            "ls-tree",
            "-r",
            "--name-only",
            "-z",
            revision,
            "--",
            file,
        ],
    )?;
    if !listing.status.success() {
        return Err(format!("Could not inspect Git tree {revision} for {file}"));
    }
    if !listing
        .stdout
        .split(|byte| *byte == 0)
        .any(|path| path == file.as_bytes())
    {
        return Ok(None);
    }
    let object = format!("{revision}:{file}");
    let output = command_output(repo, "git", &["--no-pager", "show", &object])?;
    if !output.status.success() {
        return Err(format!("Could not read {file} at Git revision {revision}"));
    }
    Ok(Some(output.stdout))
}

fn read_jj(repo: &Path, revision: &str, file: &str) -> Result<Option<Vec<u8>>, String> {
    let list = command_output(
        repo,
        "jj",
        &[
            "--no-pager",
            "file",
            "list",
            "-r",
            revision,
            "-T",
            "self.path().display() ++ \"\\0\"",
            "--",
            file,
        ],
    )?;
    if !list.status.success() {
        return Err(format!(
            "Could not list files at Jujutsu revision {revision}"
        ));
    }
    if !list
        .stdout
        .split(|byte| *byte == 0)
        .any(|path| path == file.as_bytes())
    {
        return Ok(None);
    }
    let output = command_output(
        repo,
        "jj",
        &["--no-pager", "file", "show", "-r", revision, "--", file],
    )?;
    if !output.status.success() {
        return Err(format!(
            "Could not read {file} at Jujutsu revision {revision}"
        ));
    }
    Ok(Some(output.stdout))
}

fn read_snapshot(
    repo: &Path,
    source: &SnapshotRef,
    file: &str,
) -> Result<Option<(Vec<u8>, String, String)>, String> {
    let (content, label) = match source {
        SnapshotRef::WorkingCopy => (
            read_working_copy_file(repo, file)?,
            "working copy".to_string(),
        ),
        SnapshotRef::Git(revision) => (read_git(repo, revision, file)?, format!("Git {revision}")),
        SnapshotRef::Jujutsu(revision) => {
            (read_jj(repo, revision, file)?, format!("JJ {revision}"))
        }
    };
    Ok(content.map(|content| {
        let source_id = sha256(&content);
        (content, source_id, label)
    }))
}

fn analyze_content(
    parsers: &[Box<dyn Parser>],
    repo: &Path,
    file: &str,
    content: &[u8],
) -> Result<FileAnatomy, String> {
    let text = std::str::from_utf8(content)
        .map_err(|error| format!("File is not valid UTF-8: {error}"))?;
    let extension = Path::new(file)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_ascii_lowercase();
    let Some(language_parser) = parsers
        .iter()
        .find(|candidate| candidate.extensions().contains(&extension.as_str()))
    else {
        return Ok(FileAnatomy::unavailable(
            file,
            &extension,
            "no Tree-sitter anatomy profile for this extension",
        ));
    };
    Ok(language_parser.analyze_file(repo, file, text))
}

fn endpoint(
    parsers: &[Box<dyn Parser>],
    repo: &Path,
    file: &str,
    source: Option<&SnapshotRef>,
) -> AnatomyEndpoint {
    let Some(source) = source else {
        return AnatomyEndpoint {
            label: None,
            source_id: None,
            status: "absent".into(),
            anatomy: None,
            reason: Some("No comparison selected".into()),
        };
    };
    match read_snapshot(repo, source, file) {
        Ok(None) => AnatomyEndpoint {
            label: Some(source.label()),
            source_id: None,
            status: "absent".into(),
            anatomy: None,
            reason: Some("File did not exist at this endpoint".into()),
        },
        Ok(Some((content, source_id, label))) => {
            let line_count = physical_line_count(&content);
            let mut anatomy =
                analyze_content(parsers, repo, file, &content).unwrap_or_else(|reason| {
                    let extension = Path::new(file)
                        .extension()
                        .and_then(|value| value.to_str())
                        .unwrap_or("unknown");
                    FileAnatomy::unavailable(file, extension, &reason)
                });
            anatomy.source_id = source_id.clone();
            anatomy.line_count = Some(line_count);
            enrich_age(repo, file, source, &mut anatomy);
            let unavailable = anatomy.status == crate::parser::AnatomyStatus::Unavailable;
            AnatomyEndpoint {
                label: Some(label),
                source_id: Some(source_id),
                status: if unavailable {
                    "unavailable"
                } else {
                    "available"
                }
                .into(),
                reason: if unavailable {
                    anatomy.reason.clone()
                } else {
                    None
                },
                anatomy: Some(anatomy),
            }
        }
        Err(reason) => AnatomyEndpoint {
            label: Some(source.label()),
            source_id: None,
            status: "unavailable".into(),
            anatomy: None,
            reason: Some(reason),
        },
    }
}

fn enrich_age(repo: &Path, file: &str, source: &SnapshotRef, anatomy: &mut FileAnatomy) {
    let (revision, working_copy_time) = match source {
        SnapshotRef::Git(revision) => (revision.clone(), None),
        SnapshotRef::WorkingCopy => {
            if !matches!(vcs::detect_engine(repo), VCSEngine::Git) {
                return set_age_unknown(
                    anatomy,
                    "Git first-appearance age is unavailable outside Git repositories",
                );
            }
            let head = match git_head(repo) {
                Ok(Some(head)) => head,
                Ok(None) => {
                    return set_age_unknown(anatomy, "Git repository has no committed history")
                }
                Err(reason) => return set_age_unknown(anatomy, reason),
            };
            let now = match anatomy_history::current_utc_time() {
                Ok(value) => value,
                Err(reason) => return set_age_unknown(anatomy, reason),
            };
            (head, Some(now))
        }
        SnapshotRef::Jujutsu(_) => {
            return set_age_unknown(
                anatomy,
                "Git first-appearance age is unsupported for Jujutsu endpoints",
            )
        }
    };
    let index = match anatomy_history::for_revision(repo, &revision) {
        Ok(index) => index,
        Err(reason) => return set_age_unknown(anatomy, reason),
    };
    if matches!(source, SnapshotRef::WorkingCopy) && !index.contains_path(file) {
        return set_age_unknown(anatomy, "File is untracked at the Git HEAD endpoint");
    }
    let (as_of_epoch, as_of) =
        working_copy_time.unwrap_or_else(|| (index.revision_epoch, index.revision_at.clone()));
    let first_seen = match index.first_appearance(file) {
        Ok(first_seen) => first_seen,
        Err(reason) => return set_age_unknown(anatomy, reason),
    };
    let Some(age_seconds) = as_of_epoch.checked_sub(first_seen.epoch) else {
        return set_age_unknown(anatomy, "Git timestamps are not ordered for this endpoint");
    };
    if age_seconds < 0 {
        return set_age_unknown(anatomy, "Git timestamps are not ordered for this endpoint");
    }
    let Ok(age_days) = u32::try_from(age_seconds / 86_400) else {
        return set_age_unknown(anatomy, "Git age exceeds the supported day range");
    };
    anatomy.first_seen_at = Some(first_seen.at.clone());
    anatomy.age_as_of = Some(as_of);
    anatomy.age_days = Some(age_days);
    anatomy.age_reason = None;
}

fn set_age_unknown(anatomy: &mut FileAnatomy, reason: impl Into<String>) {
    anatomy.first_seen_at = None;
    anatomy.age_as_of = None;
    anatomy.age_days = None;
    anatomy.age_reason = Some(reason.into());
}

impl SnapshotRef {
    fn label(&self) -> String {
        match self {
            SnapshotRef::WorkingCopy => "working copy".into(),
            SnapshotRef::Git(revision) => format!("Git {revision}"),
            SnapshotRef::Jujutsu(revision) => format!("JJ {revision}"),
        }
    }
}

pub fn compare_file(
    parsers: &[Box<dyn Parser>],
    repo: &Path,
    file: &str,
    request: &AnatomySelection,
) -> AnatomyComparison {
    let selection = match resolve_selection(repo, request) {
        Ok(selection) => selection,
        Err(reason) => {
            let unavailable = AnatomyEndpoint {
                label: request.since.clone(),
                source_id: None,
                status: "unavailable".into(),
                anatomy: None,
                reason: Some(reason),
            };
            return AnatomyComparison {
                file: file.into(),
                comparison_id: request.since.clone().unwrap_or_else(|| "unresolved".into()),
                has_baseline: request.mode != "none",
                before: unavailable.clone(),
                after: unavailable,
            };
        }
    };
    if !valid_file_path(file) {
        let unavailable = AnatomyEndpoint {
            label: None,
            source_id: None,
            status: "unavailable".into(),
            anatomy: None,
            reason: Some("invalid repository path".into()),
        };
        return AnatomyComparison {
            file: file.into(),
            comparison_id: selection.id,
            has_baseline: selection.has_baseline,
            before: unavailable.clone(),
            after: unavailable,
        };
    }
    let before = endpoint(parsers, repo, file, selection.before.as_ref());
    let after = endpoint(parsers, repo, file, Some(&selection.after));
    AnatomyComparison {
        file: file.into(),
        comparison_id: selection.id,
        has_baseline: selection.has_baseline,
        before,
        after,
    }
}

pub fn analyze_files_after(
    parsers: &[Box<dyn Parser>],
    repo: &Path,
    files: &[String],
    request: &AnatomySelection,
) -> Vec<AnatomyEndpointWithFile> {
    let selection = resolve_selection(repo, request);
    files
        .iter()
        .map(|file| {
            let result = match &selection {
                _ if !valid_file_path(file) => AnatomyEndpoint {
                    label: None,
                    source_id: None,
                    status: "unavailable".into(),
                    anatomy: None,
                    reason: Some("invalid repository path".into()),
                },
                Ok(selection) => endpoint(parsers, repo, file, Some(&selection.after)),
                Err(reason) => AnatomyEndpoint {
                    label: None,
                    source_id: None,
                    status: "unavailable".into(),
                    anatomy: None,
                    reason: Some(reason.clone()),
                },
            };
            AnatomyEndpointWithFile {
                file: file.clone(),
                endpoint: result,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnatomyEndpointWithFile {
    pub file: String,
    #[serde(flatten)]
    pub endpoint: AnatomyEndpoint,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::tempdir;

    fn git(repo: &Path, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command")
    }

    fn jj(repo: &Path, args: &[&str]) -> Output {
        Command::new("jj")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("jj command")
    }

    fn init_repo() -> tempfile::TempDir {
        let dir = tempdir().expect("temp repo");
        assert!(git(dir.path(), &["init", "-q"]).status.success());
        git(
            dir.path(),
            &["config", "user.email", "test@example.invalid"],
        );
        git(dir.path(), &["config", "user.name", "Test"]);
        dir
    }

    fn git_at(repo: &Path, args: &[&str], timestamp: &str) -> Output {
        Command::new("git")
            .args(args)
            .env("GIT_AUTHOR_DATE", timestamp)
            .env("GIT_COMMITTER_DATE", timestamp)
            .current_dir(repo)
            .output()
            .expect("git command")
    }

    fn rust_parsers() -> Vec<Box<dyn Parser>> {
        use crate::parser::PluggableParser;
        vec![Box::new(PluggableParser {
            extensions: vec!["rs"],
            language: tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
            queries: "",
            default_node_kind: "module",
            default_edge_kind: "use",
        })]
    }

    /// Given a file added then modified in Git, when comparing historical
    /// endpoints, then line count uses each endpoint's bytes and age starts at
    /// the first addition rather than the last modification.
    #[test]
    fn historical_anatomy_reports_endpoint_lines_and_first_appearance_age() {
        let repo = init_repo();
        fs::write(repo.path().join("src.rs"), "fn first() {}\n").unwrap();
        assert!(git(repo.path(), &["add", "src.rs"]).status.success());
        assert!(git_at(
            repo.path(),
            &["commit", "-qm", "add source"],
            "2020-01-01T10:00:00+00:00"
        )
        .status
        .success());
        let before = String::from_utf8_lossy(&git(repo.path(), &["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();

        fs::write(
            repo.path().join("src.rs"),
            "fn first() { if true {} }\n// extra\n",
        )
        .unwrap();
        assert!(git(repo.path(), &["add", "src.rs"]).status.success());
        assert!(git_at(
            repo.path(),
            &["commit", "-qm", "modify source"],
            "2020-01-04T10:00:00+00:00"
        )
        .status
        .success());
        let after = String::from_utf8_lossy(&git(repo.path(), &["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();

        let comparison = compare_file(
            &rust_parsers(),
            repo.path(),
            "src.rs",
            &AnatomySelection {
                mode: "change".into(),
                since: Some(format!("{before}..{after}")),
            },
        );
        let before = serde_json::to_value(comparison.before.anatomy.unwrap()).unwrap();
        let after = serde_json::to_value(comparison.after.anatomy.unwrap()).unwrap();
        assert_eq!(before["lineCount"], 1);
        assert_eq!(after["lineCount"], 2);
        assert_eq!(before["firstSeenAt"], "2020-01-01T10:00:00Z", "{before}");
        assert_eq!(after["firstSeenAt"], "2020-01-01T10:00:00Z", "{after}");
        assert_eq!(before["ageAsOf"], "2020-01-01T10:00:00Z");
        assert_eq!(after["ageAsOf"], "2020-01-04T10:00:00Z");
        assert_eq!(before["ageDays"], 0);
        assert_eq!(after["ageDays"], 3);
        assert!(after["ageReason"].is_null());
    }

    /// Given an untracked Git working-copy file, when its anatomy is measured,
    /// then actual size is known while Git-first-appearance age stays unknown.
    #[test]
    fn untracked_file_has_line_count_but_no_git_age() {
        let repo = init_repo();
        fs::write(repo.path().join("new.rs"), "fn old_file() {}\n").unwrap();
        assert!(git(repo.path(), &["add", "new.rs"]).status.success());
        assert!(git(repo.path(), &["commit", "-qm", "add old file"])
            .status
            .success());
        assert!(git(repo.path(), &["rm", "-q", "new.rs"]).status.success());
        assert!(git(repo.path(), &["commit", "-qm", "delete old file"])
            .status
            .success());
        fs::write(repo.path().join("new.rs"), "fn new_file() {}\n\n").unwrap();
        let comparison = compare_file(
            &rust_parsers(),
            repo.path(),
            "new.rs",
            &AnatomySelection {
                mode: "working_copy".into(),
                since: Some("@".into()),
            },
        );
        let after = serde_json::to_value(comparison.after.anatomy.unwrap()).unwrap();
        assert_eq!(after["lineCount"], 2);
        assert!(after["firstSeenAt"].is_null());
        assert!(after["ageAsOf"].is_null());
        assert!(after["ageDays"].is_null());
        assert!(after["ageReason"].as_str().unwrap().contains("untracked"));
    }

    /// Given a Git file with a Unicode rename and unusual path bytes, when the
    /// history index is built, then NUL-framed names survive and age follows
    /// the rename to its original introduction.
    #[test]
    fn git_history_index_tracks_renames_and_nul_safe_paths() {
        let repo = init_repo();
        let original = "src/with\t tab\nand newline.rs";
        let renamed = "src/renamed ünicode.rs";
        fs::create_dir_all(repo.path().join("src")).unwrap();
        fs::write(repo.path().join(original), "fn sample() {}\n").unwrap();
        assert!(git(repo.path(), &["add", "--", original]).status.success());
        assert!(git_at(
            repo.path(),
            &["commit", "-qm", "add odd path"],
            "2020-02-01T10:00:00+00:00"
        )
        .status
        .success());
        fs::rename(repo.path().join(original), repo.path().join(renamed)).unwrap();
        assert!(git(repo.path(), &["add", "-A"]).status.success());
        assert!(git_at(
            repo.path(),
            &["commit", "-qm", "rename source"],
            "2020-02-02T10:00:00+00:00"
        )
        .status
        .success());
        let revision = String::from_utf8_lossy(&git(repo.path(), &["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();
        let index = anatomy_history::for_revision(repo.path(), &revision).unwrap();
        assert_eq!(
            index.first_appearance(renamed).unwrap().at,
            "2020-02-01T10:00:00Z"
        );
    }

    /// Given a shallow clone whose file existed before its history boundary,
    /// when age is requested, then the boundary commit is not claimed as birth.
    #[test]
    fn shallow_history_boundary_keeps_first_appearance_unknown() {
        let source = init_repo();
        fs::write(source.path().join("src.rs"), "fn first() {}\n").unwrap();
        assert!(git(source.path(), &["add", "src.rs"]).status.success());
        assert!(git_at(
            source.path(),
            &["commit", "-qm", "add"],
            "2020-01-01T10:00:00+00:00"
        )
        .status
        .success());
        fs::write(source.path().join("src.rs"), "fn later() {}\n").unwrap();
        assert!(git(source.path(), &["add", "src.rs"]).status.success());
        assert!(git_at(
            source.path(),
            &["commit", "-qm", "modify"],
            "2020-01-02T10:00:00+00:00"
        )
        .status
        .success());

        let clone = tempdir().unwrap();
        let file_url = format!("file://{}", source.path().display());
        let cloned = Command::new("git")
            .args(["clone", "--quiet", "--depth", "1", &file_url])
            .arg(clone.path())
            .output()
            .unwrap();
        assert!(
            cloned.status.success(),
            "{}",
            String::from_utf8_lossy(&cloned.stderr)
        );
        let revision = String::from_utf8_lossy(&git(clone.path(), &["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();
        let index = anatomy_history::for_revision(clone.path(), &revision).unwrap();
        assert!(index
            .first_appearance("src.rs")
            .unwrap_err()
            .contains("shallow"));
    }

    #[test]
    fn working_copy_selection_pins_head_and_content_identity() {
        let repo = init_repo();
        fs::write(repo.path().join("a.js"), "const value = 1;\n").unwrap();
        assert!(git(repo.path(), &["add", "a.js"]).status.success());
        assert!(git(repo.path(), &["commit", "-qm", "base"])
            .status
            .success());
        let first = resolve_selection(
            repo.path(),
            &AnatomySelection {
                mode: "working_copy".into(),
                since: Some("@".into()),
            },
        )
        .unwrap();
        fs::write(repo.path().join("a.js"), "const value = 2;\n").unwrap();
        let before = read_snapshot(repo.path(), first.before.as_ref().unwrap(), "a.js")
            .unwrap()
            .unwrap();
        let after = read_snapshot(repo.path(), &first.after, "a.js")
            .unwrap()
            .unwrap();
        assert_ne!(before.1, after.1);
        assert!(before.2.starts_with("Git "));
        assert_eq!(first.has_baseline, true);
    }

    #[test]
    fn explicit_range_keeps_requested_endpoints_and_absence_distinct() {
        let repo = init_repo();
        fs::write(repo.path().join("a.js"), "const value = 1;\n").unwrap();
        assert!(git(repo.path(), &["add", "a.js"]).status.success());
        assert!(git(repo.path(), &["commit", "-qm", "base"])
            .status
            .success());
        let base = String::from_utf8_lossy(&git(repo.path(), &["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();
        fs::write(repo.path().join("new.js"), "const added = true;\n").unwrap();
        assert!(git(repo.path(), &["add", "new.js"]).status.success());
        assert!(git(repo.path(), &["commit", "-qm", "add new"])
            .status
            .success());
        let head = String::from_utf8_lossy(&git(repo.path(), &["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();
        let selection = resolve_selection(
            repo.path(),
            &AnatomySelection {
                mode: "change".into(),
                since: Some(format!("{base}..{head}")),
            },
        )
        .unwrap();
        assert_eq!(selection.id, format!("{base}..{head}"));
        assert!(
            read_snapshot(repo.path(), selection.before.as_ref().unwrap(), "new.js")
                .unwrap()
                .is_none()
        );
        assert!(read_snapshot(repo.path(), &selection.after, "new.js")
            .unwrap()
            .is_some());
    }

    #[test]
    fn no_vcs_selection_has_no_invented_baseline() {
        let repo = tempdir().unwrap();
        let selection = resolve_selection(
            repo.path(),
            &AnatomySelection {
                mode: "none".into(),
                since: None,
            },
        )
        .unwrap();
        assert!(!selection.has_baseline);
        assert!(selection.before.is_none());
        assert_eq!(selection.after, SnapshotRef::WorkingCopy);
    }

    #[test]
    fn git_unborn_repository_is_no_baseline_not_command_failure() {
        let repo = init_repo();
        let selection = resolve_selection(
            repo.path(),
            &AnatomySelection {
                mode: "working_copy".into(),
                since: Some("@".into()),
            },
        )
        .unwrap();
        assert!(!selection.has_baseline);
        assert!(selection.before.is_none());
        assert_eq!(selection.after, SnapshotRef::WorkingCopy);
    }

    #[test]
    fn historical_selection_without_vcs_is_unavailable_not_working_copy() {
        let repo = tempdir().unwrap();
        let result = resolve_selection(
            repo.path(),
            &AnatomySelection {
                mode: "change".into(),
                since: Some("abc123".into()),
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn rejects_parent_paths_before_snapshot_read() {
        assert!(!valid_file_path("../outside.rs"));
        assert!(!valid_file_path("/absolute/path.rs"));
        assert!(valid_file_path("src/module.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_working_copy_symlink_that_escapes_repository() {
        use std::os::unix::fs::symlink;

        let repo = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.rs"), "fn secret() {}\n").unwrap();
        symlink(
            outside.path().join("secret.rs"),
            repo.path().join("link.rs"),
        )
        .unwrap();
        assert!(read_working_copy_file(repo.path(), "link.rs").is_err());
    }

    /// Given a JJ working copy with a source file, when its anatomy endpoint is
    /// resolved and the file changes later, then the old endpoint stays pinned.
    #[test]
    fn jujutsu_working_copy_endpoint_reads_file_and_pins_identity() {
        use crate::parser::PluggableParser;

        let repo = tempdir().unwrap();
        let initialized = jj(repo.path(), &["git", "init"]);
        if initialized.status.code() == Some(127) {
            eprintln!("jj is unavailable; skipping Jujutsu endpoint integration test");
            return;
        }
        assert!(
            initialized.status.success(),
            "{}",
            String::from_utf8_lossy(&initialized.stderr)
        );

        fs::write(repo.path().join("src.rs"), "fn work() {}\n").unwrap();
        let selection = resolve_selection(
            repo.path(),
            &AnatomySelection {
                mode: "working_copy".into(),
                since: Some("@".into()),
            },
        )
        .expect("resolve JJ working-copy endpoint");
        assert!(selection.has_baseline);

        let parsers: Vec<Box<dyn Parser>> = vec![Box::new(PluggableParser {
            extensions: vec!["rs"],
            language: tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
            queries: "",
            default_node_kind: "module",
            default_edge_kind: "use",
        })];
        let comparison = compare_file(
            &parsers,
            repo.path(),
            "src.rs",
            &AnatomySelection {
                mode: "working_copy".into(),
                since: Some("@".into()),
            },
        );
        assert_eq!(comparison.before.status, "absent");
        assert_eq!(comparison.after.status, "available");
        assert!(comparison
            .after
            .anatomy
            .as_ref()
            .unwrap()
            .callables
            .iter()
            .any(|callable| callable.name == "work"));

        let pinned = endpoint(&parsers, repo.path(), "src.rs", Some(&selection.after));
        assert_eq!(pinned.source_id, comparison.after.source_id);
        fs::write(repo.path().join("src.rs"), "fn changed() {}\n").unwrap();
        let reread = endpoint(&parsers, repo.path(), "src.rs", Some(&selection.after));
        assert_eq!(reread.source_id, pinned.source_id);
        assert!(reread
            .anatomy
            .as_ref()
            .unwrap()
            .callables
            .iter()
            .any(|callable| callable.name == "work"));
    }
}

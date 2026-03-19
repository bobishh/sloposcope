use crate::work_store::{NewVerificationRecord, UnitVerdict, VerdictArtifact};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

use super::{workspace_name_for_task, ConductorWorkspaceSpec};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeterministicWriteSpec {
    pub relative_path: String,
    pub content: String,
    pub validation_command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRunResult {
    pub command: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCodeWorkerResult {
    pub mission_id: String,
    pub objective_id: String,
    pub session_id: String,
    pub workspace: ConductorWorkspaceSpec,
    pub relative_path: String,
    pub absolute_path: String,
    pub touched_paths: Vec<String>,
    pub patch: String,
    pub validation: ValidationRunResult,
}

impl LocalCodeWorkerResult {
    pub fn verification_status(&self) -> &'static str {
        if self.validation.success {
            "passed"
        } else {
            "failed"
        }
    }

    pub fn to_verification_record(&self) -> NewVerificationRecord {
        NewVerificationRecord {
            mission_id: self.mission_id.clone(),
            objective_id: Some(self.objective_id.clone()),
            session_id: Some(self.session_id.clone()),
            status: self.verification_status().to_string(),
            summary: format!(
                "deterministic worker {} for {}",
                if self.validation.success {
                    "passed"
                } else {
                    "failed"
                },
                self.relative_path
            ),
            details: serde_json::json!({
                "workspacePath": self.workspace.workspace_path,
                "relativePath": self.relative_path,
                "absolutePath": self.absolute_path,
                "touchedPaths": self.touched_paths,
                "patch": self.patch,
                "validation": {
                    "command": self.validation.command,
                    "success": self.validation.success,
                    "exitCode": self.validation.exit_code,
                    "stdout": self.validation.stdout,
                    "stderr": self.validation.stderr
                }
            }),
        }
    }

    pub fn to_unit_verdict(&self, submitted_at: impl Into<String>) -> UnitVerdict {
        let mut artifacts = vec![VerdictArtifact {
            kind: "patch".to_string(),
            content: self.patch.clone(),
        }];
        if !self.validation.stdout.is_empty() {
            artifacts.push(VerdictArtifact {
                kind: "validation_stdout".to_string(),
                content: self.validation.stdout.clone(),
            });
        }
        if !self.validation.stderr.is_empty() {
            artifacts.push(VerdictArtifact {
                kind: "validation_stderr".to_string(),
                content: self.validation.stderr.clone(),
            });
        }
        UnitVerdict {
            unit_id: self.objective_id.clone(),
            task_id: self.mission_id.clone(),
            status: if self.validation.success {
                "done".to_string()
            } else {
                "failed".to_string()
            },
            summary: format!(
                "deterministic worker {} {}",
                if self.validation.success {
                    "completed"
                } else {
                    "failed"
                },
                self.relative_path
            ),
            submitted_at: submitted_at.into(),
            artifacts,
        }
    }
}

pub fn ensure_isolated_git_worktree(
    repo_root: &Path,
    workspace_key: &str,
) -> Result<ConductorWorkspaceSpec, String> {
    let root = canonical_git_root(repo_root)?;
    let worktree_root = root.join(".sloposcope").join("worktrees");
    fs::create_dir_all(&worktree_root)
        .map_err(|err| format!("Failed to create worktree root: {err}"))?;
    let workspace_name = workspace_name_for_task(workspace_key)?;
    let workspace_path = worktree_root.join(&workspace_name);
    if !workspace_path.starts_with(&worktree_root) {
        return Err("Worktree path escaped worktree root".to_string());
    }

    if workspace_path.join(".git").exists() {
        let canonical = workspace_path
            .canonicalize()
            .map_err(|err| format!("Failed to canonicalize existing worktree: {err}"))?;
        return Ok(ConductorWorkspaceSpec {
            task_id: workspace_key.to_string(),
            workspace_name,
            workspace_path: canonical.display().to_string(),
        });
    }

    if workspace_path.exists() {
        fs::remove_dir_all(&workspace_path)
            .map_err(|err| format!("Failed to clear stale worktree path: {err}"))?;
    }

    run_git(
        &root,
        [
            "worktree",
            "add",
            "--detach",
            workspace_path
                .to_str()
                .ok_or_else(|| "Worktree path is not valid UTF-8".to_string())?,
            "HEAD",
        ],
    )?;

    let canonical = workspace_path
        .canonicalize()
        .map_err(|err| format!("Failed to canonicalize created worktree: {err}"))?;
    if !canonical.starts_with(
        &worktree_root
            .canonicalize()
            .map_err(|err| format!("Failed to canonicalize worktree root after create: {err}"))?,
    ) {
        return Err("Worktree canonical path escaped root".to_string());
    }

    link_shared_dependencies(&root, &canonical)?;

    Ok(ConductorWorkspaceSpec {
        task_id: workspace_key.to_string(),
        workspace_name,
        workspace_path: canonical.display().to_string(),
    })
}

pub fn run_deterministic_code_worker(
    repo_root: &Path,
    mission_id: &str,
    objective_id: &str,
    session_id: &str,
    spec: &DeterministicWriteSpec,
) -> Result<LocalCodeWorkerResult, String> {
    let relative_path = sanitize_relative_path(&spec.relative_path)?;
    let workspace = ensure_isolated_git_worktree(
        repo_root,
        &format!("{mission_id}-{objective_id}-{session_id}"),
    )?;
    let workspace_path = PathBuf::from(&workspace.workspace_path);
    let absolute_path = workspace_path.join(&relative_path);
    if let Some(parent) = absolute_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create parent directories: {err}"))?;
    }
    fs::write(&absolute_path, &spec.content)
        .map_err(|err| format!("Failed to write file in worktree: {err}"))?;

    let patch = capture_patch(&workspace_path, &relative_path)?;
    let validation = run_validation_command(&workspace_path, &spec.validation_command)?;

    Ok(LocalCodeWorkerResult {
        mission_id: mission_id.to_string(),
        objective_id: objective_id.to_string(),
        session_id: session_id.to_string(),
        workspace,
        relative_path: relative_path.to_string_lossy().to_string(),
        absolute_path: absolute_path.display().to_string(),
        touched_paths: vec![relative_path.to_string_lossy().to_string()],
        patch,
        validation,
    })
}

fn canonical_git_root(repo_root: &Path) -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(repo_root)
        .output()
        .map_err(|err| format!("Failed to run git rev-parse: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let root = PathBuf::from(text);
    root.canonicalize()
        .map_err(|err| format!("Failed to canonicalize git root: {err}"))
}

fn sanitize_relative_path(raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw.trim());
    if path.as_os_str().is_empty() {
        return Err("relative_path is empty".to_string());
    }
    if path.is_absolute() {
        return Err("relative_path must be relative".to_string());
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err("relative_path escaped worktree root".to_string());
    }
    Ok(path.to_path_buf())
}

fn run_git<const N: usize>(cwd: &Path, args: [&str; N]) -> Result<Output, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|err| format!("Failed to run git {:?}: {err}", &args))?;
    if !output.status.success() {
        return Err(format!(
            "git {:?} failed: {}",
            &args,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output)
}

fn capture_patch(worktree: &Path, relative_path: &Path) -> Result<String, String> {
    let relative = relative_path
        .to_str()
        .ok_or_else(|| "relative_path is not valid UTF-8".to_string())?;
    let diff = run_git(worktree, ["diff", "--", relative])?;
    let patch = String::from_utf8_lossy(&diff.stdout).to_string();
    if !patch.trim().is_empty() {
        return Ok(patch);
    }

    let absolute = worktree.join(relative_path);
    if !absolute.is_file() {
        return Ok(String::new());
    }

    let fallback = Command::new("git")
        .args([
            "diff",
            "--no-index",
            "--",
            "/dev/null",
            absolute
                .to_str()
                .ok_or_else(|| "absolute path is not valid UTF-8".to_string())?,
        ])
        .current_dir(worktree)
        .output()
        .map_err(|err| format!("Failed to run git diff --no-index: {err}"))?;
    let mut fallback_patch = String::from_utf8_lossy(&fallback.stdout).to_string();
    if fallback_patch.trim().is_empty() {
        fallback_patch = String::from_utf8_lossy(&fallback.stderr).to_string();
    }
    Ok(fallback_patch)
}

fn run_validation_command(worktree: &Path, command: &str) -> Result<ValidationRunResult, String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let output = Command::new(&shell)
        .arg("-lc")
        .arg(command)
        .current_dir(worktree)
        .output()
        .map_err(|err| format!("Failed to run validation command: {err}"))?;
    Ok(ValidationRunResult {
        command: command.to_string(),
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn link_shared_dependencies(repo_root: &Path, worktree: &Path) -> Result<(), String> {
    let shared_node_modules = repo_root.join("node_modules");
    let target_node_modules = worktree.join("node_modules");
    if shared_node_modules.is_dir() && !target_node_modules.exists() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(&shared_node_modules, &target_node_modules)
            .map_err(|err| format!("Failed to symlink node_modules into worktree: {err}"))?;
    }
    Ok(())
}

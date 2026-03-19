#![recursion_limit = "256"]

mod agent_harness;
mod cluster_service;
mod conductor;
mod graph;
pub mod graph_query;
pub mod graph_store;
mod graph_view;
mod jj;
mod julia_sidecar;
mod parser;
pub mod search_service;
pub mod vcs;
pub mod watchdog;
pub mod watchdog_config;
pub mod watchdog_types;
pub mod work_store;

use cluster_service::ClusterService;
use conductor::RunnerAdapter as _;
use graph::{Graph, Node};
use graph_store::{GraphStore, SqliteGraphStore};
use graph_view::{GraphViewRequest, GraphViewResponse, GraphViewSeed, GraphViewService};
use ignore::WalkBuilder;
use notify::{event::EventKind, Event, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use parser::{Parser, PluggableParser};
use search_service::{SearchResult, SearchService};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use work_store::{
    derive_agent_session_runtime_slice_at, AgentProfile, AgentSessionRuntimeRecord, MissionInput,
    MissionRecord, MissionStateView, NewObjective, NewVerificationRecord, PatchDecisionRecord,
    PatchGateRecord, UnitVerdict, VerdictArtifact, VerificationRecord, WorkStore, WorkUnitStatus,
};

struct AppState {
    repo: Mutex<PathBuf>,
    parsers: Vec<Box<dyn Parser>>,
    watcher: Mutex<Option<RecommendedWatcher>>,
    graph: Mutex<Graph>,
    last_graph_view_request: Mutex<GraphViewRequest>,
    last_since: Mutex<Option<String>>,
    last_include_neighbors: Mutex<bool>,
    last_branch: Mutex<String>,
    last_revision: Mutex<String>,
    watcher_flush_scheduled: Mutex<bool>,
    watcher_pending_vcs_meta: Mutex<bool>,
    watcher_last_vcs_meta_at: Mutex<Option<Instant>>,
    watcher_pending_paths: Mutex<Vec<PathBuf>>,
    cluster_refresh_scheduled: Mutex<bool>,
    watchdog: Mutex<watchdog::WatchdogRuntimeState>,
    conductor: Mutex<conductor::ConductorRuntimeState>,
}

fn debug_enabled() -> bool {
    static DEBUG: OnceLock<bool> = OnceLock::new();
    *DEBUG.get_or_init(|| {
        match std::env::var("SLOPOSCOPE_DEBUG").or_else(|_| std::env::var("EYELOSS_DEBUG")) {
            Ok(v) => {
                let v = v.to_ascii_lowercase();
                matches!(v.as_str(), "1" | "true" | "yes" | "on")
            }
            Err(_) => cfg!(debug_assertions),
        }
    })
}

fn vcs_poll_interval() -> Duration {
    static POLL_INTERVAL: OnceLock<Duration> = OnceLock::new();
    *POLL_INTERVAL.get_or_init(|| {
        // Faster than previous 5s default so branch switches propagate quicker.
        // Override via SLOPOSCOPE_VCS_POLL_MS / EYELOSS_VCS_POLL_MS.
        let raw = std::env::var("SLOPOSCOPE_VCS_POLL_MS")
            .or_else(|_| std::env::var("EYELOSS_VCS_POLL_MS"))
            .ok();
        let parsed = raw
            .as_deref()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1000);
        Duration::from_millis(parsed.clamp(250, 10_000))
    })
}

fn watcher_debounce_interval() -> Duration {
    static WATCH_DEBOUNCE: OnceLock<Duration> = OnceLock::new();
    *WATCH_DEBOUNCE.get_or_init(|| {
        let raw = std::env::var("SLOPOSCOPE_WATCH_DEBOUNCE_MS")
            .or_else(|_| std::env::var("EYELOSS_WATCH_DEBOUNCE_MS"))
            .ok();
        let parsed = raw
            .as_deref()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(450);
        Duration::from_millis(parsed.clamp(100, 5_000))
    })
}

fn watcher_vcs_quiet_period() -> Duration {
    static WATCH_VCS_QUIET: OnceLock<Duration> = OnceLock::new();
    *WATCH_VCS_QUIET.get_or_init(|| {
        let raw = std::env::var("SLOPOSCOPE_WATCH_VCS_QUIET_MS")
            .or_else(|_| std::env::var("EYELOSS_WATCH_VCS_QUIET_MS"))
            .ok();
        let parsed = raw
            .as_deref()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1200);
        Duration::from_millis(parsed.clamp(200, 10_000))
    })
}

fn watchdog_trigger_cooldown() -> Duration {
    static WATCHDOG_TRIGGER_COOLDOWN: OnceLock<Duration> = OnceLock::new();
    *WATCHDOG_TRIGGER_COOLDOWN.get_or_init(|| {
        let raw = std::env::var("SLOPOSCOPE_WATCHDOG_TRIGGER_COOLDOWN_MS")
            .or_else(|_| std::env::var("EYELOSS_WATCHDOG_TRIGGER_COOLDOWN_MS"))
            .ok();
        let parsed = raw
            .as_deref()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30_000);
        Duration::from_millis(parsed.clamp(1_000, 300_000))
    })
}

fn watchdog_auto_trigger_min_interval() -> Duration {
    static WATCHDOG_AUTO_TRIGGER_MIN_INTERVAL: OnceLock<Duration> = OnceLock::new();
    *WATCHDOG_AUTO_TRIGGER_MIN_INTERVAL.get_or_init(|| {
        let raw = std::env::var("SLOPOSCOPE_WATCHDOG_AUTO_MIN_INTERVAL_MS")
            .or_else(|_| std::env::var("EYELOSS_WATCHDOG_AUTO_MIN_INTERVAL_MS"))
            .ok();
        let parsed = raw
            .as_deref()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10_000);
        Duration::from_millis(parsed.clamp(1_000, 300_000))
    })
}

fn watchdog_auto_trigger_quality_gate() -> watchdog::WatchdogEvalQualityGate {
    static AUTO_TRIGGER_QUALITY_GATE: OnceLock<watchdog::WatchdogEvalQualityGate> = OnceLock::new();
    if let Ok(value) = std::env::var("SLOPOSCOPE_WATCHDOG_EVAL_GATE")
        .or_else(|_| std::env::var("EYELOSS_WATCHDOG_EVAL_GATE"))
    {
        let lowered = value.trim().to_ascii_lowercase();
        if matches!(lowered.as_str(), "0" | "false" | "off" | "no") {
            return watchdog::WatchdogEvalQualityGate {
                passed: true,
                reason: "auto-trigger eval quality gate disabled by env".to_string(),
                traces_evaluated: 0,
                deduped_issues_per_run: 0.0,
                status_change_rate: 0.0,
            };
        }
    }
    AUTO_TRIGGER_QUALITY_GATE
        .get_or_init(watchdog::evaluate_auto_trigger_quality_gate)
        .clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchdogAutoTriggerReason {
    BranchSwitchAfterQuietPeriod,
    LargeChangeBatch,
}

fn should_run_watchdog_auto_trigger(
    trigger_mode: &watchdog_types::WatchdogTriggerMode,
    reason: WatchdogAutoTriggerReason,
    changed_files_total: usize,
    max_files_in_snapshot: usize,
) -> bool {
    match (trigger_mode, reason) {
        (
            watchdog_types::WatchdogTriggerMode::OnBranchSwitchAfterQuietPeriod,
            WatchdogAutoTriggerReason::BranchSwitchAfterQuietPeriod,
        ) => true,
        (
            watchdog_types::WatchdogTriggerMode::OnLargeChangeBatch,
            WatchdogAutoTriggerReason::LargeChangeBatch,
        ) => changed_files_total >= max_files_in_snapshot,
        _ => false,
    }
}

fn unix_now_seconds() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => 0,
    }
}

fn should_defer_vcs_refresh(
    pending_vcs_meta: bool,
    last_vcs_meta_at: Option<Instant>,
    quiet_period: Duration,
) -> bool {
    if pending_vcs_meta {
        return true;
    }
    match last_vcs_meta_at {
        Some(last) => last.elapsed() < quiet_period,
        None => false,
    }
}

fn short_rev_label(rev: &str) -> String {
    if rev.len() > 12 {
        rev[..12].to_string()
    } else {
        rev.to_string()
    }
}

fn changed_set_fingerprint(changed: &HashMap<String, String>) -> String {
    let mut entries: Vec<(&String, &String)> = changed.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (path, status) in entries {
        std::hash::Hash::hash(path, &mut hasher);
        std::hash::Hash::hash(status, &mut hasher);
    }
    format!("{:016x}", std::hash::Hasher::finish(&hasher))
}

fn pending_paths_fingerprint(paths: &[PathBuf]) -> String {
    let mut ordered: Vec<&PathBuf> = paths.iter().collect();
    ordered.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for path in ordered {
        let display = path.display().to_string();
        std::hash::Hash::hash(&display, &mut hasher);
        match std::fs::metadata(path) {
            Ok(meta) => {
                std::hash::Hash::hash(&true, &mut hasher);
                std::hash::Hash::hash(&meta.len(), &mut hasher);
                let stamp = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(UNIX_EPOCH).ok());
                if let Some(duration) = stamp {
                    std::hash::Hash::hash(&duration.as_secs(), &mut hasher);
                    std::hash::Hash::hash(&duration.subsec_nanos(), &mut hasher);
                } else {
                    std::hash::Hash::hash(&0u64, &mut hasher);
                    std::hash::Hash::hash(&0u32, &mut hasher);
                }
            }
            Err(_) => {
                std::hash::Hash::hash(&false, &mut hasher);
            }
        }
    }
    format!("{:016x}", std::hash::Hasher::finish(&hasher))
}

fn unix_now_label() -> String {
    let secs = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => 0,
    };
    format!("unix:{secs}")
}

fn parse_watchdog_severity_label(label: &str) -> Option<watchdog_types::WatchdogSeverity> {
    match label.trim().to_ascii_lowercase().as_str() {
        "info" => Some(watchdog_types::WatchdogSeverity::Info),
        "warning" | "warn" => Some(watchdog_types::WatchdogSeverity::Warning),
        "critical" | "error" => Some(watchdog_types::WatchdogSeverity::Critical),
        _ => None,
    }
}

fn normalize_mission_id(input: &MissionInput) -> Result<String, String> {
    let candidate = input
        .id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .or_else(|| {
            input
                .issue_key
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.trim().to_string())
        })
        .unwrap_or_else(|| input.title.trim().to_string());

    let normalized = candidate
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if normalized.is_empty() {
        return Err("Mission id is empty.".to_string());
    }
    if normalized.len() > 64 {
        return Err("Mission id is too long.".to_string());
    }
    Ok(normalized)
}

fn build_instruction_pack(repo: &Path) -> watchdog_types::WatchdogAgentsContext {
    watchdog::load_agents_context(repo)
}

fn render_instruction_pack(context: &watchdog_types::WatchdogAgentsContext) -> String {
    let mut lines = Vec::new();
    if let Some(path) = &context.load.path {
        lines.push(format!("Source: {path}"));
    }
    if context.rules.is_empty() {
        lines.push("Rules: none loaded".to_string());
    } else {
        lines.push("Rules:".to_string());
        for rule in context.rules.iter().take(64) {
            lines.push(format!("- {rule}"));
        }
    }
    lines.join("\n")
}

fn instruction_pack_hash(context: &watchdog_types::WatchdogAgentsContext) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    context.load.path.hash(&mut hasher);
    for rule in &context.rules {
        rule.hash(&mut hasher);
    }
    format!("{:016x}", std::hash::Hasher::finish(&hasher))
}

fn normalize_scope_path(scope: &str) -> String {
    let trimmed = scope.trim().replace('\\', "/");
    if trimmed.is_empty() {
        return String::new();
    }
    let without_prefix = trimmed.trim_start_matches("./").trim_start_matches('/');
    without_prefix.to_string()
}

fn parse_objective_scope_tag(line: &str) -> (Vec<String>, String) {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix("[scope:") else {
        return (Vec::new(), trimmed.to_string());
    };
    let Some(end_idx) = rest.find(']') else {
        return (Vec::new(), trimmed.to_string());
    };
    let raw_scopes = &rest[..end_idx];
    let scopes = raw_scopes
        .split(',')
        .map(normalize_scope_path)
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    let remainder = rest[end_idx + 1..].trim();
    (scopes, remainder.to_string())
}

fn fixture_objectives(input: &MissionInput) -> Vec<NewObjective> {
    let validation_command = input.validation_command.trim();
    let validation = if validation_command.is_empty() {
        Vec::new()
    } else {
        vec![validation_command.to_string()]
    };
    let lines = input
        .brief
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let objective_lines = if lines.is_empty() {
        vec![input.title.trim()]
    } else {
        lines
    };

    let mut objectives = Vec::new();
    let mut index = 0usize;
    while index < objective_lines.len() {
        let (scope_paths, mut prompt_body) = parse_objective_scope_tag(objective_lines[index]);
        if prompt_body.is_empty() && index + 1 < objective_lines.len() {
            prompt_body = objective_lines[index + 1].trim().to_string();
            if !prompt_body.is_empty() {
                index += 1;
            }
        }
        let objective_number = objectives.len() + 1;
        let title = if prompt_body.is_empty() {
            format!("Mission slice {}", objective_number)
        } else {
            prompt_body.clone()
        };
        objectives.push(NewObjective {
            id: format!("objective-{}", objective_number),
            title,
            prompt: if prompt_body.is_empty() {
                format!(
                    "Complete mission slice {} for {}.",
                    objective_number,
                    input.title.trim()
                )
            } else {
                prompt_body
            },
            scope_paths,
            expected_validation: validation.clone(),
            done_criteria: vec![
                "Relevant behavior slice is complete.".to_string(),
                "Validation command passes or reports expected failure.".to_string(),
            ],
            test_intent: Some("mission_slice".to_string()),
        });
        index += 1;
    }
    objectives
}

fn sanitize_identifier_fragment(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    let collapsed = normalized
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if collapsed.is_empty() {
        "mission".to_string()
    } else {
        collapsed
    }
}

fn deterministic_code_write_spec(
    mission: &MissionRecord,
    objective: &work_store::ObjectiveRecord,
) -> Result<conductor::DeterministicWriteSpec, String> {
    let has_web_scope = objective
        .meta
        .scope_paths
        .iter()
        .any(|path| path.starts_with("src/"));
    let has_rust_scope = objective
        .meta
        .scope_paths
        .iter()
        .any(|path| path.starts_with("src-tauri/"));

    if !has_web_scope && !has_rust_scope {
        return Err("Code worker requires scope under src/ or src-tauri/.".to_string());
    }

    let mission_slug = sanitize_identifier_fragment(&mission.id);
    let objective_slug = sanitize_identifier_fragment(&objective.id);
    let behavior_slug = sanitize_identifier_fragment(&mission.title);

    if has_web_scope {
        return Ok(conductor::DeterministicWriteSpec {
            relative_path: format!(
                "src/lib/generated_workers/{}_{}_worker.js",
                mission_slug, objective_slug
            ),
            content: format!(
                "export const {behavior}_worker = {{\n  missionId: '{mission_id}',\n  objectiveId: '{objective_id}',\n  summary: {summary:?},\n  touchedScope: {scope:?},\n}};\n",
                behavior = behavior_slug,
                mission_id = mission.id,
                objective_id = objective.id,
                summary = mission.brief,
                scope = objective.meta.scope_paths,
            ),
            validation_command: objective_validation_command(mission, objective),
        });
    }

    let const_name = format!(
        "{}_{}_WORKER",
        mission_slug.to_ascii_uppercase(),
        objective_slug.to_ascii_uppercase()
    );
    Ok(conductor::DeterministicWriteSpec {
        relative_path: format!(
            "src-tauri/src/generated_workers/{}_{}_worker.rs",
            mission_slug, objective_slug
        ),
        content: format!(
            "pub const {const_name}: &str = {summary:?};\n",
            const_name = const_name,
            summary = mission.brief,
        ),
        validation_command: objective_validation_command(mission, objective),
    })
}

fn objective_validation_command(
    mission: &MissionRecord,
    objective: &work_store::ObjectiveRecord,
) -> String {
    if objective.meta.expected_validation.is_empty() {
        mission.validation_command.clone()
    } else {
        objective.meta.expected_validation.join(" && ")
    }
}

fn touched_path_in_scope(path: &str, scope_paths: &[String]) -> bool {
    if scope_paths.is_empty() {
        return true;
    }
    let normalized = normalize_scope_path(path);
    scope_paths.iter().any(|scope| {
        let scope = normalize_scope_path(scope);
        normalized == scope
            || normalized.starts_with(&(scope.clone() + "/"))
            || (scope.ends_with('/') && normalized.starts_with(&scope))
    })
}

fn current_repo_revision(repo: &Path) -> String {
    vcs::get_current_revision(repo)
}

fn current_repo_branch(repo: &Path) -> String {
    vcs::get_current_branch(repo)
}

fn git_tracked_changes_for_paths(repo: &Path, paths: &[String]) -> Result<bool, String> {
    if paths.is_empty() {
        return Ok(false);
    }
    let mut command = Command::new("git");
    command.arg("status").arg("--porcelain").arg("--");
    for path in paths {
        command.arg(path);
    }
    let output = command
        .current_dir(repo)
        .output()
        .map_err(|err| format!("git status --porcelain failed: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git status --porcelain failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn capture_worktree_touched_paths(worktree: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree)
        .output()
        .map_err(|err| format!("git status --porcelain failed: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git status --porcelain failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut paths = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.len() < 4 {
            continue;
        }
        let mut path = line[3..].trim();
        if let Some((_, renamed_target)) = path.split_once(" -> ") {
            path = renamed_target.trim();
        }
        if path.is_empty() || is_ignored_runtime_path(path) {
            continue;
        }
        if !paths.iter().any(|existing| existing == path) {
            paths.push(path.to_string());
        }
    }
    Ok(paths)
}

fn capture_worktree_patch(worktree: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["diff"])
        .current_dir(worktree)
        .output()
        .map_err(|err| format!("git diff failed: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_shell_command(cwd: &Path, command: &str) -> Result<conductor::ValidationRunResult, String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let output = Command::new(&shell)
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .output()
        .map_err(|err| format!("Failed to run shell command: {err}"))?;
    Ok(conductor::ValidationRunResult {
        command: command.to_string(),
        success: output.status.success(),
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn validation_from_diagnostics(
    diagnostics: Option<&serde_json::Value>,
) -> Option<conductor::ValidationRunResult> {
    let validation = diagnostics?.get("validation")?;
    Some(conductor::ValidationRunResult {
        command: validation
            .get("command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        success: validation
            .get("success")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        exit_code: validation
            .get("exitCode")
            .and_then(serde_json::Value::as_i64)
            .map(|value| value as i32),
        stdout: validation
            .get("stdout")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        stderr: validation
            .get("stderr")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn write_patch_artifact(
    workspace_path: &str,
    objective_id: &str,
    patch_text: &str,
) -> Result<String, String> {
    let patch_artifact_path = PathBuf::from(workspace_path)
        .join("artifacts")
        .join(format!("{objective_id}.patch"));
    if let Some(parent) = patch_artifact_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("create code worker artifact dir: {err}"))?;
    }
    std::fs::write(&patch_artifact_path, patch_text)
        .map_err(|err| format!("write code worker patch artifact: {err}"))?;
    Ok(patch_artifact_path.display().to_string())
}

fn patch_summary_for_path(relative_path: &str) -> String {
    format!("Patch artifact for {relative_path}")
}

fn primary_path_from_touched_paths(touched_paths: &[String]) -> String {
    touched_paths
        .first()
        .cloned()
        .unwrap_or_else(|| "unknown".to_string())
}

fn ensure_apply_freshness(
    repo: &Path,
    mission: &MissionRecord,
    touched_paths: &[String],
    base_revision: &str,
) -> Result<(), String> {
    let repo_canonical = repo
        .canonicalize()
        .map_err(|err| format!("canonicalize active repo: {err}"))?;
    let mission_repo_canonical = PathBuf::from(&mission.repo_path)
        .canonicalize()
        .map_err(|err| format!("canonicalize mission repo: {err}"))?;
    if repo_canonical != mission_repo_canonical {
        return Err("mission repo does not match active map".to_string());
    }
    let current_revision = current_repo_revision(repo);
    if current_revision != base_revision {
        return Err(format!(
            "stale map: current revision {} != patch base {}",
            current_revision, base_revision
        ));
    }
    if git_tracked_changes_for_paths(repo, touched_paths)? {
        return Err("active map has incompatible local changes in touched paths".to_string());
    }
    Ok(())
}

fn apply_patch_text(
    repo: &Path,
    mission_id: &str,
    objective_id: &str,
    patch_text: &str,
) -> Result<(), String> {
    let patch_dir = repo.join(".sloposcope").join("tmp");
    std::fs::create_dir_all(&patch_dir).map_err(|err| format!("create patch tmp dir: {err}"))?;
    let patch_file = patch_dir.join(format!("{}-{}.patch", mission_id, objective_id));
    std::fs::write(&patch_file, patch_text).map_err(|err| format!("write patch file: {err}"))?;

    let check_output = Command::new("git")
        .args(["apply", "--check", patch_file.to_str().unwrap_or("")])
        .current_dir(repo)
        .output()
        .map_err(|err| format!("git apply --check failed: {err}"))?;
    if !check_output.status.success() {
        return Err(String::from_utf8_lossy(&check_output.stderr)
            .trim()
            .to_string());
    }
    let apply_output = Command::new("git")
        .args(["apply", patch_file.to_str().unwrap_or("")])
        .current_dir(repo)
        .output()
        .map_err(|err| format!("git apply failed: {err}"))?;
    if !apply_output.status.success() {
        return Err(String::from_utf8_lossy(&apply_output.stderr)
            .trim()
            .to_string());
    }
    Ok(())
}

fn build_agent_profile_record(input: AgentProfileInput) -> Result<AgentProfile, String> {
    let now = unix_now_label();
    let id = input
        .id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(sanitize_identifier_fragment)
        .or_else(|| {
            let label = input.label.trim();
            if label.is_empty() {
                None
            } else {
                Some(sanitize_identifier_fragment(label))
            }
        })
        .ok_or_else(|| "Agent profile id/label is empty.".to_string())?;
    Ok(AgentProfile {
        id,
        label: input.label.trim().to_string(),
        provider: input.provider.trim().to_string(),
        model: input
            .model
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        command: input.command.unwrap_or_default().trim().to_string(),
        args: input.args.unwrap_or_default(),
        env: input.env.unwrap_or_else(|| serde_json::json!({})),
        capabilities: input.capabilities.unwrap_or_default(),
        transport_kind: input
            .transport_kind
            .unwrap_or_else(|| "internal".to_string())
            .trim()
            .to_string(),
        active: input.active.unwrap_or(false),
        created_at: now.clone(),
        updated_at: now,
    })
}

fn resolve_requested_agent_profile(
    store: &WorkStore,
    requested_profile_id: Option<&str>,
) -> Result<Option<AgentProfile>, String> {
    if let Some(profile_id) = requested_profile_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return store
            .get_agent_profile(profile_id)?
            .ok_or_else(|| format!("Agent profile not found: {profile_id}"))
            .map(Some);
    }
    store.get_active_agent_profile()
}

fn verification_details_patch_base_revision(details: &serde_json::Value) -> Option<String> {
    details
        .get("baseRevision")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn latest_session_for_objective<'a>(
    view: &'a MissionStateView,
    objective_id: &str,
) -> Option<&'a AgentSessionRuntimeRecord> {
    view.sessions
        .iter()
        .find(|session| session.objective_id.as_deref() == Some(objective_id))
}

fn latest_verification_for_objective<'a>(
    view: &'a MissionStateView,
    objective_id: &str,
) -> Option<&'a VerificationRecord> {
    view.verifications
        .iter()
        .find(|verification| verification.objective_id.as_deref() == Some(objective_id))
}

#[derive(Debug, Clone)]
struct ObjectiveSchedulerEntry {
    state: String,
    reason: Option<String>,
}

#[derive(Debug, Clone)]
struct MissionSchedulerSummary {
    ready_count: usize,
    blocked_count: usize,
    active_count: usize,
    stale_session_count: usize,
    next_objective_id: Option<String>,
    by_objective: HashMap<String, ObjectiveSchedulerEntry>,
}

fn normalize_scope_paths(paths: &[String]) -> Vec<String> {
    paths.iter()
        .map(|path| normalize_scope_path(path))
        .filter(|path| !path.is_empty())
        .collect()
}

fn scope_paths_overlap(left: &[String], right: &[String]) -> bool {
    let left = normalize_scope_paths(left);
    let right = normalize_scope_paths(right);
    if left.is_empty() || right.is_empty() {
        return false;
    }
    left.iter().any(|lhs| {
        right.iter().any(|rhs| {
            lhs == rhs
                || lhs.starts_with(&format!("{rhs}/"))
                || rhs.starts_with(&format!("{lhs}/"))
        })
    })
}

fn derive_mission_scheduler_summary(
    view: &MissionStateView,
    now_unix_seconds: u64,
) -> MissionSchedulerSummary {
    let latest_session_by_objective = view
        .sessions
        .iter()
        .rev()
        .filter_map(|session| {
            session
                .objective_id
                .as_ref()
                .map(|objective_id| (objective_id.clone(), session))
        })
        .fold(HashMap::new(), |mut acc, (objective_id, session)| {
            acc.entry(objective_id).or_insert(session);
            acc
        });

    let mut ready_count = 0usize;
    let mut blocked_count = 0usize;
    let mut active_count = 0usize;
    let mut stale_session_count = 0usize;
    let mut next_objective_id = None;
    let mut by_objective = HashMap::new();
    let mut reserved_scopes: Vec<Vec<String>> = Vec::new();

    for objective in &view.objectives {
        let session = latest_session_by_objective.get(&objective.id).copied();
        let runtime = session.map(|entry| derive_agent_session_runtime_slice_at(entry, now_unix_seconds));
        let normalized_scope = normalize_scope_paths(&objective.meta.scope_paths);

        let entry = match objective.status {
            WorkUnitStatus::Done => ObjectiveSchedulerEntry {
                state: "done".to_string(),
                reason: None,
            },
            WorkUnitStatus::Failed => ObjectiveSchedulerEntry {
                state: "failed".to_string(),
                reason: None,
            },
            WorkUnitStatus::Claimed => {
                let runtime_status = runtime
                    .as_ref()
                    .map(|slice| slice.runtime_status.as_str())
                    .unwrap_or("active");
                if runtime_status == "stuck" {
                    stale_session_count += 1;
                    blocked_count += 1;
                    ObjectiveSchedulerEntry {
                        state: "blocked_stale".to_string(),
                        reason: Some("stale claimed session blocks objective".to_string()),
                    }
                } else {
                    active_count += 1;
                    if !normalized_scope.is_empty() {
                        reserved_scopes.push(normalized_scope.clone());
                    }
                    ObjectiveSchedulerEntry {
                        state: "active".to_string(),
                        reason: None,
                    }
                }
            }
            WorkUnitStatus::Pending => {
                let mut blocked_reason = None;
                if let Some(slice) = runtime.as_ref() {
                    if slice.runtime_status == "stuck" {
                        stale_session_count += 1;
                        blocked_reason = Some("stale session for objective needs sweep".to_string());
                    }
                }
                if blocked_reason.is_none()
                    && !normalized_scope.is_empty()
                    && reserved_scopes
                        .iter()
                        .any(|reserved| scope_paths_overlap(&normalized_scope, reserved))
                {
                    blocked_reason = Some(format!("waiting for scope {}", normalized_scope[0]));
                }

                if let Some(reason) = blocked_reason {
                    blocked_count += 1;
                    ObjectiveSchedulerEntry {
                        state: if reason.contains("stale") {
                            "blocked_stale".to_string()
                        } else {
                            "blocked_scope".to_string()
                        },
                        reason: Some(reason),
                    }
                } else {
                    ready_count += 1;
                    if next_objective_id.is_none() {
                        next_objective_id = Some(objective.id.clone());
                    }
                    if !normalized_scope.is_empty() {
                        reserved_scopes.push(normalized_scope.clone());
                    }
                    ObjectiveSchedulerEntry {
                        state: "ready".to_string(),
                        reason: None,
                    }
                }
            }
        };
        by_objective.insert(objective.id.clone(), entry);
    }

    MissionSchedulerSummary {
        ready_count,
        blocked_count,
        active_count,
        stale_session_count,
        next_objective_id,
        by_objective,
    }
}

fn latest_agent_submitted_verification_for_session(
    store: &WorkStore,
    mission_id: &str,
    objective_id: &str,
    session_id: &str,
) -> Result<Option<VerificationRecord>, String> {
    let verification = store.get_latest_verification_for_session(mission_id, session_id)?;
    Ok(verification.filter(|record| {
        record.objective_id.as_deref() == Some(objective_id)
            && record.session_id.as_deref() == Some(session_id)
            && record
                .details
                .get("submittedByAgent")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
    }))
}

#[allow(clippy::too_many_arguments)]
fn persist_agent_verdict(
    store: &mut WorkStore,
    mission_id: &str,
    session_id: &str,
    objective_id: Option<&str>,
    status: &str,
    summary: &str,
    details: Option<serde_json::Value>,
    artifacts: Vec<VerdictArtifact>,
    now: &str,
) -> Result<(i64, String), String> {
    let session = store
        .get_agent_session_runtime(session_id)?
        .ok_or_else(|| format!("agent session not found: {session_id}"))?;
    let objective_id = objective_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| session.objective_id.clone())
        .ok_or_else(|| "agent verdict missing objective id".to_string())?;
    let verification_details = match details.unwrap_or_else(|| serde_json::json!({})) {
        serde_json::Value::Object(mut map) => {
            map.entry("sessionId".to_string())
                .or_insert_with(|| serde_json::Value::String(session_id.to_string()));
            map.entry("profileId".to_string())
                .or_insert_with(|| serde_json::Value::String(session.profile_id.clone()));
            map.entry("workspacePath".to_string())
                .or_insert_with(|| serde_json::Value::String(session.workspace_path.clone()));
            map.entry("touchedPaths".to_string()).or_insert_with(|| {
                serde_json::Value::Array(
                    session
                        .touched_paths
                        .iter()
                        .cloned()
                        .map(serde_json::Value::String)
                        .collect(),
                )
            });
            serde_json::Value::Object(map)
        }
        other => other,
    };
    let verification_id = store.insert_verification(&NewVerificationRecord {
        mission_id: mission_id.to_string(),
        objective_id: Some(objective_id.clone()),
        session_id: Some(session_id.to_string()),
        status: status.to_string(),
        summary: summary.to_string(),
        details: verification_details,
    })?;
    store.submit_verdict(UnitVerdict {
        unit_id: objective_id.clone(),
        task_id: mission_id.to_string(),
        status: if status == "passed" || status == "done" {
            "done".to_string()
        } else {
            "failed".to_string()
        },
        summary: summary.to_string(),
        submitted_at: now.to_string(),
        artifacts,
    })?;
    let session_state = if status == "passed" || status == "done" {
        "completed"
    } else if status == "failed" {
        "failed"
    } else {
        status
    };
    store.update_agent_session_status(
        session_id,
        session_state,
        now,
        if session_state == "failed" {
            Some(summary.to_string())
        } else {
            None
        },
    )?;
    Ok((verification_id, objective_id))
}

fn build_mission_record(
    repo: &Path,
    input: &MissionInput,
    last_revision: &str,
) -> Result<MissionRecord, String> {
    let now = unix_now_label();
    let mission_id = normalize_mission_id(input)?;
    let instruction_pack = build_instruction_pack(repo);
    Ok(MissionRecord {
        id: mission_id,
        repo_path: repo.display().to_string(),
        worktree_path: repo.display().to_string(),
        graph_snapshot: last_revision.to_string(),
        title: input.title.trim().to_string(),
        brief: input.brief.trim().to_string(),
        validation_command: input.validation_command.trim().to_string(),
        planner_notes: input
            .planner_notes
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        planner_profile_id: input
            .planner_profile_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        planner_prompt_override: input
            .planner_prompt_override
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        issue_key: input
            .issue_key
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        issue_url: input
            .issue_url
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        instruction_pack_hash: instruction_pack_hash(&instruction_pack),
        state: "draft".to_string(),
        created_at: now.clone(),
        updated_at: now,
    })
}

fn agent_profile_has_capability(profile: &AgentProfile, capability: &str) -> bool {
    let wanted = capability.trim().to_ascii_lowercase();
    if wanted.is_empty() {
        return false;
    }
    profile
        .capabilities
        .iter()
        .any(|entry| entry.trim().to_ascii_lowercase() == wanted)
}

fn is_nim_http_profile(profile: &AgentProfile) -> bool {
    let provider = profile.provider.trim().to_ascii_lowercase();
    (profile.transport_kind.trim() == "http"
        && (provider == "nim_http" || provider == "nim" || provider == "nvidia_nim"))
        || provider == "opencode_nim"
        || agent_harness::is_opencode_cli_provider(&profile.provider)
        || agent_harness::is_pi_cli_provider(&profile.provider)
}

fn resolve_mission_planner_profile(
    store: &WorkStore,
    requested_profile_id: Option<&str>,
) -> Result<Option<AgentProfile>, String> {
    if let Some(profile_id) = requested_profile_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let profile = store
            .get_agent_profile(profile_id)?
            .ok_or_else(|| format!("Planner profile not found: {profile_id}"))?;
        if !agent_profile_has_capability(&profile, "can_plan") {
            return Err(format!(
                "Planner profile '{profile_id}' is missing can_plan capability."
            ));
        }
        if !is_nim_http_profile(&profile) {
            return Err(format!(
                "Planner profile '{profile_id}' is not backed by NIM-compatible planning."
            ));
        }
        return Ok(Some(profile));
    }
    let profiles = store.list_agent_profiles()?;
    Ok(profiles
        .iter()
        .find(|profile| profile.active && agent_profile_has_capability(profile, "can_plan"))
        .filter(|profile| is_nim_http_profile(profile))
        .cloned()
        .or_else(|| {
            profiles
                .into_iter()
                .find(|profile| {
                    is_nim_http_profile(profile) && agent_profile_has_capability(profile, "can_plan")
                })
        }))
}

fn profile_env_lookup(profile: &AgentProfile, keys: &[&str]) -> Option<String> {
    let from_profile = profile
        .env
        .as_object()
        .and_then(|map| {
            keys.iter().find_map(|key| {
                map.get(*key)
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| value.to_string())
            })
        });
    if from_profile.is_some() {
        return from_profile;
    }
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn nim_profile_base_url(profile: &AgentProfile) -> String {
    profile_env_lookup(
        profile,
        &[
            "NVIDIA_NIM_BASE_URL",
            "NVIDIA_BASE_URL",
            "SLOPOSCOPE_NVIDIA_NIM_BASE_URL",
        ],
    )
    .unwrap_or_else(|| "https://integrate.api.nvidia.com/v1".to_string())
}

fn nim_profile_api_key(profile: &AgentProfile) -> Result<String, String> {
    profile_env_lookup(
        profile,
        &[
            "NVIDIA_API_KEY",
            "NGC_API_KEY",
            "NVIDIA_NIM_API_KEY",
            "SLOPOSCOPE_NVIDIA_NIM_API_KEY",
        ],
    )
    .ok_or_else(|| {
        format!(
            "NIM planner profile '{}' is missing NVIDIA API key. Set NVIDIA_API_KEY, NGC_API_KEY, or NVIDIA_NIM_API_KEY in Env JSON or shell env.",
            profile.id
        )
    })
}

fn is_opencode_cli_profile(profile: &AgentProfile) -> bool {
    agent_harness::is_opencode_cli_provider(&profile.provider)
}

fn is_pi_cli_profile(profile: &AgentProfile) -> bool {
    agent_harness::is_pi_cli_provider(&profile.provider)
}

fn opencode_profile_model(profile: &AgentProfile) -> String {
    profile
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| {
            profile_env_lookup(
                profile,
                &["SLOPOSCOPE_NVIDIA_NIM_MODEL", "NVIDIA_NIM_MODEL"],
            )
        })
        .unwrap_or_else(|| "meta/llama3-8b-instruct".to_string())
}

fn opencode_model_ref(profile: &AgentProfile) -> String {
    format!("sloposcope_nim/{}", opencode_profile_model(profile))
}

fn profile_launch_env_json(
    profile: &AgentProfile,
    bundle: &agent_harness::AgentHarnessBundle,
) -> Result<String, String> {
    let mut env_map = profile
        .env
        .as_object()
        .cloned()
        .unwrap_or_default();
    if is_opencode_cli_profile(profile) {
        env_map.insert(
            "OPENCODE_CONFIG".to_string(),
            serde_json::Value::String(bundle.opencode_config_path.clone()),
        );
    }
    if is_pi_cli_profile(profile) {
        env_map.insert(
            "PI_CODING_AGENT_DIR".to_string(),
            serde_json::Value::String(bundle.pi_config_dir.clone()),
        );
        env_map.insert(
            "NVIDIA_API_KEY".to_string(),
            serde_json::Value::String(nim_profile_api_key(profile)?),
        );
    }
    serde_json::to_string(&serde_json::Value::Object(env_map))
        .map_err(|err| format!("serialize profile env: {err}"))
}

fn profile_launch_args_json(
    profile: &AgentProfile,
    bundle: &agent_harness::AgentHarnessBundle,
) -> Result<String, String> {
    let mut args = profile.args.clone();
    if is_opencode_cli_profile(profile) {
        let has_run = args.first().map(|value| value == "run").unwrap_or(false);
        if !has_run {
            args.insert(0, "run".to_string());
        }
        let has_model = args.iter().any(|value| value == "--model" || value == "-m");
        if !has_model {
            args.push("--model".to_string());
            args.push(opencode_model_ref(profile));
        }
        let prompt = std::fs::read_to_string(&bundle.prompt_path)
            .map_err(|err| format!("read {}: {err}", bundle.prompt_path))?;
        args.push(prompt);
    }
    if is_pi_cli_profile(profile) {
        let has_provider = args.iter().any(|value| value == "--provider");
        if !has_provider {
            args.push("--provider".to_string());
            args.push("sloposcope_nim".to_string());
        }
        let has_model = args.iter().any(|value| value == "--model" || value == "-m");
        if !has_model {
            args.push("--model".to_string());
            args.push(opencode_profile_model(profile));
        }
        let has_prompt = args.iter().any(|value| value == "-p" || value == "--prompt");
        if !has_prompt {
            let prompt = std::fs::read_to_string(&bundle.prompt_path)
                .map_err(|err| format!("read {}: {err}", bundle.prompt_path))?;
            args.push("-p".to_string());
            args.push(prompt);
        }
        let has_no_session = args.iter().any(|value| value == "--no-session");
        if !has_no_session {
            args.push("--no-session".to_string());
        }
    }
    serde_json::to_string(&args).map_err(|err| format!("serialize profile args: {err}"))
}

fn extract_json_object(raw: &str) -> Result<&str, String> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Ok(trimmed);
    }
    let start = trimmed
        .find('{')
        .ok_or_else(|| "planner response missing JSON object start".to_string())?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| "planner response missing JSON object end".to_string())?;
    Ok(&trimmed[start..=end])
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlannerObjectivesPayload {
    objectives: Vec<PlannerObjectiveDraft>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlannerObjectiveDraft {
    title: String,
    prompt: String,
    #[serde(default)]
    #[serde(alias = "scope_paths")]
    scope_paths: Vec<String>,
    #[serde(default)]
    #[serde(alias = "done_criteria")]
    done_criteria: Vec<String>,
    #[serde(default)]
    #[serde(alias = "test_intent")]
    test_intent: Option<String>,
}

fn parse_planned_objectives(
    raw: &str,
    validation_command: &str,
) -> Result<Vec<NewObjective>, String> {
    let json = extract_json_object(raw)?;
    let payload: PlannerObjectivesPayload = serde_json::from_str(json)
        .map_err(|err| format!("planner response json parse failed: {err}"))?;
    if payload.objectives.is_empty() {
        return Err("planner returned zero objectives".to_string());
    }
    let expected_validation = if validation_command.trim().is_empty() {
        Vec::new()
    } else {
        vec![validation_command.trim().to_string()]
    };
    Ok(payload
        .objectives
        .into_iter()
        .enumerate()
        .map(|(index, draft)| NewObjective {
            id: format!("objective-{}", index + 1),
            title: if draft.title.trim().is_empty() {
                format!("Mission slice {}", index + 1)
            } else {
                draft.title.trim().to_string()
            },
            prompt: if draft.prompt.trim().is_empty() {
                format!("Complete mission slice {}.", index + 1)
            } else {
                draft.prompt.trim().to_string()
            },
            scope_paths: draft
                .scope_paths
                .into_iter()
                .map(|path| normalize_scope_path(&path))
                .filter(|path| !path.is_empty())
                .collect(),
            expected_validation: expected_validation.clone(),
            done_criteria: if draft.done_criteria.is_empty() {
                vec![
                    "Relevant behavior slice is complete.".to_string(),
                    "Validation command passes or reports expected failure.".to_string(),
                ]
            } else {
                draft.done_criteria
            },
            test_intent: draft
                .test_intent
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        })
        .collect())
}

fn build_nim_planner_prompt(repo: &Path, input: &MissionInput) -> String {
    let agents = render_instruction_pack(&build_instruction_pack(repo));
    let planner_notes = input
        .planner_notes
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("(none)");
    let planner_prompt_override = input
        .planner_prompt_override
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("(none)");
    format!(
        "You are mission planner for a coding system.\n\
Return exactly one JSON object. No markdown. No prose.\n\
Schema:\n\
{{\"objectives\":[{{\"title\":\"...\",\"prompt\":\"...\",\"scopePaths\":[\"src/...\"],\"doneCriteria\":[\"...\"],\"testIntent\":\"...\"}}]}}\n\
Rules from AGENTS.md:\n{agents}\n\
Mission title: {title}\n\
Mission brief:\n{brief}\n\
Planner notes:\n{planner_notes}\n\
Planner prompt override:\n{planner_prompt_override}\n\
Validation command: {validation}\n\
Constraints:\n\
- Split mission into smallest useful objectives.\n\
- Keep objective count between 1 and 6.\n\
- Prefer explicit scopePaths when brief implies location.\n\
- Use BDD/TDD language in doneCriteria.\n\
- Keep prompt imperative and concrete.\n",
        agents = agents,
        title = input.title.trim(),
        brief = input.brief.trim(),
        planner_notes = planner_notes,
        planner_prompt_override = planner_prompt_override,
        validation = input.validation_command.trim(),
    )
}

fn plan_mission_with_nim_profile(
    repo: &Path,
    input: &MissionInput,
    profile: &AgentProfile,
) -> Result<Vec<NewObjective>, String> {
    let api_key = nim_profile_api_key(profile)?;
    let base_url = nim_profile_base_url(profile);
    let default_model = profile
        .model
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "meta/llama3-8b-instruct".to_string());
    let prompt = build_nim_planner_prompt(repo, input);
    let (_config, output_text) = conductor::run_openai_compatible_chat_completion(
        "NVIDIA NIM",
        &api_key,
        &base_url,
        &default_model,
        &prompt,
        None,
        60,
    )?;
    parse_planned_objectives(&output_text, &input.validation_command)
}

fn update_watchdog_from_mission(
    runtime: &mut watchdog::WatchdogRuntimeState,
    mission: &MissionRecord,
) {
    runtime.active_issue = Some(watchdog_types::WatchdogIssueRef {
        id: mission.id.clone(),
        title: mission.title.clone(),
        identifier: mission.issue_key.clone(),
        url: mission.issue_url.clone(),
    });
    runtime.active_issue_context = Some(watchdog_types::WatchdogIssueContext {
        issue: runtime.active_issue.clone().expect("active issue set"),
        summary: Some(mission.brief.clone()),
        acceptance_criteria: Vec::new(),
        non_goals: Vec::new(),
        labels: vec!["mission".to_string()],
        priority: None,
    });
}

fn summarize_mission(view: &MissionStateView) -> String {
    let Some(mission) = &view.mission else {
        return "Mission not planned yet.".to_string();
    };
    format!(
        "{} objectives, {} sessions. {}",
        view.summary.total_objectives,
        view.sessions.len(),
        mission.brief
    )
}

fn evaluate_patch_gate_for_objective(
    repo: &Path,
    mission: &MissionRecord,
    objective: &work_store::ObjectiveRecord,
    session: &AgentSessionRuntimeRecord,
    verification: &VerificationRecord,
) -> PatchGateRecord {
    let now = unix_now_label();
    let details = &verification.details;
    let patch_text = details
        .get("patch")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let validation_success = details
        .get("validation")
        .and_then(|entry| entry.get("success"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let mut reasons = Vec::new();
    if patch_text.trim().is_empty() {
        reasons.push("patch missing".to_string());
    }
    if mission.validation_command.trim().is_empty() {
        reasons.push("mission validation command missing".to_string());
    }
    if !validation_success {
        reasons.push("validation failed".to_string());
    }

    let out_of_scope = session
        .touched_paths
        .iter()
        .filter(|path| !touched_path_in_scope(path, &objective.meta.scope_paths))
        .cloned()
        .collect::<Vec<_>>();
    if !out_of_scope.is_empty() {
        reasons.push(format!("scope spill: {}", out_of_scope.join(", ")));
    }

    let watchdog_status = {
        let (config, _) = watchdog_config::load_repo_config(repo);
        let mut runtime = watchdog::WatchdogRuntimeState::default();
        update_watchdog_from_mission(&mut runtime, mission);
        let snapshot = watchdog::build_snapshot_for_repo(
            repo,
            &mission.graph_snapshot,
            &mut runtime,
            config.max_files_in_snapshot,
            config.max_diff_lines_per_file,
        );
        let verdict = watchdog::evaluate_snapshot(&snapshot, &config);
        Some(format!("{:?}", verdict.status).to_ascii_lowercase())
    };

    if matches!(watchdog_status.as_deref(), Some("blocked")) {
        reasons.push("watchdog blocked current map".to_string());
    }
    if matches!(watchdog_status.as_deref(), Some("drift")) {
        reasons.push("watchdog detected drift".to_string());
    }

    let status = if reasons.iter().any(|reason| {
        reason == "patch missing"
            || reason == "mission validation command missing"
            || reason == "validation failed"
            || reason == "watchdog blocked current map"
    }) {
        "blocked"
    } else if reasons.is_empty() {
        "ready"
    } else {
        "needs_review"
    };

    PatchGateRecord {
        mission_id: mission.id.clone(),
        objective_id: objective.id.clone(),
        session_id: Some(session.id.clone()),
        status: status.to_string(),
        reasons,
        watchdog_status,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn mission_patch_decision_status(
    decision: Option<&PatchDecisionRecord>,
    gate: Option<&PatchGateRecord>,
) -> String {
    if let Some(decision) = decision {
        return decision.status.clone();
    }
    if let Some(gate) = gate {
        if gate.status == "ready" {
            return "accepted".to_string();
        }
    }
    "pending_review".to_string()
}

fn review_state_from_decision(decision: Option<&PatchDecisionRecord>) -> &'static str {
    match decision.map(|entry| entry.status.as_str()) {
        Some("accepted") | Some("applied") | Some("apply_failed") => "accepted",
        Some("rejected") => "rejected",
        _ => "pending",
    }
}

fn apply_state_from_decision(decision: Option<&PatchDecisionRecord>) -> &'static str {
    match decision.map(|entry| entry.status.as_str()) {
        Some("applied") => "applied",
        Some("apply_failed") => "failed",
        Some("rejected") => "blocked",
        _ => "not_applied",
    }
}

fn gate_state_from_record(gate: Option<&PatchGateRecord>) -> &'static str {
    match gate.map(|entry| entry.status.as_str()) {
        Some("ready") => "ready",
        Some("blocked") => "blocked",
        Some("needs_review") => "needs_review",
        _ => "open",
    }
}

#[derive(Debug, Clone)]
struct CodeWorkerExecution {
    profile_id: String,
    worker_kind: String,
    workspace_path: String,
    relative_path: String,
    touched_paths: Vec<String>,
    patch_text: String,
    patch_artifact_path: String,
    validation: conductor::ValidationRunResult,
    base_revision: String,
    diagnostics: Option<serde_json::Value>,
}

fn execute_internal_code_worker(
    repo: &Path,
    mission: &MissionRecord,
    objective: &work_store::ObjectiveRecord,
    session_id: &str,
) -> Result<CodeWorkerExecution, String> {
    let write_spec = deterministic_code_write_spec(mission, objective)?;
    let base_revision = current_repo_revision(repo);
    let result = conductor::run_deterministic_code_worker(
        repo,
        &mission.id,
        &objective.id,
        session_id,
        &write_spec,
    )?;
    let patch_artifact_path = write_patch_artifact(
        &result.workspace.workspace_path,
        &objective.id,
        &result.patch,
    )?;
    Ok(CodeWorkerExecution {
        profile_id: "local-code-worker".to_string(),
        worker_kind: "code_worker".to_string(),
        workspace_path: result.workspace.workspace_path,
        relative_path: result.relative_path,
        touched_paths: result.touched_paths,
        patch_text: result.patch,
        patch_artifact_path,
        validation: result.validation,
        base_revision,
        diagnostics: None,
    })
}

fn execute_profiled_code_worker(
    repo: &Path,
    mission: &MissionRecord,
    objective: &work_store::ObjectiveRecord,
    session_id: &str,
    profile: &AgentProfile,
) -> Result<CodeWorkerExecution, String> {
    if profile.command.trim().is_empty() {
        return Err(format!("agent profile '{}' command is empty", profile.id));
    }
    let base_revision = current_repo_revision(repo);
    let workspace = conductor::ensure_isolated_git_worktree(
        repo,
        &format!("{}-{}-{}", mission.id, objective.id, session_id),
    )?;
    let workspace_path = PathBuf::from(&workspace.workspace_path);
    let harness_bundle = agent_harness::prepare_agent_harness_bundle(
        repo,
        mission,
        objective,
        session_id,
        profile,
        &workspace.workspace_path,
    )?;
    let scope_json =
        serde_json::to_string(&objective.meta.scope_paths).unwrap_or_else(|_| "[]".to_string());
    let validation_command = objective_validation_command(mission, objective);
    let profile_args_json = profile_launch_args_json(profile, &harness_bundle)?;
    let profile_env_json = profile_launch_env_json(profile, &harness_bundle)?;
    let profile_capabilities_json =
        serde_json::to_string(&profile.capabilities).unwrap_or_else(|_| "[]".to_string());
    let mcp_command_json =
        serde_json::to_string(&harness_bundle.mcp_command).unwrap_or_else(|_| "[]".to_string());
    let python_bin = std::env::var("SLOPOSCOPE_PYTHON_BIN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "python3".to_string());
    let mut command = Command::new(&python_bin);
    command.arg(&harness_bundle.harness_script_path);
    command
        .env("SLOPOSCOPE_REPO_PATH", repo.display().to_string())
        .env("SLOPOSCOPE_MISSION_ID", &mission.id)
        .env("SLOPOSCOPE_OBJECTIVE_ID", &objective.id)
        .env("SLOPOSCOPE_SESSION_ID", session_id)
        .env("SLOPOSCOPE_PROFILE_ID", &profile.id)
        .env("SLOPOSCOPE_PROFILE_LABEL", &profile.label)
        .env("SLOPOSCOPE_PROFILE_PROVIDER", &profile.provider)
        .env(
            "SLOPOSCOPE_PROFILE_MODEL",
            profile.model.clone().unwrap_or_default(),
        )
        .env("SLOPOSCOPE_PROFILE_TRANSPORT_KIND", &profile.transport_kind)
        .env(
            "SLOPOSCOPE_PROFILE_CAPABILITIES_JSON",
            profile_capabilities_json,
        )
        .env("SLOPOSCOPE_WORKTREE_PATH", &workspace.workspace_path)
        .env("SLOPOSCOPE_OBJECTIVE_PROMPT", &objective.prompt)
        .env("SLOPOSCOPE_VALIDATION_COMMAND", &validation_command)
        .env("SLOPOSCOPE_SCOPE_PATHS_JSON", scope_json)
        .env("SLOPOSCOPE_TARGET_COMMAND", profile.command.trim())
        .env("SLOPOSCOPE_TARGET_ARGS_JSON", profile_args_json)
        .env("SLOPOSCOPE_TARGET_ENV_JSON", profile_env_json)
        .env("SLOPOSCOPE_AGENT_BUNDLE_DIR", &harness_bundle.bundle_dir)
        .env(
            "SLOPOSCOPE_AGENT_BASE_INSTRUCTIONS_PATH",
            &harness_bundle.instruction_pack_path,
        )
        .env(
            "SLOPOSCOPE_AGENT_MISSION_FACTS_PATH",
            &harness_bundle.mission_facts_path,
        )
        .env(
            "SLOPOSCOPE_AGENT_OBJECTIVE_CONTRACT_PATH",
            &harness_bundle.objective_contract_path,
        )
        .env(
            "SLOPOSCOPE_AGENT_SESSION_CONTEXT_PATH",
            &harness_bundle.session_context_path,
        )
        .env(
            "SLOPOSCOPE_AGENT_CONTEXT_PATH",
            &harness_bundle.context_snapshot_path,
        )
        .env("SLOPOSCOPE_AGENT_PROMPT_PATH", &harness_bundle.prompt_path)
        .env(
            "SLOPOSCOPE_PROMPT_TO_STDIN",
            if agent_harness::prompt_to_stdin_for_provider(&profile.provider) {
                "1"
            } else {
                "0"
            },
        )
        .env(
            "SLOPOSCOPE_ACTIVE_MCP_CONFIG_PATH",
            &harness_bundle.mcp_config_path,
        )
        .env("SLOPOSCOPE_ACTIVE_MCP_COMMAND_JSON", mcp_command_json)
        .env(
            "SLOPOSCOPE_AGENT_HARNESS_DIAGNOSTICS_PATH",
            &harness_bundle.diagnostics_path,
        )
        .current_dir(&workspace_path);
    let output = command
        .output()
        .map_err(|err| format!("failed to run agent profile '{}': {err}", profile.id))?;
    let touched_paths = capture_worktree_touched_paths(&workspace_path)?;
    let patch_text = capture_worktree_patch(&workspace_path)?;
    let patch_artifact_path =
        write_patch_artifact(&workspace.workspace_path, &objective.id, &patch_text)?;
    let relative_path = primary_path_from_touched_paths(&touched_paths);
    let harness_diagnostics = std::fs::read_to_string(&harness_bundle.diagnostics_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
    let harness_reported_validation = harness_diagnostics.is_some();
    let mut diagnostics = harness_diagnostics.unwrap_or_else(|| serde_json::json!({}));
    if let Some(map) = diagnostics.as_object_mut() {
        map.insert(
            "command".to_string(),
            serde_json::Value::String(profile.command.clone()),
        );
        map.insert(
            "args".to_string(),
            serde_json::to_value(&profile.args).unwrap_or_else(|_| serde_json::json!([])),
        );
        map.insert(
            "exitCode".to_string(),
            serde_json::json!(output.status.code()),
        );
        map.insert(
            "stdout".to_string(),
            serde_json::Value::String(String::from_utf8_lossy(&output.stdout).to_string()),
        );
        map.insert(
            "stderr".to_string(),
            serde_json::Value::String(String::from_utf8_lossy(&output.stderr).to_string()),
        );
        map.insert(
            "bundleDir".to_string(),
            serde_json::Value::String(harness_bundle.bundle_dir.clone()),
        );
        map.insert(
            "promptPath".to_string(),
            serde_json::Value::String(harness_bundle.prompt_path.clone()),
        );
        map.insert(
            "mcpConfigPath".to_string(),
            serde_json::Value::String(harness_bundle.mcp_config_path.clone()),
        );
        map.insert(
            "opencodeConfigPath".to_string(),
            serde_json::Value::String(harness_bundle.opencode_config_path.clone()),
        );
        map.insert(
            "piConfigDir".to_string(),
            serde_json::Value::String(harness_bundle.pi_config_dir.clone()),
        );
        map.insert(
            "piConfigPath".to_string(),
            serde_json::Value::String(harness_bundle.pi_config_path.clone()),
        );
        map.insert(
            "piModelsPath".to_string(),
            serde_json::Value::String(harness_bundle.pi_models_path.clone()),
        );
        map.insert(
            "contextPath".to_string(),
            serde_json::Value::String(harness_bundle.context_snapshot_path.clone()),
        );
    }
    let validation = if harness_reported_validation {
        validation_from_diagnostics(Some(&diagnostics)).unwrap_or(conductor::ValidationRunResult {
            command: validation_command.clone(),
            success: false,
            exit_code: None,
            stdout: String::new(),
            stderr: "agent harness omitted validation result".to_string(),
        })
    } else {
        run_shell_command(&workspace_path, &validation_command)?
    };
    Ok(CodeWorkerExecution {
        profile_id: profile.id.clone(),
        worker_kind: "cli_worker".to_string(),
        workspace_path: workspace.workspace_path,
        relative_path,
        touched_paths,
        patch_text,
        patch_artifact_path,
        validation,
        base_revision,
        diagnostics: Some(diagnostics),
    })
}

fn mission_view_to_frontend(view: MissionStateView) -> serde_json::Value {
    mission_view_to_frontend_at(view, unix_now_seconds())
}

fn mission_view_to_frontend_at(
    view: MissionStateView,
    now_unix_seconds: u64,
) -> serde_json::Value {
    let objective_by_id = view
        .objectives
        .iter()
        .map(|objective| (objective.id.clone(), objective))
        .collect::<HashMap<_, _>>();
    let profile_by_id = view
        .profiles
        .iter()
        .map(|profile| (profile.id.clone(), profile))
        .collect::<HashMap<_, _>>();
    let patch_decision_by_objective = view
        .patch_decisions
        .iter()
        .map(|decision| (decision.objective_id.clone(), decision))
        .collect::<HashMap<_, _>>();
    let patch_gate_by_objective = view
        .patch_gates
        .iter()
        .map(|gate| (gate.objective_id.clone(), gate))
        .collect::<HashMap<_, _>>();
    let summary_text = summarize_mission(&view);
    let status = view
        .mission
        .as_ref()
        .map(|mission| mission.state.clone())
        .unwrap_or_else(|| "idle".to_string());
    let last_run_at = view
        .events
        .iter()
        .rev()
        .find(|event| event.event_type == "completed" || event.event_type == "progress")
        .map(|event| event.created_at.clone());
    let last_planned_event = view
        .events
        .iter()
        .rev()
        .find(|event| event.event_type == "planned");
    let last_planned_at = last_planned_event
        .map(|event| event.created_at.clone())
        .or_else(|| {
            view.mission
                .as_ref()
                .map(|mission| mission.updated_at.clone())
        });
    let planner_profile_id = view
        .mission
        .as_ref()
        .and_then(|mission| mission.planner_profile_id.clone())
        .unwrap_or_default();
    let planner_prompt_override = view
        .mission
        .as_ref()
        .and_then(|mission| mission.planner_prompt_override.clone())
        .unwrap_or_default();
    let planner_profile_label = last_planned_event
        .and_then(|event| event.payload.get("planner"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            (!planner_profile_id.is_empty()).then(|| planner_profile_id.clone())
        })
        .unwrap_or_default();
    let planner_summary = last_planned_event
        .and_then(|event| event.payload.get("summary"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            if planner_profile_id.is_empty() {
                "viewer-first planner ready".to_string()
            } else {
                format!("Planner profile ready: {planner_profile_label}")
            }
        });
    let planner_status = if last_planned_event.is_some() {
        "planned"
    } else if !planner_profile_id.is_empty() {
        "ready"
    } else {
        "idle"
    };
    let planner_mode = if planner_profile_id.is_empty() {
        "viewer_first"
    } else {
        "profile"
    };
    let viewer_session = view.sessions.first();
    let viewer_decision = viewer_session
        .and_then(|session| session.objective_id.as_deref())
        .and_then(|objective_id| patch_decision_by_objective.get(objective_id))
        .copied();
    let viewer_gate = viewer_session
        .and_then(|session| session.objective_id.as_deref())
        .and_then(|objective_id| patch_gate_by_objective.get(objective_id))
        .copied();
    let scheduler = derive_mission_scheduler_summary(&view, now_unix_seconds);

    serde_json::json!({
        "missionId": view.mission.as_ref().map(|mission| mission.id.clone()).unwrap_or_default(),
        "title": view.mission.as_ref().map(|mission| mission.title.clone()).unwrap_or_default(),
        "brief": view.mission.as_ref().map(|mission| mission.brief.clone()).unwrap_or_default(),
        "plannerProfileId": planner_profile_id,
        "plannerProfileLabel": planner_profile_label,
        "plannerNotes": view.mission.as_ref().and_then(|mission| mission.planner_notes.clone()).unwrap_or_default(),
        "plannerPromptOverride": planner_prompt_override,
        "plannerSummary": planner_summary,
        "plannerStatus": planner_status,
        "plannerMode": planner_mode,
        "validationCommand": view.mission.as_ref().map(|mission| mission.validation_command.clone()).unwrap_or_default(),
        "issueKey": view.mission.as_ref().and_then(|mission| mission.issue_key.clone()).unwrap_or_default(),
        "issueUrl": view.mission.as_ref().and_then(|mission| mission.issue_url.clone()).unwrap_or_default(),
        "summary": summary_text,
        "status": status,
        "objectives": view.objectives.iter().map(|objective| {
            let decision = patch_decision_by_objective.get(&objective.id).copied();
            let gate = patch_gate_by_objective.get(&objective.id).copied();
            let schedule = scheduler.by_objective.get(&objective.id);
            serde_json::json!({
                "id": objective.id,
                "title": objective.title,
                "prompt": objective.prompt,
                "state": match objective.status {
                    WorkUnitStatus::Pending => "pending",
                    WorkUnitStatus::Claimed => "claimed",
                    WorkUnitStatus::Done => "done",
                    WorkUnitStatus::Failed => "failed",
                },
                "expectedValidation": objective.meta.expected_validation.join(" && "),
                "scope": { "paths": objective.meta.scope_paths },
                "doneCriteria": objective.meta.done_criteria,
                "testIntent": objective.meta.test_intent,
                "claimedBy": objective.claimed_by,
                "claimedAt": objective.claimed_at,
                "createdAt": objective.created_at,
                "decisionStatus": mission_patch_decision_status(decision, gate),
                "decisionReason": decision.and_then(|entry| entry.reason.clone()),
                "applyStatus": decision.map(|entry| entry.status.clone()),
                "applyError": decision.and_then(|entry| entry.apply_error.clone()),
                "appliedAt": decision.and_then(|entry| entry.applied_at.clone()),
                "gateStatus": gate.map(|entry| entry.status.clone()),
                "gateReasons": gate.map(|entry| entry.reasons.clone()).unwrap_or_default(),
                "schedulerState": schedule.map(|entry| entry.state.clone()).unwrap_or_else(|| "pending".to_string()),
                "schedulerReason": schedule.and_then(|entry| entry.reason.clone()),
            })
        }).collect::<Vec<_>>(),
        "sessions": view.sessions.iter().map(|session| {
            let runtime = derive_agent_session_runtime_slice_at(session, now_unix_seconds);
            let linked_verification = view
                .verifications
                .iter()
                .rev()
                .find(|verification| verification.session_id.as_deref() == Some(session.id.as_str()));
            let verification_details = linked_verification.map(|verification| &verification.details);
            let validation = verification_details.and_then(|details| details.get("validation"));
            let patch_summary = verification_details
                .and_then(|details| details.get("patchSummary"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    verification_details
                        .and_then(|details| details.get("relativePath"))
                        .and_then(serde_json::Value::as_str)
                        .map(|path| format!("Patch artifact for {path}"))
                });
            let objective_title = session
                .objective_id
                .as_ref()
                .and_then(|objective_id| objective_by_id.get(objective_id))
                .map(|objective| objective.title.clone());
            let linked_profile = profile_by_id.get(&session.profile_id).copied();
            let linked_decision = session
                .objective_id
                .as_ref()
                .and_then(|objective_id| patch_decision_by_objective.get(objective_id))
                .copied();
            let linked_gate = session
                .objective_id
                .as_ref()
                .and_then(|objective_id| patch_gate_by_objective.get(objective_id))
                .copied();
            serde_json::json!({
                "sessionId": session.id,
                "objectiveId": session.objective_id,
                "objectiveTitle": objective_title,
                "profileId": session.profile_id,
                "profileLabel": linked_profile.map(|profile| profile.label.clone()),
                "provider": linked_profile.map(|profile| profile.provider.clone()),
                "model": linked_profile.and_then(|profile| profile.model.clone()),
                "transportKind": linked_profile.map(|profile| profile.transport_kind.clone()),
                "assignmentKind": session.assignment_kind,
                "workerKind": verification_details
                    .and_then(|details| details.get("kind"))
                    .and_then(serde_json::Value::as_str),
                "state": session.state,
                "runtimeStatus": runtime.runtime_status,
                "heartbeatAgeSec": runtime.heartbeat_age_sec,
                "eventAgeSec": runtime.event_age_sec,
                "leaseExpired": runtime.lease_expired,
                "result": linked_verification.map(|verification| verification.status.clone()).unwrap_or_else(|| session.state.clone()),
                "workspacePath": session.workspace_path,
                "leaseExpiresAt": session.lease_expires_at,
                "lastHeartbeatAt": session.last_heartbeat_at,
                "lastEventAt": session.last_event_at,
                "lastError": session.last_error,
                "startedAt": session.created_at,
                "updatedAt": session.updated_at,
                "touchedPaths": session.touched_paths,
                "verificationId": linked_verification.map(|verification| format!("verification-{}", verification.id)),
                "command": verification_details
                    .and_then(|details| details.get("command"))
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| validation.and_then(|entry| entry.get("command")).and_then(serde_json::Value::as_str)),
                "patchSummary": patch_summary,
                "patchPath": verification_details
                    .and_then(|details| details.get("patchPath"))
                    .and_then(serde_json::Value::as_str),
                "patchArtifact": verification_details
                    .and_then(|details| details.get("patch"))
                    .and_then(serde_json::Value::as_str),
                "validationStatus": verification_details
                    .and_then(|details| details.get("validationStatus"))
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| validation
                        .and_then(|entry| entry.get("success"))
                        .and_then(serde_json::Value::as_bool)
                        .map(|ok| if ok { "passed" } else { "failed" })),
                "validation": validation,
                "decisionStatus": linked_decision.map(|entry| entry.status.clone()),
                "decisionReason": linked_decision.and_then(|entry| entry.reason.clone()),
                "applyStatus": linked_decision.map(|entry| entry.status.clone()),
                "applyError": linked_decision.and_then(|entry| entry.apply_error.clone()),
                "appliedAt": linked_decision.and_then(|entry| entry.applied_at.clone()),
                "gateStatus": linked_gate.map(|entry| entry.status.clone()),
                "gateReasons": linked_gate.map(|entry| entry.reasons.clone()).unwrap_or_default(),
                "reviewState": review_state_from_decision(linked_decision),
                "applyState": apply_state_from_decision(linked_decision),
                "gateState": gate_state_from_record(linked_gate),
                "schedulerState": session
                    .objective_id
                    .as_ref()
                    .and_then(|objective_id| scheduler.by_objective.get(objective_id))
                    .map(|entry| entry.state.clone()),
                "schedulerReason": session
                    .objective_id
                    .as_ref()
                    .and_then(|objective_id| scheduler.by_objective.get(objective_id))
                    .and_then(|entry| entry.reason.clone()),
            })
        }).collect::<Vec<_>>(),
        "verifications": view.verifications.iter().map(|verification| {
            let objective_title = verification
                .objective_id
                .as_ref()
                .and_then(|objective_id| objective_by_id.get(objective_id))
                .map(|objective| objective.title.clone());
            let patch_summary = verification
                .details
                .get("patchSummary")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    verification
                        .details
                        .get("relativePath")
                        .and_then(serde_json::Value::as_str)
                        .map(|path| format!("Patch artifact for {path}"))
                });
            serde_json::json!({
                "verificationId": format!("verification-{}", verification.id),
                "objectiveId": verification.objective_id,
                "objectiveTitle": objective_title,
                "sessionId": verification.session_id,
                "status": verification.status,
                "summary": verification.summary,
                "checkedAt": verification.created_at,
                "details": verification.details,
                "kind": verification.details.get("kind").and_then(serde_json::Value::as_str),
                "command": verification.details.get("command").and_then(serde_json::Value::as_str)
                    .or_else(|| verification.details.get("validation").and_then(|entry| entry.get("command")).and_then(serde_json::Value::as_str)),
                "profileId": verification.details.get("profileId").and_then(serde_json::Value::as_str),
                "workspacePath": verification.details.get("workspacePath").and_then(serde_json::Value::as_str),
                "patchPath": verification.details.get("patchPath").and_then(serde_json::Value::as_str),
                "patchSummary": patch_summary,
                "patchArtifact": verification.details.get("patch").and_then(serde_json::Value::as_str),
                "targetFile": verification.details.get("targetFile").and_then(serde_json::Value::as_str)
                    .or_else(|| verification.details.get("relativePath").and_then(serde_json::Value::as_str)),
                "validationStatus": verification.details.get("validationStatus").and_then(serde_json::Value::as_str)
                    .or_else(|| verification.details.get("validation").and_then(|entry| entry.get("success")).and_then(serde_json::Value::as_bool).map(|ok| if ok { "passed" } else { "failed" })),
                "validation": verification.details.get("validation"),
            })
        }).collect::<Vec<_>>(),
        "eventLedger": view.events.iter().map(|event| {
            let summary = event.payload.get("summary")
                .and_then(serde_json::Value::as_str)
                .or_else(|| event.payload.get("message").and_then(serde_json::Value::as_str))
                .map(|value| value.to_string())
                .unwrap_or_else(|| event.event_type.clone());
            serde_json::json!({
                "at": event.created_at,
                "kind": event.event_type,
                "summary": summary,
                "sessionId": event.session_id,
                "objectiveId": event.unit_id,
            })
        }).collect::<Vec<_>>(),
        "robotTargets": [],
        "lastSavedAt": view.mission.as_ref().map(|mission| mission.updated_at.clone()),
        "lastPlannedAt": last_planned_at,
        "lastRunAt": last_run_at,
        "lastCheckedAt": serde_json::Value::Null,
        "lastCheckResult": serde_json::Value::Null,
        "viewerProfileId": viewer_session.map(|session| session.profile_id.clone()).unwrap_or_default(),
        "viewerSessionId": viewer_session.map(|session| session.id.clone()).unwrap_or_default(),
        "reviewState": review_state_from_decision(viewer_decision),
        "applyState": apply_state_from_decision(viewer_decision),
        "gateState": gate_state_from_record(viewer_gate),
        "scheduler": {
            "readyCount": scheduler.ready_count,
            "blockedCount": scheduler.blocked_count,
            "activeCount": scheduler.active_count,
            "staleSessionCount": scheduler.stale_session_count,
            "nextObjectiveId": scheduler.next_objective_id,
        },
        "lastAction": view.events.last().map(|event| event.event_type.clone()).unwrap_or_default(),
        "lastActionAt": view.events.last().map(|event| event.created_at.clone()),
        "profiles": view.profiles.iter().map(|profile| {
            serde_json::json!({
                "id": profile.id,
                "label": profile.label,
                "provider": profile.provider,
                "model": profile.model,
                "transportKind": profile.transport_kind,
                "command": profile.command,
                "capabilities": profile.capabilities,
                "active": profile.active,
            })
        }).collect::<Vec<_>>(),
    })
}

#[derive(Debug, Clone)]
struct WorkflowProviderCheckError {
    provider: conductor::ConductorRunnerProvider,
    strict_mode: bool,
    message: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentProfileInput {
    id: Option<String>,
    label: String,
    provider: String,
    model: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<serde_json::Value>,
    capabilities: Option<Vec<String>>,
    transport_kind: Option<String>,
    active: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentLoginInput {
    mission_id: String,
    objective_id: Option<String>,
    session_id: Option<String>,
    profile_id: String,
    assignment_kind: Option<String>,
    workspace_path: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentActivityInput {
    mission_id: String,
    session_id: String,
    state: String,
    error: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentEventInput {
    mission_id: String,
    objective_id: Option<String>,
    session_id: String,
    event_type: String,
    payload: serde_json::Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentTouchedPathsInput {
    mission_id: String,
    session_id: String,
    paths: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentVerdictInput {
    mission_id: String,
    objective_id: Option<String>,
    session_id: String,
    status: String,
    summary: String,
    details: Option<serde_json::Value>,
    artifacts: Option<Vec<VerdictArtifact>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentLogoutInput {
    mission_id: String,
    session_id: String,
    state: Option<String>,
    error: Option<String>,
}

fn provider_label(provider: &conductor::ConductorRunnerProvider) -> &'static str {
    match provider {
        conductor::ConductorRunnerProvider::CodingAgentAppServer => "coding_agent_app_server",
        conductor::ConductorRunnerProvider::OpenAi => "openai",
        conductor::ConductorRunnerProvider::Nim => "nim",
    }
}

fn trim_for_evidence(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in input.chars().take(max_chars) {
        out.push(ch);
    }
    out
}

fn run_watchdog_check_via_workflow(
    repo: &Path,
    since: &str,
    runtime: &mut watchdog::WatchdogRuntimeState,
) -> Result<watchdog_types::WatchdogRunTrace, WorkflowProviderCheckError> {
    let workflow = conductor::load_workflow_contract(repo);
    let Some(contract) = workflow.contract else {
        return Ok(watchdog::run_manual_check(repo, since, runtime));
    };
    let provider = contract.front_matter.provider.clone();
    let strict_mode = contract.front_matter.provider_strict_mode;

    match provider {
        conductor::ConductorRunnerProvider::CodingAgentAppServer => {
            Ok(watchdog::run_manual_check(repo, since, runtime))
        }
        conductor::ConductorRunnerProvider::OpenAi => {
            let (config, _) = watchdog_config::load_repo_config(repo);
            let snapshot = watchdog::build_snapshot_for_repo(
                repo,
                since,
                runtime,
                config.max_files_in_snapshot,
                config.max_diff_lines_per_file,
            );
            let snapshot_json =
                serde_json::to_string(&snapshot).map_err(|err| WorkflowProviderCheckError {
                    provider: provider.clone(),
                    strict_mode,
                    message: format!("Failed to serialize watchdog snapshot: {err}"),
                })?;
            let prompt = conductor::build_openai_prompt(&contract.prompt_template, &snapshot_json)
                .map_err(|err| WorkflowProviderCheckError {
                    provider: provider.clone(),
                    strict_mode,
                    message: err,
                })?;
            let run = conductor::run_openai_watchdog_check(
                &prompt,
                contract.front_matter.provider_model.as_deref(),
                contract.front_matter.timeout_seconds,
            )
            .map_err(|err| WorkflowProviderCheckError {
                provider: provider.clone(),
                strict_mode,
                message: err,
            })?;
            Ok(watchdog::record_external_check(
                repo,
                runtime,
                snapshot,
                run.verdict,
            ))
        }
        conductor::ConductorRunnerProvider::Nim => {
            let (config, _) = watchdog_config::load_repo_config(repo);
            let snapshot = watchdog::build_snapshot_for_repo(
                repo,
                since,
                runtime,
                config.max_files_in_snapshot,
                config.max_diff_lines_per_file,
            );
            let snapshot_json =
                serde_json::to_string(&snapshot).map_err(|err| WorkflowProviderCheckError {
                    provider: provider.clone(),
                    strict_mode,
                    message: format!("Failed to serialize watchdog snapshot: {err}"),
                })?;
            let prompt = conductor::build_openai_prompt(&contract.prompt_template, &snapshot_json)
                .map_err(|err| WorkflowProviderCheckError {
                    provider: provider.clone(),
                    strict_mode,
                    message: err,
                })?;
            let run = conductor::run_nim_watchdog_check(
                &prompt,
                contract.front_matter.provider_model.as_deref(),
                contract.front_matter.timeout_seconds,
            )
            .map_err(|err| WorkflowProviderCheckError {
                provider: provider.clone(),
                strict_mode,
                message: err,
            })?;
            Ok(watchdog::record_external_check(
                repo,
                runtime,
                snapshot,
                run.verdict,
            ))
        }
    }
}

fn run_watchdog_strict_mode_blocked_check(
    repo: &Path,
    since: &str,
    runtime: &mut watchdog::WatchdogRuntimeState,
    provider: &conductor::ConductorRunnerProvider,
    reason: &str,
) -> watchdog_types::WatchdogRunTrace {
    let (config, _) = watchdog_config::load_repo_config(repo);
    let snapshot = watchdog::build_snapshot_for_repo(
        repo,
        since,
        runtime,
        config.max_files_in_snapshot,
        config.max_diff_lines_per_file,
    );
    let provider_name = provider_label(provider);
    let evidence_reason = trim_for_evidence(reason, 800);
    let verdict = watchdog_types::WatchdogVerdict {
        status: watchdog_types::WatchdogStatus::Blocked,
        confidence: 0.95,
        summary: format!(
            "Workflow provider '{}' failed in strict mode; deterministic fallback is disabled.",
            provider_name
        ),
        issues: vec![watchdog_types::WatchdogVerdictIssue {
            kind: watchdog_types::WatchdogIssueKind::UnclearMapping,
            severity: watchdog_types::WatchdogSeverity::Critical,
            title: "Workflow provider check failed".to_string(),
            why: "WORKFLOW.md has providerStrictMode=true, so watchdog fallback is disabled when provider execution fails.".to_string(),
            evidence: vec![
                format!("provider={provider_name}"),
                "providerStrictMode=true".to_string(),
                format!("error={evidence_reason}"),
            ],
            recommended_next_step:
                "Fix provider configuration/runtime error or set providerStrictMode=false in WORKFLOW.md."
                    .to_string(),
        }],
    };
    watchdog::record_external_check(repo, runtime, snapshot, verdict)
}

fn run_watchdog_check_with_fallback(
    repo: &Path,
    since: &str,
    runtime: &mut watchdog::WatchdogRuntimeState,
    conductor_runtime: Option<&mut conductor::ConductorRuntimeState>,
) -> watchdog_types::WatchdogRunTrace {
    let now_unix = unix_now_seconds();
    match run_watchdog_check_via_workflow(repo, since, runtime) {
        Ok(trace) => trace,
        Err(err) => {
            if let Some(runtime) = conductor_runtime {
                conductor::record_provider_failure(
                    runtime,
                    err.provider.clone(),
                    err.strict_mode,
                    &err.message,
                    now_unix,
                );
            }
            if err.strict_mode {
                if debug_enabled() {
                    println!(
                        "[BACKEND][watchdog] workflow-provider strict-mode failure provider={} error={}",
                        provider_label(&err.provider),
                        err.message
                    );
                }
                return run_watchdog_strict_mode_blocked_check(
                    repo,
                    since,
                    runtime,
                    &err.provider,
                    &err.message,
                );
            }
            if debug_enabled() {
                println!(
                    "[BACKEND][watchdog] workflow-provider check failed: {}. Falling back to deterministic evaluator.",
                    err.message
                );
            }
            watchdog::run_manual_check(repo, since, runtime)
        }
    }
}

fn output_tail(stdout: &[u8], stderr: &[u8], max_lines: usize, max_chars: usize) -> String {
    let mut lines: Vec<String> = String::from_utf8_lossy(stdout)
        .lines()
        .map(|line| line.to_string())
        .collect();
    lines.extend(
        String::from_utf8_lossy(stderr)
            .lines()
            .map(|line| line.to_string()),
    );
    if lines.len() > max_lines {
        let keep_from = lines.len() - max_lines;
        lines = lines.split_off(keep_from);
    }
    let tail = lines.join("\n");
    if tail.chars().count() <= max_chars {
        return tail;
    }
    let mut trimmed = String::new();
    for ch in tail.chars().take(max_chars) {
        trimmed.push(ch);
    }
    trimmed
}

fn resolve_single_edge_target(target: &str, all_node_ids: &[String]) -> Option<String> {
    if all_node_ids.iter().any(|id| id == target) {
        return Some(target.to_string());
    }

    let normalized = target
        .replace("::", "/")
        .replace("crate/", "")
        .trim_start_matches("./")
        .to_string();
    if let Some(found) = all_node_ids.iter().find(|id| {
        id.as_str() == normalized.as_str()
            || id.ends_with(&normalized)
            || id.replace("/", ".").contains(target)
    }) {
        return Some(found.clone());
    }

    if let Some(found) = all_node_ids.iter().find(|id| {
        let id_no_ext = id.split('.').next().unwrap_or(id);
        id_no_ext.contains(target) || target.contains(id_no_ext)
    }) {
        return Some(found.clone());
    }

    None
}

fn resolve_graph_edges_for_sources(g: &mut Graph, sources: &HashSet<String>) {
    let all_node_ids: Vec<String> = g.nodes.iter().map(|n| n.id.clone()).collect();

    for edge in &mut g.edges {
        if !sources.is_empty() && !sources.contains(&edge.source) {
            continue;
        }

        if let Some(found) = resolve_single_edge_target(&edge.target, &all_node_ids) {
            edge.target = found.clone();
        }
    }
}

fn resolve_graph_edges(g: &mut Graph) {
    let empty = HashSet::new();
    resolve_graph_edges_for_sources(g, &empty);
}

fn is_ignored_runtime_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    for segment in normalized.split('/') {
        match segment {
            ".git" | ".jj" | ".sloposcope" | "node_modules" | "target" | "_build" | "deps"
            | "dist" | ".svelte-kit" | ".output" => return true,
            _ => {}
        }
    }
    normalized.starts_with("src-tauri/gen/")
}

fn collect_repo_files(repo: &Path) -> Vec<String> {
    const MAX_FILES: usize = 20_000;
    let mut files = Vec::new();
    let mut builder = WalkBuilder::new(repo);
    builder.hidden(false);
    builder.git_ignore(true);
    builder.git_exclude(true);
    builder.git_global(true);

    for entry in builder.build().filter_map(Result::ok) {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(rel_path_buf) = entry.path().strip_prefix(repo) else {
            continue;
        };
        let rel_path = rel_path_buf.display().to_string().replace('\\', "/");
        if rel_path.is_empty() || is_ignored_runtime_path(&rel_path) {
            continue;
        }
        files.push(rel_path);
        if files.len() >= MAX_FILES {
            break;
        }
    }
    files.sort();
    files
}

fn fallback_file_node(rel_path: &str, line_count: usize, change_status: &str) -> Node {
    Node {
        id: rel_path.to_string(),
        label: rel_path.to_string(),
        kind: "file".into(),
        file: rel_path.to_string(),
        line_count,
        change_status: change_status.to_string(),
        functions: vec![],
    }
}

fn parse_graph_fragment(
    parsers: &[Box<dyn Parser>],
    repo: &Path,
    rel_path: &str,
    change_status: &str,
) -> (Vec<Node>, Vec<graph::Edge>) {
    let full_path = repo.join(rel_path);
    if !full_path.exists() || !full_path.is_file() {
        return (Vec::new(), Vec::new());
    }

    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let source = std::fs::read_to_string(&full_path).ok();
    let line_count = source
        .as_ref()
        .map(|content| content.lines().count())
        .unwrap_or(0);

    let (mut nodes, edges) = if let (Some(parser), Some(content)) = (
        parsers.iter().find(|p| p.extensions().contains(&ext)),
        source.as_deref(),
    ) {
        parser.parse_file(repo, rel_path, content)
    } else {
        (
            vec![fallback_file_node(rel_path, line_count, change_status)],
            Vec::new(),
        )
    };

    if nodes.is_empty() {
        nodes.push(fallback_file_node(rel_path, line_count, change_status));
    }

    for node in &mut nodes {
        node.change_status = change_status.to_string();
    }

    (nodes, edges)
}

fn build_canonical_repo_graph(parsers: &[Box<dyn Parser>], repo: &Path) -> Graph {
    let started = Instant::now();
    let files_to_process = collect_repo_files(repo);
    let mut graph = Graph::new();

    for rel_path in files_to_process {
        let (nodes, edges) = parse_graph_fragment(parsers, repo, &rel_path, "unchanged");
        graph.add_nodes(nodes);
        graph.add_edges(edges);
    }

    resolve_graph_edges(&mut graph);
    graph.finalize();

    if debug_enabled() {
        println!(
            "[BACKEND][graph-store] canonical build repo={} nodes={} edges={} took={}ms",
            repo.display(),
            graph.nodes.len(),
            graph.edges.len(),
            started.elapsed().as_millis()
        );
    }

    graph
}

fn graph_view_since_hint(request: &GraphViewRequest) -> Option<String> {
    match request.seed.as_ref() {
        Some(GraphViewSeed::Commit { revset }) => Some(revset.clone()),
        Some(GraphViewSeed::WorkingCopy) | None => Some("@".to_string()),
        Some(GraphViewSeed::NodeIds { .. }) | Some(GraphViewSeed::Paths { .. }) => {
            Some("@".to_string())
        }
    }
}

fn graph_view_request_from_legacy(
    since: Option<String>,
    include_neighbors: bool,
) -> GraphViewRequest {
    let trimmed_since = since
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("@");
    let depth = usize::from(include_neighbors);
    if trimmed_since == "@" {
        GraphViewRequest {
            mode: graph_view::GraphViewMode::Investigation,
            seed: Some(GraphViewSeed::WorkingCopy),
            depth,
            min_confidence: 0.0,
            aggregate: graph_view::GraphViewAggregate::File,
        }
    } else {
        GraphViewRequest {
            mode: graph_view::GraphViewMode::Investigation,
            seed: Some(GraphViewSeed::Commit {
                revset: trimmed_since.to_string(),
            }),
            depth,
            min_confidence: 0.0,
            aggregate: graph_view::GraphViewAggregate::File,
        }
    }
}

fn graph_view_changed_statuses(
    repo: &Path,
    request: &GraphViewRequest,
) -> (HashMap<String, String>, bool, bool) {
    let no_vcs_mode = matches!(vcs::detect_engine(repo), vcs::VCSEngine::None);
    if no_vcs_mode {
        return (HashMap::new(), true, false);
    }

    match request.seed.as_ref() {
        Some(GraphViewSeed::Commit { revset }) => {
            (vcs::get_changed_files(repo, revset), false, true)
        }
        _ => (vcs::get_changed_files(repo, "@"), false, false),
    }
}

fn rebuild_canonical_graph_store(parsers: &[Box<dyn Parser>], repo: &Path) -> Result<(), String> {
    let store = SqliteGraphStore::open_or_create(repo)?;
    let graph = build_canonical_repo_graph(parsers, repo);
    let snapshot = graph_store::snapshot_from_graph(
        repo,
        &graph,
        &vcs::get_current_revision(repo),
        graph::source_fingerprint(repo),
    );
    store.replace_all(&snapshot)?;
    rebuild_search_index(repo, &snapshot)
}

fn update_canonical_graph_for_file(
    parsers: &[Box<dyn Parser>],
    repo: &Path,
    rel_path: &str,
) -> Result<(), String> {
    let store = SqliteGraphStore::open_or_create(repo)?;
    let mut existing_snapshot = store.load_snapshot()?;
    store.remove_file(rel_path)?;

    let full_path = repo.join(rel_path);
    if !full_path.exists() || !full_path.is_file() {
        return Ok(());
    }

    let (nodes, edges) = parse_graph_fragment(parsers, repo, rel_path, "unchanged");
    let mut fragment = Graph::new();
    fragment.add_nodes(nodes);
    fragment.add_edges(edges);

    let mut known_ids = existing_snapshot
        .entities
        .iter()
        .filter(|entity| entity.file_path != rel_path)
        .map(|entity| entity.id.clone())
        .collect::<Vec<_>>();
    known_ids.extend(fragment.nodes.iter().map(|node| node.id.clone()));
    let known_target_ids = known_ids.iter().cloned().collect::<HashSet<_>>();

    for edge in &mut fragment.edges {
        if let Some(found) = resolve_single_edge_target(&edge.target, &known_ids) {
            edge.target = found;
        }
    }
    let fragment_node_ids = fragment
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    let mut seen_edges = HashSet::new();
    fragment.edges.retain(|edge| {
        if edge.source == edge.target {
            return false;
        }
        if !fragment_node_ids.contains(&edge.source) {
            return false;
        }
        if !fragment_node_ids.contains(&edge.target) && !known_target_ids.contains(&edge.target) {
            return false;
        }
        seen_edges.insert((edge.source.clone(), edge.target.clone(), edge.kind.clone()))
    });

    existing_snapshot.repo_meta.insert(
        "checkout_revision".to_string(),
        vcs::get_current_revision(repo),
    );
    existing_snapshot.repo_meta.insert(
        "build_fingerprint".to_string(),
        graph::source_fingerprint(repo).to_string(),
    );
    existing_snapshot
        .repo_meta
        .insert("built_at".to_string(), unix_now_label());

    let snapshot = graph_store::snapshot_from_graph(
        repo,
        &fragment,
        existing_snapshot
            .repo_meta
            .get("checkout_revision")
            .map(String::as_str)
            .unwrap_or("HEADLESS"),
        graph::source_fingerprint(repo),
    );
    store.upsert_file(
        &snapshot.entities,
        &snapshot.edges,
        Some(&existing_snapshot.repo_meta),
    )?;
    let latest_snapshot = store.load_snapshot()?;
    rebuild_search_index(repo, &latest_snapshot)
}

fn rebuild_search_index(
    repo: &Path,
    snapshot: &graph_store::CanonicalGraphSnapshot,
) -> Result<(), String> {
    let search = SearchService::open_or_create(repo)?;
    search.rebuild_from_snapshot(snapshot)
}

fn cluster_refresh_threshold() -> usize {
    4
}

fn load_clustering_state(
    store: &SqliteGraphStore,
    snapshot: &graph_store::CanonicalGraphSnapshot,
) -> Result<(String, Vec<graph_store::CanonicalClusterMember>), String> {
    let clustering_mode = snapshot
        .repo_meta
        .get("clustering_mode")
        .cloned()
        .unwrap_or_else(|| "heuristic".to_string());
    let cluster_members = if clustering_mode == "persisted" {
        store.load_clusters()?
    } else {
        Vec::new()
    };
    Ok((clustering_mode, cluster_members))
}

fn emit_current_graph_view(
    handle: &AppHandle,
    state: &AppState,
    repo: &Path,
    current_branch: String,
    current_revision: String,
    since: String,
    since_reset: bool,
) {
    let request = state.last_graph_view_request.lock().clone();
    match ensure_graph_view(state, &request) {
        Ok(view) => {
            apply_graph_view_state(
                state,
                &request,
                &view,
                current_branch.clone(),
                current_revision.clone(),
            );
            let _ = handle.emit(
                "graph-updated",
                serde_json::json!({
                    "graph": view.graph.clone(),
                    "graph_view": view,
                    "current_branch": current_branch,
                    "current_revision": current_revision,
                    "since": since,
                    "since_reset": since_reset,
                    "bookmarks": vcs::get_bookmarks(repo),
                }),
            );
        }
        Err(err) => {
            println!(
                "[BACKEND][graph] failed to emit refreshed graph view for {}: {}",
                repo.display(),
                err
            );
        }
    }
}

fn refresh_clusters_and_emit(handle: &AppHandle, repo: PathBuf) {
    let Some(state) = handle.try_state::<AppState>() else {
        return;
    };
    let result = ClusterService::refresh(&repo);
    if debug_enabled() {
        match &result {
            Ok(refresh) => println!(
                "[BACKEND][cluster] refresh repo={} mode={} assignments={} stale={} err={}",
                repo.display(),
                refresh.mode,
                refresh.assignment_count,
                refresh.stale,
                refresh.error.as_deref().unwrap_or("-")
            ),
            Err(err) => println!(
                "[BACKEND][cluster] refresh repo={} failed: {}",
                repo.display(),
                err
            ),
        }
    }

    *state.cluster_refresh_scheduled.lock() = false;

    if repo != state.repo.lock().clone() {
        return;
    }

    let current_branch = vcs::get_current_branch(&repo);
    let current_revision = vcs::get_current_revision(&repo);
    let since = state
        .last_since
        .lock()
        .clone()
        .unwrap_or_else(|| "@".to_string());
    emit_current_graph_view(
        handle,
        &state,
        &repo,
        current_branch,
        current_revision,
        since,
        false,
    );
}

fn schedule_cluster_refresh(handle: &AppHandle, wait: Duration) {
    let Some(state) = handle.try_state::<AppState>() else {
        return;
    };
    {
        let mut scheduled = state.cluster_refresh_scheduled.lock();
        if *scheduled {
            return;
        }
        *scheduled = true;
    }

    let repo = state.repo.lock().clone();
    let handle_clone = handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        refresh_clusters_and_emit(&handle_clone, repo);
    });
}

fn derive_graph_view(repo: &Path, request: &GraphViewRequest) -> Result<GraphViewResponse, String> {
    let store = SqliteGraphStore::open_or_create(repo)?;
    let snapshot = store.load_snapshot()?;
    if snapshot.entities.is_empty() {
        return Err("Canonical graph store is empty".to_string());
    }
    let (changed_statuses, no_vcs_mode, approximate_historical) =
        graph_view_changed_statuses(repo, request);
    let (clustering_mode, cluster_members) = load_clustering_state(&store, &snapshot)?;
    Ok(GraphViewService::derive(
        &snapshot,
        request,
        &changed_statuses,
        no_vcs_mode,
        approximate_historical,
        &clustering_mode,
        &cluster_members,
    ))
}

fn ensure_graph_view(
    state: &AppState,
    request: &GraphViewRequest,
) -> Result<GraphViewResponse, String> {
    let repo = state.repo.lock().clone();
    match derive_graph_view(&repo, request) {
        Ok(view) => Ok(view),
        // Background rebuild is in progress; return an empty shell so the UI
        // can render VCS data immediately without blocking on the graph build.
        Err(err) if err.contains("empty") => Ok(GraphViewResponse {
            graph: Graph::new(),
            mode: request.mode.clone(),
            aggregate: request.aggregate.clone(),
            approximate_historical: false,
            seed_label: None,
            node_provenance: vec![],
            canonical_entity_count: 0,
            canonical_edge_count: 0,
            clustering_mode: "heuristic".to_string(),
            cluster_assignments: vec![],
        }),
        Err(err) => Err(err),
    }
}

fn apply_graph_view_state(
    state: &AppState,
    request: &GraphViewRequest,
    view: &GraphViewResponse,
    current_branch: String,
    current_revision: String,
) {
    *state.graph.lock() = view.graph.clone();
    *state.last_graph_view_request.lock() = request.clone();
    *state.last_since.lock() = graph_view_since_hint(request);
    *state.last_include_neighbors.lock() = request.depth > 0;
    *state.last_branch.lock() = current_branch;
    *state.last_revision.lock() = current_revision;
}

fn resolve_initial_repo() -> PathBuf {
    let from_arg = std::env::args().nth(1).map(PathBuf::from);
    if let Some(path) = from_arg {
        return path;
    }

    let cwd = std::env::current_dir().expect("no cwd");
    let mut candidate = cwd.clone();
    loop {
        if !matches!(vcs::detect_engine(&candidate), vcs::VCSEngine::None) {
            return candidate;
        }
        if !candidate.pop() {
            break;
        }
    }
    cwd
}

fn activate_repo(app: &AppHandle, state: &AppState, path_buf: PathBuf) -> Result<String, String> {
    let canonical_path = path_buf.canonicalize().unwrap_or(path_buf);
    if !canonical_path.is_dir() {
        return Err("Selected path is not a directory".to_string());
    }

    // Try to serve a stale-but-instant cached snapshot so the first get_graph_view
    // returns immediately.  The background rebuild will replace it and fire
    // graph-updated when the fresh graph is ready.
    let initial_request = GraphViewRequest::default_working_copy();
    let cached_view = derive_graph_view(&canonical_path, &initial_request).ok();

    {
        *state.repo.lock() = canonical_path.clone();
    }

    if let Some(ref view) = cached_view {
        *state.graph.lock() = view.graph.clone();
    } else {
        *state.graph.lock() = Graph::new();
    }
    *state.last_graph_view_request.lock() = initial_request.clone();
    *state.last_since.lock() = graph_view_since_hint(&initial_request);
    *state.last_include_neighbors.lock() = initial_request.depth > 0;
    *state.last_branch.lock() = vcs::get_current_branch(&canonical_path);
    *state.last_revision.lock() = vcs::get_current_revision(&canonical_path);
    *state.watcher_flush_scheduled.lock() = false;
    *state.watcher_pending_vcs_meta.lock() = false;
    *state.watcher_last_vcs_meta_at.lock() = None;
    state.watcher_pending_paths.lock().clear();
    *state.cluster_refresh_scheduled.lock() = false;
    *state.watchdog.lock() = watchdog::WatchdogRuntimeState::default();
    *state.conductor.lock() = conductor::ConductorRuntimeState::default();

    let watcher = setup_watcher(app);
    *state.watcher.lock() = watcher;

    // Always rebuild in the background so the canonical graph stays current.
    let handle_clone = app.clone();
    let repo_clone = canonical_path.clone();
    std::thread::spawn(move || {
        background_rebuild_and_emit(handle_clone, repo_clone);
    });

    Ok(canonical_path.display().to_string())
}

/// Full canonical graph rebuild on a background thread.
/// Emits `graph-build-progress` events as files are parsed, then fires
/// `graph-updated` when the fresh snapshot is stored.
fn background_rebuild_and_emit(handle: AppHandle, repo: PathBuf) {
    let Some(state) = handle.try_state::<AppState>() else {
        return;
    };

    let files = collect_repo_files(&repo);
    let total = files.len();

    if debug_enabled() {
        println!(
            "[BACKEND][graph] bg rebuild start repo={} files={}",
            repo.display(),
            total
        );
    }

    let _ = handle.emit(
        "graph-build-progress",
        serde_json::json!({ "filesProcessed": 0, "filesTotal": total }),
    );

    let mut graph = Graph::new();
    for (i, rel_path) in files.iter().enumerate() {
        // Bail early if the user switched repos mid-build.
        if (i % 200 == 0) && repo != *state.repo.lock() {
            return;
        }
        let (nodes, edges) = parse_graph_fragment(&state.parsers, &repo, rel_path, "unchanged");
        graph.add_nodes(nodes);
        graph.add_edges(edges);
        if (i + 1) % 50 == 0 || i + 1 == total {
            let _ = handle.emit(
                "graph-build-progress",
                serde_json::json!({ "filesProcessed": i + 1, "filesTotal": total }),
            );
        }
    }

    resolve_graph_edges(&mut graph);
    graph.finalize();

    if repo != *state.repo.lock() {
        return;
    }

    let store = match SqliteGraphStore::open_or_create(&repo) {
        Ok(s) => s,
        Err(e) => {
            println!("[BACKEND][graph] bg rebuild store open failed: {e}");
            return;
        }
    };
    let snapshot = graph_store::snapshot_from_graph(
        &repo,
        &graph,
        &vcs::get_current_revision(&repo),
        graph::source_fingerprint(&repo),
    );
    if let Err(e) = store.replace_all(&snapshot) {
        println!("[BACKEND][graph] bg rebuild store write failed: {e}");
        return;
    }
    if let Err(e) = rebuild_search_index(&repo, &snapshot) {
        println!("[BACKEND][graph] bg rebuild search index failed: {e}");
    }

    if debug_enabled() {
        println!(
            "[BACKEND][graph] bg rebuild done repo={} entities={} edges={}",
            repo.display(),
            snapshot.entities.len(),
            snapshot.edges.len()
        );
    }

    if repo != *state.repo.lock() {
        return;
    }

    let current_branch = vcs::get_current_branch(&repo);
    let current_revision = vcs::get_current_revision(&repo);
    let since = state
        .last_since
        .lock()
        .clone()
        .unwrap_or_else(|| "@".to_string());
    emit_current_graph_view(
        &handle,
        &state,
        &repo,
        current_branch,
        current_revision,
        since,
        false,
    );
    schedule_cluster_refresh(&handle, Duration::from_millis(20));
}

// --- Tauri commands ---

#[tauri::command]
async fn select_repo(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    let started = Instant::now();
    if debug_enabled() {
        println!("[BACKEND][cmd] select_repo start");
    }

    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });

    let folder = rx.recv().map_err(|_| "Dialog cancelled".to_string())?;

    if let Some(path) = folder {
        let selected_path = match path {
            tauri_plugin_dialog::FilePath::Path(p) => p,
            tauri_plugin_dialog::FilePath::Url(u) => u
                .to_file_path()
                .map_err(|_| "Invalid URL path".to_string())?,
        };
        let activated = activate_repo(&app, &state, selected_path)?;

        if debug_enabled() {
            println!(
                "[BACKEND][cmd] select_repo done path={} took={}ms",
                activated,
                started.elapsed().as_millis()
            );
        }
        Ok(activated)
    } else {
        if debug_enabled() {
            println!(
                "[BACKEND][cmd] select_repo cancelled after {}ms",
                started.elapsed().as_millis()
            );
        }
        Err("No folder selected".into())
    }
}

/// Show the native folder-picker dialog only; does NOT activate the repo.
/// The frontend calls this first, then shows a loading state, then calls set_repo_path.
#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });
    let folder = rx.recv().map_err(|_| "Dialog cancelled".to_string())?;
    match folder {
        Some(tauri_plugin_dialog::FilePath::Path(p)) => Ok(p.to_string_lossy().into_owned()),
        Some(tauri_plugin_dialog::FilePath::Url(u)) => u
            .to_file_path()
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|_| "Invalid URL path".to_string()),
        None => Err("No folder selected".into()),
    }
}

#[tauri::command]
async fn set_repo_path(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let started = Instant::now();
    if debug_enabled() {
        println!("[BACKEND][cmd] set_repo_path start path={}", path);
    }

    let activated = activate_repo(&app, &state, PathBuf::from(path))?;

    if debug_enabled() {
        println!(
            "[BACKEND][cmd] set_repo_path done path={} took={}ms",
            activated,
            started.elapsed().as_millis()
        );
    }
    Ok(activated)
}

#[tauri::command]
async fn get_graph_view(
    state: tauri::State<'_, AppState>,
    request: GraphViewRequest,
) -> Result<GraphViewResponse, String> {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_graph_view start repo={} mode={:?} depth={} aggregate={:?} seed={:?}",
            repo.display(),
            request.mode,
            request.depth,
            request.aggregate,
            request.seed
        );
    }

    let current_branch = vcs::get_current_branch(&repo);
    let current_revision = vcs::get_current_revision(&repo);
    let view = ensure_graph_view(&state, &request)?;
    apply_graph_view_state(&state, &request, &view, current_branch, current_revision);

    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_graph_view done nodes={} edges={} canonical_entities={} canonical_edges={} took={}ms",
            view.graph.nodes.len(),
            view.graph.edges.len(),
            view.canonical_entity_count,
            view.canonical_edge_count,
            started.elapsed().as_millis()
        );
    }

    Ok(view)
}

#[tauri::command]
async fn get_graph(
    state: tauri::State<'_, AppState>,
    since: Option<String>,
    include_neighbors: Option<bool>,
) -> Result<Graph, String> {
    let request = graph_view_request_from_legacy(since, include_neighbors.unwrap_or(false));
    let view = get_graph_view(state, request).await?;
    Ok(view.graph)
}

#[tauri::command]
async fn search_graph_entities(
    state: tauri::State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    let repo = state.repo.lock().clone();
    let store = SqliteGraphStore::open_or_create(&repo)?;
    let snapshot = store.load_snapshot()?;
    let search = SearchService::open_or_create(&repo)?;
    search.rebuild_from_snapshot(&snapshot)?;
    search.query(&query, limit.unwrap_or(12))
}

#[tauri::command]
async fn refresh_graph_clusters(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<cluster_service::ClusterRefreshResult, String> {
    let repo = state.repo.lock().clone();
    let result = ClusterService::refresh(&repo)?;
    let current_branch = vcs::get_current_branch(&repo);
    let current_revision = vcs::get_current_revision(&repo);
    let since = state
        .last_since
        .lock()
        .clone()
        .unwrap_or_else(|| "@".to_string());
    emit_current_graph_view(
        &app,
        &state,
        &repo,
        current_branch,
        current_revision,
        since,
        false,
    );
    Ok(result)
}

#[tauri::command]
fn get_changes(
    state: tauri::State<AppState>,
    limit: Option<usize>,
    before_id: Option<String>,
) -> Vec<vcs::Change> {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let limit = limit.unwrap_or(20);
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_changes start repo={} limit={} before_id={:?}",
            repo.display(),
            limit,
            before_id
        );
    }
    let changes = vcs::get_changes(&repo, limit, before_id);
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_changes done count={} took={}ms",
            changes.len(),
            started.elapsed().as_millis()
        );
    }
    changes
}

#[tauri::command]
fn get_bookmarks(state: tauri::State<AppState>) -> Vec<vcs::Bookmark> {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let bookmarks = vcs::get_bookmarks(&repo);
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_bookmarks repo={} count={} took={}ms",
            repo.display(),
            bookmarks.len(),
            started.elapsed().as_millis()
        );
    }
    bookmarks
}

#[tauri::command]
fn get_current_branch(state: tauri::State<AppState>) -> String {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let branch = vcs::get_current_branch(&repo);
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_current_branch repo={} branch='{}' took={}ms",
            repo.display(),
            branch,
            started.elapsed().as_millis()
        );
    }
    branch
}

#[tauri::command]
fn get_file_diff(state: tauri::State<AppState>, file: String, since: Option<String>) -> String {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let diff = vcs::get_file_diff(&repo, &file, since.as_deref());
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_file_diff file={} bytes={} took={}ms",
            file,
            diff.len(),
            started.elapsed().as_millis()
        );
    }
    diff
}

#[tauri::command]
fn get_file_source(state: tauri::State<AppState>, file: String) -> String {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let path = repo.join(&file);
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|_| "--- FILE DELETED OR INACCESSIBLE ---".to_string());
    if debug_enabled() {
        println!(
            "[BACKEND][cmd] get_file_source file={} bytes={} took={}ms",
            file,
            content.len(),
            started.elapsed().as_millis()
        );
    }
    content
}

#[tauri::command]
fn save_file(state: tauri::State<AppState>, file: String, content: String) -> Result<(), String> {
    let started = Instant::now();
    let repo = state.repo.lock().clone();
    let path = repo.join(&file);
    let bytes = content.len();
    let result = std::fs::write(path, content).map_err(|e| e.to_string());
    if debug_enabled() {
        match &result {
            Ok(_) => println!(
                "[BACKEND][cmd] save_file file={} bytes={} took={}ms",
                file,
                bytes,
                started.elapsed().as_millis()
            ),
            Err(err) => println!(
                "[BACKEND][cmd] save_file file={} failed='{}' took={}ms",
                file,
                err,
                started.elapsed().as_millis()
            ),
        }
    }
    result
}

#[tauri::command]
fn get_repo_path(state: tauri::State<AppState>) -> String {
    let path = state.repo.lock().display().to_string();
    if debug_enabled() {
        println!("[BACKEND][cmd] get_repo_path -> {}", path);
    }
    path
}

#[tauri::command]
fn get_watchdog_state(state: tauri::State<AppState>) -> watchdog_types::WatchdogStateView {
    let repo = state.repo.lock().clone();
    let runtime = state.watchdog.lock().clone();
    watchdog::build_state_view(&repo, &runtime)
}

#[tauri::command]
fn set_watchdog_active_issue(
    state: tauri::State<AppState>,
    issue: Option<watchdog_types::WatchdogIssueRef>,
) -> watchdog_types::WatchdogStateView {
    {
        let mut runtime = state.watchdog.lock();
        runtime.active_issue = issue;
        runtime.active_issue_context = None;
    }
    let repo = state.repo.lock().clone();
    let runtime = state.watchdog.lock().clone();
    watchdog::build_state_view(&repo, &runtime)
}

#[tauri::command]
fn set_watchdog_active_issue_context(
    state: tauri::State<AppState>,
    issue_context: Option<watchdog_types::WatchdogIssueContext>,
) -> watchdog_types::WatchdogStateView {
    {
        let mut runtime = state.watchdog.lock();
        runtime.active_issue = issue_context.as_ref().map(|ctx| ctx.issue.clone());
        runtime.active_issue_context = issue_context;
    }
    let repo = state.repo.lock().clone();
    let runtime = state.watchdog.lock().clone();
    watchdog::build_state_view(&repo, &runtime)
}

#[tauri::command]
fn record_watchdog_validation(
    state: tauri::State<AppState>,
    validation: watchdog_types::WatchdogValidationRecord,
) -> watchdog_types::WatchdogStateView {
    {
        let mut runtime = state.watchdog.lock();
        runtime.last_validation = Some(validation);
    }
    let repo = state.repo.lock().clone();
    let runtime = state.watchdog.lock().clone();
    watchdog::build_state_view(&repo, &runtime)
}

#[tauri::command]
fn run_watchdog_validation_command(
    state: tauri::State<AppState>,
    kind: String,
    command: String,
) -> Result<watchdog_types::WatchdogStateView, String> {
    let repo = state.repo.lock().clone();
    let trimmed_command = command.trim().to_string();
    if trimmed_command.is_empty() {
        return Err("Validation command is empty".to_string());
    }

    let started = Instant::now();
    let output = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", trimmed_command.as_str()])
            .current_dir(&repo)
            .output()
            .map_err(|err| format!("Failed to run validation command: {err}"))?
    } else {
        Command::new("sh")
            .args(["-lc", trimmed_command.as_str()])
            .current_dir(&repo)
            .output()
            .map_err(|err| format!("Failed to run validation command: {err}"))?
    };

    let exit_code = output
        .status
        .code()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "signal".to_string());
    let tail = output_tail(&output.stdout, &output.stderr, 20, 4_000);
    let duration_ms = started.elapsed().as_millis();
    let mut details = format!(
        "command=`{}` exit_code={} duration={}ms",
        trimmed_command, exit_code, duration_ms
    );
    if !tail.trim().is_empty() {
        details.push('\n');
        details.push_str(&tail);
    }

    let validation = watchdog_types::WatchdogValidationRecord {
        kind,
        success: output.status.success(),
        ran_at: unix_now_label(),
        details: Some(details),
    };
    {
        let mut runtime = state.watchdog.lock();
        runtime.last_validation = Some(validation);
    }

    if debug_enabled() {
        println!(
            "[BACKEND][watchdog] run_watchdog_validation_command repo={} success={} took={}ms",
            repo.display(),
            output.status.success(),
            duration_ms
        );
    }

    let runtime = state.watchdog.lock().clone();
    Ok(watchdog::build_state_view(&repo, &runtime))
}

#[tauri::command]
fn check_watchdog_focus(state: tauri::State<AppState>) -> watchdog_types::WatchdogRunTrace {
    let repo = state.repo.lock().clone();
    let since = state
        .last_since
        .lock()
        .clone()
        .unwrap_or_else(|| "@".to_string());
    let mut runtime = state.watchdog.lock();
    let mut conductor_runtime = state.conductor.lock();
    run_watchdog_check_with_fallback(&repo, &since, &mut runtime, Some(&mut conductor_runtime))
}

#[tauri::command]
fn check_watchdog_before_push(state: tauri::State<AppState>) -> watchdog_types::WatchdogRunTrace {
    let repo = state.repo.lock().clone();
    let since = state
        .last_since
        .lock()
        .clone()
        .unwrap_or_else(|| "@".to_string());
    let mut runtime = state.watchdog.lock();
    let mut conductor_runtime = state.conductor.lock();
    let trace =
        run_watchdog_check_with_fallback(&repo, &since, &mut runtime, Some(&mut conductor_runtime));
    if debug_enabled() {
        println!(
            "[BACKEND][watchdog] check_watchdog_before_push trace_id={} status='{:?}'",
            trace.id, trace.verdict.status
        );
    }
    trace
}

#[tauri::command]
fn get_watchdog_traces(
    state: tauri::State<AppState>,
    limit: Option<usize>,
) -> Vec<watchdog_types::WatchdogRunTrace> {
    let runtime = state.watchdog.lock().clone();
    watchdog::get_recent_traces(&runtime, limit.unwrap_or(20))
}

#[tauri::command]
fn run_watchdog_replay_eval(
    state: tauri::State<AppState>,
    limit: Option<usize>,
) -> watchdog_types::WatchdogReplayEvalReport {
    let repo = state.repo.lock().clone();
    let runtime = state.watchdog.lock().clone();
    let mut traces = runtime.traces;
    if let Some(limit) = limit {
        let take = limit.clamp(1, 500);
        if traces.len() > take {
            traces = traces.split_off(traces.len() - take);
        }
    }

    let (config, _) = watchdog_config::load_repo_config(&repo);
    let report = watchdog::build_replay_eval_report(&traces, &config);
    if debug_enabled() {
        println!(
            "[BACKEND][watchdog] run_watchdog_replay_eval repo={} traces={} raw_issues={} deduped_issues={} status_changes={}",
            repo.display(),
            report.traces_evaluated,
            report.spam.issues_total_raw,
            report.spam.issues_total_deduped,
            report.spam.traces_with_status_change
        );
    }
    report
}

#[tauri::command]
fn mcp_watchdog_get_status(state: tauri::State<AppState>) -> watchdog_types::WatchdogMcpStatus {
    let repo = state.repo.lock().clone();
    let runtime = state.watchdog.lock().clone();
    let view = watchdog::build_state_view(&repo, &runtime);
    let (status, summary, confidence) = match &view.last_verdict {
        Some(verdict) => (
            verdict.status.clone(),
            verdict.summary.clone(),
            verdict.confidence,
        ),
        None => (
            watchdog_types::WatchdogStatus::InsufficientContext,
            "No watchdog verdict yet.".to_string(),
            0.0,
        ),
    };

    watchdog_types::WatchdogMcpStatus {
        generated_at: unix_now_label(),
        status,
        summary,
        confidence,
        active_issue: view.active_issue.clone(),
        alerts_count: view.alerts.len(),
        last_trace_id: view.last_trace_id.clone(),
        branch: view
            .last_snapshot
            .as_ref()
            .map(|snapshot| snapshot.branch.clone())
            .unwrap_or_else(|| vcs::get_current_branch(&repo)),
        revision: view
            .last_snapshot
            .as_ref()
            .map(|snapshot| snapshot.revision.clone())
            .unwrap_or_else(|| vcs::get_current_revision(&repo)),
    }
}

#[tauri::command]
fn mcp_watchdog_check_focus(state: tauri::State<AppState>) -> watchdog_types::WatchdogRunTrace {
    check_watchdog_focus(state)
}

#[tauri::command]
fn mcp_watchdog_get_alerts(
    state: tauri::State<AppState>,
    limit: Option<usize>,
    severity: Option<String>,
) -> Vec<watchdog_types::WatchdogAlert> {
    let runtime = state.watchdog.lock().clone();
    let severity_filter = severity.as_deref().and_then(parse_watchdog_severity_label);
    let take = limit.unwrap_or(50).clamp(1, 500);

    runtime
        .alerts
        .into_iter()
        .rev()
        .filter(|alert| {
            severity_filter
                .as_ref()
                .is_none_or(|severity| alert.severity == *severity)
        })
        .take(take)
        .collect()
}

#[tauri::command]
fn mcp_watchdog_get_issue_context(
    state: tauri::State<AppState>,
) -> Option<watchdog_types::WatchdogIssueContext> {
    let runtime = state.watchdog.lock().clone();
    runtime.active_issue_context
}

#[tauri::command]
fn mcp_watchdog_get_evidence_snapshot(
    state: tauri::State<AppState>,
) -> Option<watchdog_types::WatchdogSnapshot> {
    let runtime = state.watchdog.lock().clone();
    runtime.last_snapshot
}

#[tauri::command]
fn get_mission_state(
    state: tauri::State<AppState>,
    mission_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let store = WorkStore::open_or_create(&repo)?;
    let normalized_id = mission_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    store
        .load_mission_state(normalized_id, 200)
        .map(mission_view_to_frontend)
}

#[tauri::command]
fn save_mission_input(
    state: tauri::State<AppState>,
    input: MissionInput,
) -> Result<serde_json::Value, String> {
    if input.title.trim().is_empty() {
        return Err("Mission title is required.".to_string());
    }
    if input.brief.trim().is_empty() {
        return Err("Mission brief is required.".to_string());
    }
    if input.validation_command.trim().is_empty() {
        return Err("Validation command is required.".to_string());
    }

    let repo = state.repo.lock().clone();
    let last_revision = state.last_revision.lock().clone();
    let mission = build_mission_record(&repo, &input, &last_revision);
    let mission = mission?;
    let mut store = WorkStore::open_or_create(&repo)?;
    store.upsert_mission(&mission)?;
    update_watchdog_from_mission(&mut state.watchdog.lock(), &mission);
    store
        .load_mission_state(Some(&mission.id), 200)
        .map(mission_view_to_frontend)
}

#[tauri::command]
fn plan_mission(
    state: tauri::State<AppState>,
    input: MissionInput,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let last_revision = state.last_revision.lock().clone();
    let mut mission = build_mission_record(&repo, &input, &last_revision)?;
    mission.state = "planning".to_string();
    mission.updated_at = unix_now_label();

    let mut store = WorkStore::open_or_create(&repo)?;
    store.upsert_mission(&mission)?;
    let planner_profile =
        resolve_mission_planner_profile(&store, input.planner_profile_id.as_deref())?;
    let planner_label = planner_profile
        .as_ref()
        .map(|profile| profile.label.clone())
        .unwrap_or_else(|| "fixture".to_string());
    let objectives = if let Some(profile) = planner_profile.as_ref() {
        plan_mission_with_nim_profile(&repo, &input, profile)?
    } else {
        fixture_objectives(&input)
    };
    store.plan_mission(&mission, &mission.brief, &objectives)?;
    store.persist_run_event(
        &mission.id,
        "",
        &planner_label,
        "planned",
        serde_json::json!({
            "objectiveCount": objectives.len(),
            "planner": planner_label,
            "summary": format!("Planned {} objective(s) with {}.", objectives.len(), planner_label),
        }),
    )?;
    update_watchdog_from_mission(&mut state.watchdog.lock(), &mission);
    store
        .load_mission_state(Some(&mission.id), 200)
        .map(mission_view_to_frontend)
}

#[tauri::command]
fn run_fixture_worker(
    state: tauri::State<AppState>,
    mission_id: Option<String>,
    objective_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let mut mission = if let Some(requested) = mission_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        store
            .get_mission(requested)?
            .ok_or_else(|| format!("Mission not found: {requested}"))?
    } else {
        store
            .get_latest_mission()?
            .ok_or_else(|| "No mission exists.".to_string())?
    };

    let objectives = store.list_objectives(&mission.id)?;
    let objective_hint = objective_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let claimed_unit = if let Some(requested) = objective_hint {
        store.claim_specific_work_unit(&mission.id, requested, "fixture-worker")?
    } else {
        store.claim_work_unit(&mission.id, "fixture-worker")?
    };
    let Some(unit) = claimed_unit else {
        return store
            .load_mission_state(Some(&mission.id), 200)
            .map(mission_view_to_frontend);
    };

    let objective = objectives
        .into_iter()
        .find(|entry| entry.id == unit.id)
        .ok_or_else(|| format!("Objective not found after claim: {}", unit.id))?;
    let now = unix_now_label();
    let session_id = format!("fixture-{}-{}", mission.id, unix_now_seconds());
    let workspace_path = repo
        .join(".sloposcope")
        .join("missions")
        .join(&mission.id)
        .join(&objective.id);
    let workspace_label = workspace_path.display().to_string();
    let running_session = AgentSessionRuntimeRecord {
        id: session_id.clone(),
        mission_id: mission.id.clone(),
        objective_id: Some(objective.id.clone()),
        profile_id: "fixture-worker".to_string(),
        assignment_kind: "execute_objective".to_string(),
        state: "running".to_string(),
        touched_paths: objective.meta.scope_paths.clone(),
        workspace_path: workspace_label.clone(),
        lease_expires_at: Some(now.clone()),
        last_heartbeat_at: Some(now.clone()),
        last_event_at: Some(now.clone()),
        last_error: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    store.upsert_agent_session_runtime(&running_session)?;
    store.persist_run_event(
        &mission.id,
        &objective.id,
        &session_id,
        "started",
        serde_json::json!({
            "objectiveId": objective.id,
            "profileId": "fixture-worker",
        }),
    )?;
    store.persist_run_event(
        &mission.id,
        &objective.id,
        &session_id,
        "progress",
        serde_json::json!({
            "message": "Fixture worker executed objective.",
            "touchedPaths": objective.meta.scope_paths,
        }),
    )?;
    let completed_at = unix_now_label();
    store.insert_verification(&NewVerificationRecord {
        mission_id: mission.id.clone(),
        objective_id: Some(objective.id.clone()),
        session_id: Some(session_id.clone()),
        status: "passed".to_string(),
        summary: format!("Fixture worker completed {}.", objective.id),
        details: serde_json::json!({
            "kind": "fixture_worker",
            "command": mission.validation_command.clone(),
            "profileId": "fixture-worker",
            "workspacePath": workspace_label,
        }),
    })?;
    store.submit_verdict(UnitVerdict {
        unit_id: unit.id.clone(),
        task_id: mission.id.clone(),
        status: "done".to_string(),
        summary: format!("Fixture worker completed {}", unit.title),
        submitted_at: completed_at.clone(),
        artifacts: vec![VerdictArtifact {
            kind: "summary".to_string(),
            content: "fixture-worker completion".to_string(),
        }],
    })?;
    store.persist_run_event(
        &mission.id,
        &unit.id,
        &session_id,
        "completed",
        serde_json::json!({
            "summary": format!("Completed {}", unit.title),
        }),
    )?;
    store.upsert_agent_session_runtime(&AgentSessionRuntimeRecord {
        id: session_id,
        mission_id: mission.id.clone(),
        objective_id: Some(unit.id),
        profile_id: "fixture-worker".to_string(),
        assignment_kind: "execute_objective".to_string(),
        state: "completed".to_string(),
        touched_paths: objective.meta.scope_paths,
        workspace_path: repo
            .join(".sloposcope")
            .join("missions")
            .join(&mission.id)
            .join(&objective.id)
            .display()
            .to_string(),
        lease_expires_at: None,
        last_heartbeat_at: Some(completed_at.clone()),
        last_event_at: Some(completed_at.clone()),
        last_error: None,
        created_at: now,
        updated_at: completed_at,
    })?;

    let planned_state = store.load_mission_state(Some(&mission.id), 200)?;
    mission.state = if planned_state.summary.pending_objectives == 0
        && planned_state.summary.claimed_objectives == 0
        && planned_state.summary.total_objectives > 0
    {
        "completed".to_string()
    } else {
        "running".to_string()
    };
    mission.updated_at = unix_now_label();
    store.upsert_mission(&mission)?;
    store
        .load_mission_state(Some(&mission.id), 200)
        .map(mission_view_to_frontend)
}

#[tauri::command]
fn run_code_worker(
    state: tauri::State<AppState>,
    mission_id: Option<String>,
    objective_id: Option<String>,
    profile_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let mut mission = if let Some(requested) = mission_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        store
            .get_mission(requested)?
            .ok_or_else(|| format!("Mission not found: {requested}"))?
    } else {
        store
            .get_latest_mission()?
            .ok_or_else(|| "No mission exists.".to_string())?
    };

    let objectives = store.list_objectives(&mission.id)?;
    let objective_hint = objective_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let claimed_unit = if let Some(requested) = objective_hint {
        store.claim_specific_work_unit(&mission.id, requested, "local-code-worker")?
    } else {
        store.claim_work_unit(&mission.id, "local-code-worker")?
    };
    let Some(unit) = claimed_unit else {
        return store
            .load_mission_state(Some(&mission.id), 200)
            .map(mission_view_to_frontend);
    };
    let objective = objectives
        .into_iter()
        .find(|entry| entry.id == unit.id)
        .ok_or_else(|| format!("Objective not found after claim: {}", unit.id))?;
    let now = unix_now_label();
    let session_id = format!("code-{}-{}", mission.id, unix_now_seconds());
    let selected_profile = resolve_requested_agent_profile(&store, profile_id.as_deref())?;
    let profile_label = selected_profile
        .as_ref()
        .map(|profile| profile.id.clone())
        .unwrap_or_else(|| "local-code-worker".to_string());

    store.upsert_agent_session_runtime(&AgentSessionRuntimeRecord {
        id: session_id.clone(),
        mission_id: mission.id.clone(),
        objective_id: Some(objective.id.clone()),
        profile_id: profile_label.clone(),
        assignment_kind: "execute_objective".to_string(),
        state: "running".to_string(),
        touched_paths: Vec::new(),
        workspace_path: String::new(),
        lease_expires_at: Some(now.clone()),
        last_heartbeat_at: Some(now.clone()),
        last_event_at: Some(now.clone()),
        last_error: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    })?;
    store.persist_run_event(
        &mission.id,
        &objective.id,
        &session_id,
        "started",
        serde_json::json!({
            "objectiveId": objective.id.clone(),
            "profileId": profile_label.clone(),
        }),
    )?;
    let execution = if let Some(profile) = selected_profile.as_ref() {
        if profile.command.trim().is_empty() || profile.transport_kind == "internal" {
            execute_internal_code_worker(&repo, &mission, &objective, &session_id)?
        } else {
            execute_profiled_code_worker(&repo, &mission, &objective, &session_id, profile)?
        }
    } else {
        execute_internal_code_worker(&repo, &mission, &objective, &session_id)?
    };
    let worker_success = execution
        .diagnostics
        .as_ref()
        .and_then(|details| details.get("exitCode"))
        .and_then(serde_json::Value::as_i64)
        .map(|code| code == 0)
        .unwrap_or(true);
    store.persist_run_event(
        &mission.id,
        &objective.id,
        &session_id,
        "progress",
        serde_json::json!({
            "message": format!("Code worker wrote {}", execution.relative_path),
            "workspacePath": execution.workspace_path.clone(),
            "touchedPaths": execution.touched_paths.clone(),
            "patchPath": execution.patch_artifact_path.clone(),
        }),
    )?;
    let completed_at = unix_now_label();
    let agent_verification = latest_agent_submitted_verification_for_session(
        &store,
        &mission.id,
        &objective.id,
        &session_id,
    )?;
    let verification_status = agent_verification
        .as_ref()
        .map(|record| record.status.clone())
        .unwrap_or_else(|| {
            if worker_success && execution.validation.success {
                "passed".to_string()
            } else {
                "failed".to_string()
            }
        });
    let workspace_path = execution.workspace_path.clone();
    let relative_path = execution.relative_path.clone();
    let patch_text = execution.patch_text.clone();
    let touched_paths = execution.touched_paths.clone();
    let validation_command = execution.validation.command.clone();
    let validation_success = execution.validation.success;
    let validation_exit_code = execution.validation.exit_code;
    let validation_stdout = execution.validation.stdout.clone();
    let validation_stderr = execution.validation.stderr.clone();
    let mut verification_details = agent_verification
        .as_ref()
        .map(|record| record.details.clone())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(map) = verification_details.as_object_mut() {
        map.insert(
            "kind".to_string(),
            serde_json::Value::String(execution.worker_kind.clone()),
        );
        map.insert(
            "command".to_string(),
            serde_json::Value::String(validation_command.clone()),
        );
        map.insert(
            "profileId".to_string(),
            serde_json::Value::String(execution.profile_id.clone()),
        );
        map.insert(
            "workspacePath".to_string(),
            serde_json::Value::String(workspace_path.clone()),
        );
        map.insert(
            "baseRevision".to_string(),
            serde_json::Value::String(execution.base_revision.clone()),
        );
        map.insert(
            "patchPath".to_string(),
            serde_json::Value::String(execution.patch_artifact_path.clone()),
        );
        map.insert(
            "patchSummary".to_string(),
            serde_json::Value::String(patch_summary_for_path(&relative_path)),
        );
        map.insert(
            "targetFile".to_string(),
            serde_json::Value::String(relative_path.clone()),
        );
        map.insert(
            "touchedPaths".to_string(),
            serde_json::to_value(&touched_paths).unwrap_or_else(|_| serde_json::json!([])),
        );
        map.insert(
            "patch".to_string(),
            serde_json::Value::String(patch_text.clone()),
        );
        map.insert(
            "validationStatus".to_string(),
            serde_json::Value::String(verification_status.clone()),
        );
        map.insert(
            "validation".to_string(),
            serde_json::json!({
                "command": validation_command,
                "success": validation_success,
                "exitCode": validation_exit_code,
                "stdout": validation_stdout,
                "stderr": validation_stderr,
            }),
        );
        map.insert(
            "diagnostics".to_string(),
            execution
                .diagnostics
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
        );
    }
    let verification_summary = agent_verification
        .as_ref()
        .map(|record| record.summary.clone())
        .unwrap_or_else(|| format!("Code worker completed {}.", objective.id));
    let verification_id = if let Some(existing) = agent_verification {
        store.update_verification(
            existing.id,
            &verification_status,
            &verification_summary,
            &verification_details,
        )?;
        existing.id
    } else {
        store.insert_verification(&NewVerificationRecord {
            mission_id: mission.id.clone(),
            objective_id: Some(objective.id.clone()),
            session_id: Some(session_id.clone()),
            status: verification_status.clone(),
            summary: verification_summary.clone(),
            details: verification_details,
        })?
    };
    store.upsert_patch_decision(&PatchDecisionRecord {
        mission_id: mission.id.clone(),
        objective_id: objective.id.clone(),
        session_id: Some(session_id.clone()),
        status: "pending_review".to_string(),
        reason: None,
        patch_base_revision: Some(execution.base_revision.clone()),
        apply_target_path: None,
        apply_branch: None,
        applied_at: None,
        apply_error: None,
        created_at: completed_at.clone(),
        updated_at: completed_at.clone(),
    })?;
    let mut verdict = UnitVerdict {
        unit_id: objective.id.clone(),
        task_id: mission.id.clone(),
        status: if worker_success && validation_success {
            "done".to_string()
        } else {
            "failed".to_string()
        },
        summary: format!("Code worker completed {}", unit.title),
        submitted_at: completed_at.clone(),
        artifacts: vec![VerdictArtifact {
            kind: "patch".to_string(),
            content: patch_text.clone(),
        }],
    };
    verdict.artifacts.push(VerdictArtifact {
        kind: "verification_id".to_string(),
        content: format!("verification-{verification_id}"),
    });
    store.submit_verdict(verdict)?;
    store.persist_run_event(
        &mission.id,
        &unit.id,
        &session_id,
        if worker_success && validation_success {
            "completed"
        } else {
            "failed"
        },
        serde_json::json!({
            "summary": format!("Code worker completed {}.", unit.id),
            "verificationId": verification_id,
            "validationSuccess": validation_success,
            "workerSuccess": worker_success,
            "relativePath": execution.relative_path.clone(),
        }),
    )?;
    store.upsert_agent_session_runtime(&AgentSessionRuntimeRecord {
        id: session_id,
        mission_id: mission.id.clone(),
        objective_id: Some(unit.id),
        profile_id: execution.profile_id.clone(),
        assignment_kind: "execute_objective".to_string(),
        state: if worker_success && validation_success {
            "completed".to_string()
        } else {
            "failed".to_string()
        },
        touched_paths,
        workspace_path: execution.workspace_path,
        lease_expires_at: None,
        last_heartbeat_at: Some(completed_at.clone()),
        last_event_at: Some(completed_at.clone()),
        last_error: if worker_success && validation_success {
            None
        } else {
            Some(execution.validation.stderr)
        },
        created_at: now,
        updated_at: completed_at,
    })?;

    let planned_state = store.load_mission_state(Some(&mission.id), 200)?;
    mission.state = if planned_state.summary.failed_objectives > 0 {
        "failed".to_string()
    } else if planned_state.summary.pending_objectives == 0
        && planned_state.summary.claimed_objectives == 0
        && planned_state.summary.total_objectives > 0
    {
        "completed".to_string()
    } else {
        "running".to_string()
    };
    mission.updated_at = unix_now_label();
    store.upsert_mission(&mission)?;
    store
        .load_mission_state(Some(&mission.id), 200)
        .map(mission_view_to_frontend)
}

#[tauri::command]
fn run_next_code_worker(
    state: tauri::State<AppState>,
    mission_id: Option<String>,
    profile_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let store = WorkStore::open_or_create(&repo)?;
    let mission = if let Some(requested) = mission_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        store
            .get_mission(requested)?
            .ok_or_else(|| format!("Mission not found: {requested}"))?
    } else {
        store
            .get_latest_mission()?
            .ok_or_else(|| "No mission exists.".to_string())?
    };
    let view = store.load_mission_state(Some(&mission.id), 200)?;
    let scheduler = derive_mission_scheduler_summary(&view, unix_now_seconds());
    let next_objective_id = scheduler
        .next_objective_id
        .ok_or_else(|| "No schedulable objective found.".to_string())?;
    drop(store);
    run_code_worker(state, Some(mission.id), Some(next_objective_id), profile_id)
}

#[tauri::command]
fn sweep_stale_sessions(
    state: tauri::State<AppState>,
    mission_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let mut mission = if let Some(requested) = mission_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        store
            .get_mission(requested)?
            .ok_or_else(|| format!("Mission not found: {requested}"))?
    } else {
        store
            .get_latest_mission()?
            .ok_or_else(|| "No mission exists.".to_string())?
    };
    let view = store.load_mission_state(Some(&mission.id), 200)?;
    let now = unix_now_label();
    let mut swept = 0usize;

    for session in &view.sessions {
        let runtime = derive_agent_session_runtime_slice_at(session, unix_now_seconds());
        if runtime.runtime_status != "stuck" {
            continue;
        }
        let mut updated = session.clone();
        updated.state = "failed".to_string();
        updated.lease_expires_at = None;
        updated.last_error = Some("stale session swept".to_string());
        updated.last_heartbeat_at = Some(now.clone());
        updated.last_event_at = Some(now.clone());
        updated.updated_at = now.clone();
        store.upsert_agent_session_runtime(&updated)?;

        if let Some(objective_id) = session.objective_id.as_deref() {
            store.requeue_work_unit(&mission.id, objective_id)?;
            store.persist_run_event(
                &mission.id,
                objective_id,
                &session.id,
                "stale_swept",
                serde_json::json!({
                    "summary": format!("Swept stale session {}", session.id),
                    "objectiveId": objective_id,
                }),
            )?;
        }
        swept += 1;
    }

    if swept == 0 {
        store.persist_run_event(
            &mission.id,
            "",
            "scheduler",
            "stale_sweep_noop",
            serde_json::json!({
                "summary": "No stale sessions to sweep.",
            }),
        )?;
    }

    let refreshed = store.load_mission_state(Some(&mission.id), 200)?;
    mission.state = if refreshed.summary.pending_objectives == 0
        && refreshed.summary.claimed_objectives == 0
        && refreshed.summary.total_objectives > 0
    {
        "completed".to_string()
    } else {
        "running".to_string()
    };
    mission.updated_at = unix_now_label();
    store.upsert_mission(&mission)?;
    store
        .load_mission_state(Some(&mission.id), 200)
        .map(mission_view_to_frontend)
}

#[tauri::command]
fn list_agent_profiles(state: tauri::State<AppState>) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let store = WorkStore::open_or_create(&repo)?;
    let profiles = store.list_agent_profiles()?;
    Ok(serde_json::json!({
        "profiles": profiles,
    }))
}

#[tauri::command]
fn save_agent_profile(
    state: tauri::State<AppState>,
    profile: AgentProfileInput,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let existing_created_at = profile
        .id
        .as_deref()
        .and_then(|profile_id| store.get_agent_profile(profile_id).ok().flatten())
        .map(|existing| existing.created_at);
    let mut record = build_agent_profile_record(profile)?;
    if let Some(created_at) = existing_created_at {
        record.created_at = created_at;
    }
    store.upsert_agent_profile(&record)?;
    list_agent_profiles(state)
}

#[tauri::command]
fn delete_agent_profile(
    state: tauri::State<AppState>,
    profile_id: String,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    store.delete_agent_profile(profile_id.trim())?;
    list_agent_profiles(state)
}

#[tauri::command]
fn evaluate_worker_patch_gate(
    state: tauri::State<AppState>,
    mission_id: Option<String>,
    objective_id: String,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let mission = if let Some(requested) = mission_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        store
            .get_mission(requested)?
            .ok_or_else(|| format!("Mission not found: {requested}"))?
    } else {
        store
            .get_latest_mission()?
            .ok_or_else(|| "No mission exists.".to_string())?
    };
    let view = store.load_mission_state(Some(&mission.id), 200)?;
    let objective = view
        .objectives
        .iter()
        .find(|entry| entry.id == objective_id)
        .cloned()
        .ok_or_else(|| format!("Objective not found: {objective_id}"))?;
    let session = latest_session_for_objective(&view, &objective.id)
        .cloned()
        .ok_or_else(|| format!("No session found for objective: {}", objective.id))?;
    let verification = latest_verification_for_objective(&view, &objective.id)
        .cloned()
        .ok_or_else(|| format!("No verification found for objective: {}", objective.id))?;
    let gate =
        evaluate_patch_gate_for_objective(&repo, &mission, &objective, &session, &verification);
    store.upsert_patch_gate(&gate)?;
    store
        .load_mission_state(Some(&mission.id), 200)
        .map(mission_view_to_frontend)
}

#[tauri::command]
fn accept_worker_patch(
    state: tauri::State<AppState>,
    mission_id: Option<String>,
    objective_id: String,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let mission = if let Some(requested) = mission_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        store
            .get_mission(requested)?
            .ok_or_else(|| format!("Mission not found: {requested}"))?
    } else {
        store
            .get_latest_mission()?
            .ok_or_else(|| "No mission exists.".to_string())?
    };
    let view = store.load_mission_state(Some(&mission.id), 200)?;
    let session = latest_session_for_objective(&view, &objective_id)
        .cloned()
        .ok_or_else(|| format!("No session found for objective: {objective_id}"))?;
    let base_revision = latest_verification_for_objective(&view, &objective_id)
        .and_then(|verification| verification_details_patch_base_revision(&verification.details));
    let now = unix_now_label();
    store.upsert_patch_decision(&PatchDecisionRecord {
        mission_id: mission.id.clone(),
        objective_id: objective_id.clone(),
        session_id: Some(session.id.clone()),
        status: "accepted".to_string(),
        reason: None,
        patch_base_revision: base_revision,
        apply_target_path: None,
        apply_branch: None,
        applied_at: None,
        apply_error: None,
        created_at: now.clone(),
        updated_at: now,
    })?;
    evaluate_worker_patch_gate(state, Some(mission.id), objective_id)
}

#[tauri::command]
fn reject_worker_patch(
    state: tauri::State<AppState>,
    mission_id: Option<String>,
    objective_id: String,
    reason: Option<String>,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let mission = if let Some(requested) = mission_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        store
            .get_mission(requested)?
            .ok_or_else(|| format!("Mission not found: {requested}"))?
    } else {
        store
            .get_latest_mission()?
            .ok_or_else(|| "No mission exists.".to_string())?
    };
    let now = unix_now_label();
    let reason = reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| Some("rejected by operator".to_string()));
    store.upsert_patch_decision(&PatchDecisionRecord {
        mission_id: mission.id.clone(),
        objective_id: objective_id.clone(),
        session_id: None,
        status: "rejected".to_string(),
        reason: reason.clone(),
        patch_base_revision: None,
        apply_target_path: None,
        apply_branch: None,
        applied_at: None,
        apply_error: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    })?;
    store.upsert_patch_gate(&PatchGateRecord {
        mission_id: mission.id.clone(),
        objective_id,
        session_id: None,
        status: "blocked".to_string(),
        reasons: vec![reason.unwrap_or_else(|| "rejected".to_string())],
        watchdog_status: None,
        created_at: now.clone(),
        updated_at: now,
    })?;
    store
        .load_mission_state(Some(&mission.id), 200)
        .map(mission_view_to_frontend)
}

#[tauri::command]
fn apply_worker_patch(
    state: tauri::State<AppState>,
    mission_id: Option<String>,
    objective_id: String,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let mission = if let Some(requested) = mission_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        store
            .get_mission(requested)?
            .ok_or_else(|| format!("Mission not found: {requested}"))?
    } else {
        store
            .get_latest_mission()?
            .ok_or_else(|| "No mission exists.".to_string())?
    };
    let decision = store
        .get_patch_decision(&mission.id, &objective_id)?
        .ok_or_else(|| format!("Patch decision not found for objective: {objective_id}"))?;
    if decision.status != "accepted" {
        return Err(format!(
            "Patch decision must be accepted before apply; got {}",
            decision.status
        ));
    }
    let gate = store
        .get_patch_gate(&mission.id, &objective_id)?
        .ok_or_else(|| format!("Patch gate not found for objective: {objective_id}"))?;
    if gate.status != "ready" {
        return Err(format!(
            "Patch gate must be ready before apply; got {}",
            gate.status
        ));
    }
    let verification = store
        .get_latest_verification_for_objective(&mission.id, &objective_id)?
        .ok_or_else(|| format!("Verification not found for objective: {objective_id}"))?;
    let patch_text = verification
        .details
        .get("patch")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Patch artifact missing from verification.".to_string())?;
    let touched_paths = verification
        .details
        .get("touchedPaths")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let base_revision = decision
        .patch_base_revision
        .clone()
        .or_else(|| verification_details_patch_base_revision(&verification.details));
    let now = unix_now_label();
    let failure_reason = if let Some(base_revision) = base_revision.as_deref() {
        ensure_apply_freshness(&repo, &mission, &touched_paths, base_revision).err()
    } else {
        Some("patch base revision missing".to_string())
    };

    if let Some(reason) = failure_reason {
        let mut failed = decision.clone();
        failed.status = "apply_failed".to_string();
        failed.apply_error = Some(reason);
        failed.updated_at = now.clone();
        store.upsert_patch_decision(&failed)?;
        return store
            .load_mission_state(Some(&mission.id), 200)
            .map(mission_view_to_frontend);
    }

    if let Err(reason) = apply_patch_text(&repo, &mission.id, &objective_id, patch_text) {
        let mut failed = decision.clone();
        failed.status = "apply_failed".to_string();
        failed.apply_error = Some(reason);
        failed.updated_at = now.clone();
        store.upsert_patch_decision(&failed)?;
        return store
            .load_mission_state(Some(&mission.id), 200)
            .map(mission_view_to_frontend);
    }
    let mut applied = decision.clone();
    applied.status = "applied".to_string();
    applied.apply_target_path = Some(repo.display().to_string());
    applied.apply_branch = Some(current_repo_branch(&repo));
    applied.applied_at = Some(now.clone());
    applied.apply_error = None;
    applied.updated_at = now.clone();
    store.upsert_patch_decision(&applied)?;
    store.persist_run_event(
        &mission.id,
        &objective_id,
        decision.session_id.as_deref().unwrap_or(""),
        "applied",
        serde_json::json!({
            "summary": format!("Applied worker patch for {}", objective_id),
            "branch": applied.apply_branch,
            "targetPath": applied.apply_target_path,
        }),
    )?;
    store
        .load_mission_state(Some(&mission.id), 200)
        .map(mission_view_to_frontend)
}

#[tauri::command]
fn agent_login(
    state: tauri::State<AppState>,
    input: AgentLoginInput,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let now = unix_now_label();
    let session_id = input
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("agent-{}-{}", input.profile_id, unix_now_seconds()));
    store.upsert_agent_session_runtime(&AgentSessionRuntimeRecord {
        id: session_id.clone(),
        mission_id: input.mission_id.clone(),
        objective_id: input.objective_id.clone(),
        profile_id: input.profile_id.clone(),
        assignment_kind: input
            .assignment_kind
            .unwrap_or_else(|| "execute_objective".to_string()),
        state: "running".to_string(),
        touched_paths: Vec::new(),
        workspace_path: input.workspace_path.unwrap_or_default(),
        lease_expires_at: Some(now.clone()),
        last_heartbeat_at: Some(now.clone()),
        last_event_at: Some(now.clone()),
        last_error: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    })?;
    store.persist_run_event(
        &input.mission_id,
        input.objective_id.as_deref().unwrap_or(""),
        &session_id,
        "agent_login",
        serde_json::json!({
            "profileId": input.profile_id,
        }),
    )?;
    Ok(serde_json::json!({
        "sessionId": session_id,
        "at": now,
    }))
}

#[tauri::command]
fn agent_activity_set(
    state: tauri::State<AppState>,
    input: AgentActivityInput,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let now = unix_now_label();
    store.update_agent_session_status(&input.session_id, &input.state, &now, input.error)?;
    store.persist_run_event(
        &input.mission_id,
        "",
        &input.session_id,
        "activity",
        serde_json::json!({
            "state": input.state,
        }),
    )?;
    Ok(serde_json::json!({ "ok": true, "at": now }))
}

#[tauri::command]
fn agent_report_event(
    state: tauri::State<AppState>,
    input: AgentEventInput,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let now = unix_now_label();
    let objective_id = input.objective_id.unwrap_or_default();
    let event_id = store.persist_run_event(
        &input.mission_id,
        &objective_id,
        &input.session_id,
        &input.event_type,
        input.payload,
    )?;
    store.update_agent_session_status(&input.session_id, "running", &now, None)?;
    Ok(serde_json::json!({ "eventId": event_id, "at": now }))
}

#[tauri::command]
fn agent_report_touched_files(
    state: tauri::State<AppState>,
    input: AgentTouchedPathsInput,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let now = unix_now_label();
    store.merge_agent_session_touched_paths(&input.session_id, &input.paths, &now)?;
    store.persist_run_event(
        &input.mission_id,
        "",
        &input.session_id,
        "touched_files",
        serde_json::json!({
            "paths": input.paths,
        }),
    )?;
    Ok(serde_json::json!({ "ok": true, "at": now }))
}

#[tauri::command]
fn agent_submit_verdict(
    state: tauri::State<AppState>,
    input: AgentVerdictInput,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let now = unix_now_label();
    let (verification_id, objective_id) = persist_agent_verdict(
        &mut store,
        &input.mission_id,
        &input.session_id,
        input.objective_id.as_deref(),
        &input.status,
        &input.summary,
        input.details,
        input.artifacts.unwrap_or_default(),
        &now,
    )?;
    store.persist_run_event(
        &input.mission_id,
        &objective_id,
        &input.session_id,
        "agent_verdict",
        serde_json::json!({
            "status": input.status,
            "summary": input.summary,
            "verificationId": verification_id,
        }),
    )?;
    Ok(serde_json::json!({
        "ok": true,
        "at": now,
        "verificationId": verification_id,
        "objectiveId": objective_id,
    }))
}

#[tauri::command]
fn agent_logout(
    state: tauri::State<AppState>,
    input: AgentLogoutInput,
) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().clone();
    let mut store = WorkStore::open_or_create(&repo)?;
    let now = unix_now_label();
    let state_value = input.state.unwrap_or_else(|| "logged_out".to_string());
    store.update_agent_session_status(&input.session_id, &state_value, &now, input.error)?;
    store.persist_run_event(
        &input.mission_id,
        "",
        &input.session_id,
        "agent_logout",
        serde_json::json!({
            "state": state_value,
        }),
    )?;
    Ok(serde_json::json!({ "ok": true, "at": now }))
}

#[tauri::command]
fn conductor_get_status(
    state: tauri::State<AppState>,
    trace_limit: Option<usize>,
) -> conductor::ConductorStatusView {
    let runtime = state.conductor.lock().clone();
    conductor::build_status_view(&runtime, unix_now_seconds(), trace_limit.unwrap_or(30))
}

#[tauri::command]
fn conductor_refresh(
    state: tauri::State<AppState>,
    updates: Option<Vec<conductor::ConductorRunnerUpdate>>,
) -> Result<conductor::ConductorRefreshResponse, String> {
    let now = unix_now_seconds();
    let mut runtime = state.conductor.lock();
    conductor::refresh_runtime(&mut runtime, updates.as_deref().unwrap_or(&[]), now)
}

#[tauri::command]
fn conductor_enqueue_task(
    state: tauri::State<AppState>,
    task_id: String,
    max_retries: Option<u32>,
) -> Result<conductor::ConductorTask, String> {
    let repo = state.repo.lock().clone();
    let now = unix_now_seconds();
    // Read watchdog issue context before acquiring the conductor lock.
    let active_issue_context = state.watchdog.lock().active_issue_context.clone();
    let mut runtime = state.conductor.lock();
    let task = conductor::enqueue_task(&mut runtime, &task_id, now, max_retries)?;
    let contract_load = conductor::load_workflow_contract(&repo);
    if let Some(contract) = contract_load.contract {
        let mut context = conductor::TaskContext::from_contract(
            &task.task_id,
            &contract,
            contract.prompt_template.clone(),
        );
        if let Some(issue_ctx) = &active_issue_context {
            let issue = &issue_ctx.issue;
            context = context.with_issue(issue.id.clone(), issue.title.clone());
        }
        runtime
            .issue_runs
            .push(conductor::IssueRun::new(task.clone(), context));
    }
    Ok(task)
}

#[tauri::command]
fn conductor_prepare_launch(
    state: tauri::State<AppState>,
    task_id: String,
) -> Result<conductor::ConductorRunnerLaunchPayload, String> {
    let repo = state.repo.lock().clone();
    let now = unix_now_seconds();
    let mut runtime = state.conductor.lock();
    let ir = runtime
        .issue_runs
        .iter_mut()
        .find(|ir| ir.task.task_id == task_id.trim().to_ascii_lowercase())
        .ok_or_else(|| format!("no issue run for task: {task_id}"))?;
    if ir.is_terminal() {
        return Err(format!("task is already terminal: {task_id}"));
    }
    let run_attempt = ir.task.run_attempt;
    let task_id_norm = ir.task.task_id.clone();
    let prompt = ir.context.prompt.clone();
    let workflow_id = ir.context.workflow_id.clone();
    let session_id = conductor::app_server_session_id(&task_id_norm, run_attempt)?;
    let workspace_root = repo.join(".sloposcope").join("workspaces");
    let workspace_spec = conductor::ensure_task_workspace(&workspace_root, &task_id_norm)?;
    let worker_run = conductor::WorkerRun {
        session_id: session_id.clone(),
        task_id: task_id_norm.clone(),
        run_attempt,
        workspace_path: workspace_spec.workspace_path.clone(),
        launched_at: format!("unix:{now}"),
    };
    ir.attach_session(worker_run.session_id.clone());
    let contract_load = conductor::load_workflow_contract(&repo);
    let provider = contract_load
        .contract
        .map(|c| c.front_matter.provider)
        .unwrap_or(conductor::ConductorRunnerProvider::CodingAgentAppServer);
    let request = conductor::ConductorRunnerLaunchRequest {
        provider,
        task_id: task_id_norm,
        run_attempt,
        workflow_id,
        workspace_path: workspace_spec.workspace_path,
        prompt,
    };
    conductor::build_app_server_launch_payload(&request)
}

#[tauri::command]
fn conductor_ingest_runner_event(
    state: tauri::State<AppState>,
    task_id: String,
    raw_event: String,
) -> Result<conductor::ConductorRefreshResponse, String> {
    let repo = state.repo.lock().clone();
    let event = conductor::parse_app_server_event(&raw_event)?;
    let now = unix_now_seconds();
    let contract = conductor::load_workflow_contract(&repo).contract;
    let mut runtime = state.conductor.lock();
    if let conductor::ConductorRunnerEvent::Completed { result_summary, .. } = &event {
        let task_norm = task_id.trim().to_ascii_lowercase();
        if let Some(ir) = runtime
            .issue_runs
            .iter_mut()
            .find(|ir| ir.task.task_id == task_norm)
        {
            let content = if let Some(ref c) = contract {
                let adapter = conductor::HttpRunnerAdapter::from_contract(c);
                match adapter.parse_event(&raw_event) {
                    Ok(run_event) => serde_json::to_string(&run_event)
                        .unwrap_or_else(|_| result_summary.as_deref().unwrap_or("").to_string()),
                    Err(_) => result_summary.as_deref().unwrap_or("").to_string(),
                }
            } else {
                result_summary.as_deref().unwrap_or("").to_string()
            };
            ir.push_artifact(conductor::RunArtifact {
                kind: "result_summary".to_string(),
                content,
                created_at: format!("unix:{now}"),
            });
        }
    }
    let update = conductor::ConductorRunnerUpdate { task_id, event };
    conductor::refresh_runtime(&mut runtime, &[update], now)
}

#[tauri::command]
fn conductor_dispatch_launch(
    state: tauri::State<AppState>,
    task_id: String,
) -> Result<conductor::ConductorRunnerLaunchPayload, String> {
    let repo = state.repo.lock().clone();
    let now = unix_now_seconds();
    let task_id_norm = task_id.trim().to_ascii_lowercase();
    // Extract data under lock, then drop before any IO.
    let (prompt, workflow_id, run_attempt) = {
        let runtime = state.conductor.lock();
        let ir = runtime
            .issue_runs
            .iter()
            .find(|ir| ir.task.task_id == task_id_norm)
            .ok_or_else(|| format!("no issue run for task: {task_id}"))?;
        if ir.is_terminal() {
            return Err(format!("task is already terminal: {task_id}"));
        }
        (
            ir.context.prompt.clone(),
            ir.context.workflow_id.clone(),
            ir.task.run_attempt,
        )
    };
    let workspace_root = repo.join(".sloposcope").join("workspaces");
    let workspace_spec = conductor::ensure_task_workspace(&workspace_root, &task_id_norm)?;
    let session_id = conductor::app_server_session_id(&task_id_norm, run_attempt)?;
    let worker_run = conductor::WorkerRun {
        session_id: session_id.clone(),
        task_id: task_id_norm.clone(),
        run_attempt,
        workspace_path: workspace_spec.workspace_path.clone(),
        launched_at: format!("unix:{now}"),
    };
    let contract = conductor::load_workflow_contract(&repo)
        .contract
        .ok_or_else(|| "WORKFLOW.md not loaded".to_string())?;
    let adapter = conductor::HttpRunnerAdapter::from_contract(&contract);
    adapter.launch(&worker_run, &prompt)?;
    // Re-acquire lock only to record session attachment.
    {
        let mut runtime = state.conductor.lock();
        if let Some(ir) = runtime
            .issue_runs
            .iter_mut()
            .find(|ir| ir.task.task_id == task_id_norm)
        {
            ir.attach_session(session_id.clone());
        }
    }
    let request = conductor::ConductorRunnerLaunchRequest {
        provider: conductor::ConductorRunnerProvider::CodingAgentAppServer,
        task_id: task_id_norm,
        run_attempt,
        workflow_id,
        workspace_path: workspace_spec.workspace_path,
        prompt,
    };
    conductor::build_app_server_launch_payload(&request)
}

#[tauri::command]
fn get_conductor_workflow_contract(
    state: tauri::State<AppState>,
) -> conductor::ConductorWorkflowContractLoad {
    let repo = state.repo.lock().clone();
    conductor::load_workflow_contract(&repo)
}

#[tauri::command]
fn save_conductor_workflow_settings(
    state: tauri::State<AppState>,
    settings: conductor::ConductorWorkflowSettingsUpdate,
) -> Result<conductor::ConductorWorkflowContractLoad, String> {
    let repo = state.repo.lock().clone();
    conductor::save_workflow_settings(&repo, &settings)
}

#[tauri::command]
fn save_watchdog_repo_config(
    state: tauri::State<AppState>,
    config: watchdog_types::WatchdogRepoConfig,
) -> Result<watchdog_types::WatchdogStateView, String> {
    let repo = state.repo.lock().clone();
    watchdog_config::save_repo_config(&repo, &config)?;
    let runtime = state.watchdog.lock();
    Ok(watchdog::build_state_view(&repo, &runtime))
}

fn maybe_run_watchdog_auto_trigger(
    state: &AppState,
    repo: &Path,
    since: &str,
    reason: WatchdogAutoTriggerReason,
    changed_files_total: usize,
    trigger_key: String,
) -> Option<watchdog_types::WatchdogRunTrace> {
    let (config, _) = watchdog_config::load_repo_config(repo);
    let quality_gate = watchdog_auto_trigger_quality_gate();
    if !quality_gate.passed {
        if debug_enabled() {
            println!(
                "[BACKEND][watchdog] auto-trigger blocked by eval quality gate: {}",
                quality_gate.reason
            );
        }
        return None;
    }
    if !config.enabled
        || !should_run_watchdog_auto_trigger(
            &config.trigger_mode,
            reason,
            changed_files_total,
            config.max_files_in_snapshot,
        )
    {
        return None;
    }

    let now = unix_now_seconds();
    let cooldown_secs = watchdog_trigger_cooldown().as_secs().max(1);
    let min_interval_secs = watchdog_auto_trigger_min_interval().as_secs().max(1);
    let mut runtime = state.watchdog.lock();
    let mut conductor_runtime = state.conductor.lock();
    if runtime
        .last_check_at
        .is_some_and(|last| now.saturating_sub(last) < min_interval_secs)
    {
        if debug_enabled() {
            println!(
                "[BACKEND][watchdog] auto-trigger skipped reason={:?} key='{}' min_interval={}s",
                reason, trigger_key, min_interval_secs
            );
        }
        return None;
    }
    if runtime.last_policy_trigger_key.as_deref() == Some(trigger_key.as_str())
        && runtime.last_verdict_hash.is_some()
    {
        if debug_enabled() {
            println!(
                "[BACKEND][watchdog] auto-trigger skipped reason={:?} key='{}' unchanged trigger key",
                reason, trigger_key
            );
        }
        return None;
    }
    if runtime.last_policy_trigger_key.as_deref() == Some(trigger_key.as_str())
        && runtime
            .last_policy_trigger_at
            .is_some_and(|last| now.saturating_sub(last) < cooldown_secs)
    {
        if debug_enabled() {
            println!(
                "[BACKEND][watchdog] auto-trigger skipped reason={:?} key='{}' cooldown={}s",
                reason, trigger_key, cooldown_secs
            );
        }
        return None;
    }

    let trace =
        run_watchdog_check_with_fallback(repo, since, &mut runtime, Some(&mut conductor_runtime));
    runtime.last_policy_trigger_key = Some(trigger_key.clone());
    runtime.last_policy_trigger_at = Some(now);
    if debug_enabled() {
        println!(
            "[BACKEND][watchdog] auto-trigger executed reason={:?} key='{}' trace_id={}",
            reason, trigger_key, trace.id
        );
    }
    Some(trace)
}

// --- File watcher and polling ---

fn refresh_vcs_state(handle: &AppHandle) {
    if let Some(state) = handle.try_state::<AppState>() {
        let started = Instant::now();
        let quiet_period = watcher_vcs_quiet_period();
        let pending_vcs_meta = *state.watcher_pending_vcs_meta.lock();
        let last_vcs_meta_at = *state.watcher_last_vcs_meta_at.lock();
        if should_defer_vcs_refresh(pending_vcs_meta, last_vcs_meta_at, quiet_period) {
            if debug_enabled() {
                let elapsed = last_vcs_meta_at
                    .map(|t| t.elapsed().as_millis().to_string())
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "[BACKEND][watcher] refresh_vcs_state deferred pending_meta={} last_meta_event_age_ms={} quiet_ms={}",
                    pending_vcs_meta,
                    elapsed,
                    quiet_period.as_millis()
                );
            }
            return;
        }

        let repo = state.repo.lock().clone();
        let vcs_probe_started = Instant::now();
        let current_branch = vcs::get_current_branch(&repo);
        let current_revision = vcs::get_current_revision(&repo);
        let vcs_probe_ms = vcs_probe_started.elapsed().as_millis();
        let last_branch = state.last_branch.lock().clone();
        let last_revision = state.last_revision.lock().clone();
        let branch_changed = current_branch != last_branch;
        let revision_changed = current_revision != last_revision;

        if !branch_changed && !revision_changed {
            if debug_enabled() && vcs_probe_ms >= 40 {
                println!(
                    "[BACKEND][watcher] refresh_vcs_state no-op branch='{}' rev='{}' vcs_probe={}ms total={}ms",
                    current_branch,
                    short_rev_label(&current_revision),
                    vcs_probe_ms,
                    started.elapsed().as_millis()
                );
            }
            return;
        }

        if debug_enabled() {
            println!(
                "[BACKEND][watcher] VCS state changed: branch '{}' -> '{}', rev '{}' -> '{}'. Rebuilding graph.",
                last_branch,
                current_branch,
                short_rev_label(last_revision.as_str()),
                short_rev_label(&current_revision),
            );
        }

        let previous_request = state.last_graph_view_request.lock().clone();
        let next_request = if branch_changed
            && matches!(previous_request.seed, Some(GraphViewSeed::Commit { .. }))
        {
            if debug_enabled() {
                println!(
                    "[BACKEND][watcher] branch changed, resetting graph view request to working copy"
                );
            }
            GraphViewRequest::default_working_copy()
        } else {
            previous_request
        };
        let build_started = Instant::now();
        if let Err(err) = rebuild_canonical_graph_store(&state.parsers, &repo) {
            println!(
                "[BACKEND][watcher] canonical rebuild failed during VCS refresh: {}",
                err
            );
            return;
        }
        let rebuilt = match ensure_graph_view(&state, &next_request) {
            Ok(view) => view,
            Err(err) => {
                println!(
                    "[BACKEND][watcher] graph view derivation failed during VCS refresh: {}",
                    err
                );
                return;
            }
        };
        let build_ms = build_started.elapsed().as_millis();
        apply_graph_view_state(
            &state,
            &next_request,
            &rebuilt,
            current_branch.clone(),
            current_revision.clone(),
        );

        let bookmarks_started = Instant::now();
        let bookmarks = vcs::get_bookmarks(&repo);
        let bookmarks_ms = bookmarks_started.elapsed().as_millis();
        let emit_started = Instant::now();
        let graph_snapshot = rebuilt.graph.clone();
        let emit_result = handle.emit(
            "graph-updated",
            serde_json::json!({
                "graph": graph_snapshot,
                "graph_view": rebuilt,
                "bookmarks": bookmarks,
                "current_branch": current_branch,
                "current_revision": current_revision,
                "since": graph_view_since_hint(&next_request).unwrap_or_else(|| "@".to_string()),
                "since_reset": branch_changed && next_request.seed == Some(GraphViewSeed::WorkingCopy),
            }),
        );
        let emit_ms = emit_started.elapsed().as_millis();
        schedule_cluster_refresh(handle, Duration::from_millis(20));
        if branch_changed {
            let since_for_trigger =
                graph_view_since_hint(&next_request).unwrap_or_else(|| "@".to_string());
            let trigger_key = format!(
                "branch-switch:{}:{}",
                current_branch,
                state.last_revision.lock().clone()
            );
            if let Some(trace) = maybe_run_watchdog_auto_trigger(
                &state,
                &repo,
                &since_for_trigger,
                WatchdogAutoTriggerReason::BranchSwitchAfterQuietPeriod,
                0,
                trigger_key,
            ) {
                let _ = handle.emit(
                    "watchdog-auto-check",
                    serde_json::json!({
                        "reason": "on_branch_switch_after_quiet_period",
                        "trace": trace,
                    }),
                );
            }
        }
        if debug_enabled() {
            println!(
                "[BACKEND][watcher] refresh_vcs_state done build={}ms bookmarks={}ms emit={}ms vcs_probe={}ms total={}ms",
                build_ms,
                bookmarks_ms,
                emit_ms,
                vcs_probe_ms,
                started.elapsed().as_millis()
            );
            if let Err(err) = &emit_result {
                println!("[BACKEND][watcher] graph-updated emit failed: {}", err);
            }
        }
    }
}

fn queue_watcher_batch(
    handle: &AppHandle,
    vcs_meta_changed: bool,
    interesting_paths: Vec<PathBuf>,
) {
    if !vcs_meta_changed && interesting_paths.is_empty() {
        return;
    }

    if let Some(state) = handle.try_state::<AppState>() {
        if vcs_meta_changed {
            *state.watcher_pending_vcs_meta.lock() = true;
            *state.watcher_last_vcs_meta_at.lock() = Some(Instant::now());
        }
        if !interesting_paths.is_empty() {
            state.watcher_pending_paths.lock().extend(interesting_paths);
        }
    }

    schedule_watcher_flush(handle, watcher_debounce_interval());
}

fn schedule_watcher_flush(handle: &AppHandle, wait: Duration) {
    let should_schedule = if let Some(state) = handle.try_state::<AppState>() {
        let mut scheduled = state.watcher_flush_scheduled.lock();
        if *scheduled {
            return;
        }
        *scheduled = true;
        true
    } else {
        false
    };

    if should_schedule {
        let handle_clone = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(wait);
            flush_watcher_batch(&handle_clone);
        });
    }
}

fn flush_watcher_batch(handle: &AppHandle) {
    let (pending_vcs_meta, pending_paths) = if let Some(state) = handle.try_state::<AppState>() {
        let pending_vcs_meta = *state.watcher_pending_vcs_meta.lock();
        *state.watcher_pending_vcs_meta.lock() = false;

        let mut unique = HashSet::new();
        let mut pending_paths = Vec::new();
        for path in state.watcher_pending_paths.lock().drain(..) {
            let key = path.display().to_string();
            if unique.insert(key) {
                pending_paths.push(path);
            }
        }

        *state.watcher_flush_scheduled.lock() = false;
        (pending_vcs_meta, pending_paths)
    } else {
        return;
    };

    // During branch/checkout operations, prioritize single state refresh over noisy incremental events.
    if pending_vcs_meta {
        let quiet_period = watcher_vcs_quiet_period();
        let last_vcs_meta_at = if let Some(state) = handle.try_state::<AppState>() {
            *state.watcher_last_vcs_meta_at.lock()
        } else {
            None
        };

        if should_defer_vcs_refresh(false, last_vcs_meta_at, quiet_period) {
            let remaining = last_vcs_meta_at
                .map(|last| quiet_period.saturating_sub(last.elapsed()))
                .unwrap_or(quiet_period);
            if let Some(state) = handle.try_state::<AppState>() {
                *state.watcher_pending_vcs_meta.lock() = true;
            }
            if debug_enabled() {
                println!(
                    "[BACKEND][watcher] checkout still noisy; deferring VCS refresh by {}ms",
                    remaining.as_millis()
                );
            }
            schedule_watcher_flush(handle, remaining.max(Duration::from_millis(60)));
            return;
        }

        if let Some(state) = handle.try_state::<AppState>() {
            *state.watcher_last_vcs_meta_at.lock() = None;
        }
        refresh_vcs_state(handle);
        return;
    }

    if pending_paths.is_empty() {
        return;
    }

    if let Some(state) = handle.try_state::<AppState>() {
        let repo = state.repo.lock().clone();
        let since_filter = state
            .last_since
            .lock()
            .clone()
            .unwrap_or_else(|| "@".to_string());
        let no_vcs_mode = matches!(vcs::detect_engine(&repo), vcs::VCSEngine::None);
        let changed_statuses = if no_vcs_mode {
            HashMap::new()
        } else {
            vcs::get_changed_files(&repo, &since_filter)
        };

        if debug_enabled() {
            println!(
                "[BACKEND][watcher] flush batch paths={} changed_set={} since={} no_vcs={}",
                pending_paths.len(),
                changed_statuses.len(),
                since_filter,
                no_vcs_mode
            );
        }

        if !no_vcs_mode {
            let changed_set_hash = changed_set_fingerprint(&changed_statuses);
            let touched_paths_hash = pending_paths_fingerprint(&pending_paths);
            let trigger_key = format!(
                "large-batch:{}:{}:{}:{}:{}",
                since_filter,
                state.last_revision.lock().clone(),
                changed_statuses.len(),
                changed_set_hash,
                touched_paths_hash
            );
            if let Some(trace) = maybe_run_watchdog_auto_trigger(
                &state,
                &repo,
                &since_filter,
                WatchdogAutoTriggerReason::LargeChangeBatch,
                changed_statuses.len(),
                trigger_key,
            ) {
                let _ = handle.emit(
                    "watchdog-auto-check",
                    serde_json::json!({
                        "reason": "on_large_change_batch",
                        "trace": trace,
                    }),
                );
            }
        }

        let mut touched_count = 0usize;
        let mut canonical_changed = false;

        for path in &pending_paths {
            let Ok(rel_path_buf) = path.strip_prefix(&repo) else {
                continue;
            };
            let rel_path = rel_path_buf.display().to_string().replace('\\', "/");
            if rel_path.is_empty() || is_ignored_runtime_path(&rel_path) {
                continue;
            }

            let update_result = if !path.exists() || !path.is_file() {
                SqliteGraphStore::open_or_create(&repo)
                    .and_then(|store| store.remove_file(&rel_path))
            } else {
                update_canonical_graph_for_file(&state.parsers, &repo, &rel_path)
            };

            match update_result {
                Ok(()) => {
                    canonical_changed = true;
                    let _ = handle.emit("file-touched", rel_path);
                    touched_count += 1;
                }
                Err(err) => {
                    println!(
                        "[BACKEND][watcher] canonical graph update failed for {}: {}",
                        rel_path, err
                    );
                }
            }
        }

        if canonical_changed {
            let _ = ClusterService::mark_stale(&repo);
            let current_branch = vcs::get_current_branch(&repo);
            let current_revision = vcs::get_current_revision(&repo);
            emit_current_graph_view(
                handle,
                &state,
                &repo,
                current_branch,
                current_revision,
                since_filter.clone(),
                false,
            );

            if touched_count >= cluster_refresh_threshold() {
                schedule_cluster_refresh(handle, Duration::from_millis(1800));
            }
        }

        if debug_enabled() {
            println!(
                "[BACKEND][watcher] flush done touched={} changed={} pending_paths={}",
                touched_count,
                canonical_changed,
                pending_paths.len()
            );
        }
    }
}

fn setup_watcher(app: &AppHandle) -> Option<RecommendedWatcher> {
    if debug_enabled() {
        println!("[BACKEND][watcher] setup start");
    }
    let handle = app.clone();

    let mut watcher = match notify::recommended_watcher(move |res: Result<Event, _>| {
        if let Ok(event) = res {
            if let Some(state) = handle.try_state::<AppState>() {
                let active_since = state
                    .last_since
                    .lock()
                    .clone()
                    .unwrap_or_else(|| "@".to_string());

                let is_vcs_meta_path = |p: &PathBuf| {
                    let s = p.display().to_string();
                    s.contains("/.git/")
                        || s.ends_with("/.git")
                        || s.contains("/.jj/")
                        || s.ends_with("/.jj")
                };
                let is_ignored_runtime_path = |p: &PathBuf| {
                    let s = p.display().to_string();
                    s.contains("/_build/")
                        || s.contains("/deps/")
                        || s.contains("/dist/")
                        || s.contains("/.svelte-kit/")
                        || s.contains("/.output/")
                        || s.contains("/node_modules/")
                        || s.contains("/target/")
                        || s.contains("/src-tauri/gen/")
                };
                let is_file_content_event = matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                );

                let vcs_meta_changed = event.paths.iter().any(&is_vcs_meta_path);
                if vcs_meta_changed {
                    queue_watcher_batch(&handle, true, Vec::new());
                    return;
                }
                if !is_file_content_event {
                    return;
                }
                if active_since != "@" {
                    // Ignore live watcher noise when user is viewing historical revsets.
                    return;
                }

                let interesting_paths: Vec<PathBuf> = event
                    .paths
                    .iter()
                    .filter(|p| !is_vcs_meta_path(p) && !is_ignored_runtime_path(p))
                    .cloned()
                    .collect();
                if interesting_paths.is_empty() {
                    return;
                }

                // Defer changed-set filtering to flush (batched) to avoid expensive VCS calls per raw event.
                queue_watcher_batch(&handle, false, interesting_paths);
            }
        }
    }) {
        Ok(w) => w,
        Err(_) => return None,
    };

    if let Some(state) = app.try_state::<AppState>() {
        let repo_path = state.repo.lock().clone();
        let _ = watcher.watch(&repo_path, RecursiveMode::Recursive);
        if debug_enabled() {
            println!(
                "[BACKEND][watcher] watching repo={} recursively",
                repo_path.display()
            );
        }
    }

    if debug_enabled() {
        println!("[BACKEND][watcher] setup complete");
    }
    Some(watcher)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    fn mk_node(id: &str, file: &str) -> Node {
        Node {
            id: id.to_string(),
            label: id.to_string(),
            kind: "file".to_string(),
            file: file.to_string(),
            line_count: 1,
            change_status: "modified".to_string(),
            functions: vec![],
        }
    }

    #[test]
    fn resolve_graph_edges_for_sources_only_updates_selected_sources() {
        let mut g = Graph::new();
        g.add_nodes(vec![
            mk_node("src/a.rs", "src/a.rs"),
            mk_node("src/b.rs", "src/b.rs"),
            mk_node("src/c.rs", "src/c.rs"),
        ]);
        g.add_edges(vec![
            graph::Edge {
                source: "src/a.rs".into(),
                target: "b".into(),
                kind: "use".into(),
            },
            graph::Edge {
                source: "src/c.rs".into(),
                target: "b".into(),
                kind: "use".into(),
            },
        ]);

        let mut changed_sources = HashSet::new();
        changed_sources.insert("src/a.rs".to_string());
        resolve_graph_edges_for_sources(&mut g, &changed_sources);

        assert_eq!(g.edges[0].target, "src/b.rs");
        assert_eq!(g.edges[1].target, "b");
    }

    #[test]
    fn resolve_graph_edges_empty_source_set_updates_all() {
        let mut g = Graph::new();
        g.add_nodes(vec![
            mk_node("src/a.rs", "src/a.rs"),
            mk_node("src/b.rs", "src/b.rs"),
        ]);
        g.add_edges(vec![graph::Edge {
            source: "src/a.rs".into(),
            target: "b".into(),
            kind: "use".into(),
        }]);

        let all_sources = HashSet::new();
        resolve_graph_edges_for_sources(&mut g, &all_sources);
        assert_eq!(g.edges[0].target, "src/b.rs");
    }

    #[test]
    fn vcs_refresh_deferred_when_meta_pending_or_recent() {
        let quiet = Duration::from_millis(500);
        assert!(should_defer_vcs_refresh(true, None, quiet));
        assert!(should_defer_vcs_refresh(false, Some(Instant::now()), quiet));

        let stale = Instant::now()
            .checked_sub(Duration::from_millis(700))
            .expect("instant subtraction must succeed");
        assert!(!should_defer_vcs_refresh(false, Some(stale), quiet));
    }

    #[test]
    fn auto_trigger_mode_matches_branch_switch_policy() {
        assert!(should_run_watchdog_auto_trigger(
            &watchdog_types::WatchdogTriggerMode::OnBranchSwitchAfterQuietPeriod,
            WatchdogAutoTriggerReason::BranchSwitchAfterQuietPeriod,
            0,
            20
        ));
        assert!(!should_run_watchdog_auto_trigger(
            &watchdog_types::WatchdogTriggerMode::OnLargeChangeBatch,
            WatchdogAutoTriggerReason::BranchSwitchAfterQuietPeriod,
            40,
            20
        ));
    }

    #[test]
    fn auto_trigger_mode_matches_large_batch_threshold() {
        assert!(!should_run_watchdog_auto_trigger(
            &watchdog_types::WatchdogTriggerMode::OnLargeChangeBatch,
            WatchdogAutoTriggerReason::LargeChangeBatch,
            19,
            20
        ));
        assert!(should_run_watchdog_auto_trigger(
            &watchdog_types::WatchdogTriggerMode::OnLargeChangeBatch,
            WatchdogAutoTriggerReason::LargeChangeBatch,
            20,
            20
        ));
        assert!(!should_run_watchdog_auto_trigger(
            &watchdog_types::WatchdogTriggerMode::ManualOnly,
            WatchdogAutoTriggerReason::LargeChangeBatch,
            100,
            20
        ));
    }

    #[test]
    fn output_tail_keeps_last_lines() {
        let stdout = b"line1\nline2\nline3\nline4\nline5\n";
        let stderr = b"err1\nerr2\n";
        let tail = output_tail(stdout, stderr, 3, 500);
        assert_eq!(tail, "line5\nerr1\nerr2");
    }

    #[test]
    fn output_tail_caps_chars() {
        let stdout = b"abcdefghijklmnopqrstuvwxyz";
        let tail = output_tail(stdout, b"", 20, 10);
        assert_eq!(tail, "abcdefghij");
    }

    #[test]
    fn parse_watchdog_severity_label_supports_aliases() {
        assert_eq!(
            parse_watchdog_severity_label("info"),
            Some(watchdog_types::WatchdogSeverity::Info)
        );
        assert_eq!(
            parse_watchdog_severity_label("warn"),
            Some(watchdog_types::WatchdogSeverity::Warning)
        );
        assert_eq!(
            parse_watchdog_severity_label("ERROR"),
            Some(watchdog_types::WatchdogSeverity::Critical)
        );
        assert_eq!(parse_watchdog_severity_label("unknown"), None);
    }

    #[test]
    fn strict_mode_provider_failure_records_blocked_trace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut runtime = watchdog::WatchdogRuntimeState::default();
        let trace = run_watchdog_strict_mode_blocked_check(
            dir.path(),
            "@",
            &mut runtime,
            &conductor::ConductorRunnerProvider::OpenAi,
            "simulated provider failure",
        );

        assert_eq!(
            trace.verdict.status,
            watchdog_types::WatchdogStatus::Blocked
        );
        assert_eq!(trace.verdict.issues.len(), 1);
        assert_eq!(
            trace.verdict.issues[0].severity,
            watchdog_types::WatchdogSeverity::Critical
        );
        assert!(trace.verdict.summary.contains("strict mode"));
        assert_eq!(runtime.last_verdict, Some(trace.verdict.clone()));
        assert!(!runtime.alerts.is_empty());
    }

    fn init_git_repo_with_file(path: &Path, rel_path: &str, content: &str) -> MissionRecord {
        std::fs::create_dir_all(path).expect("create repo dir");
        if let Some(parent) = path.join(rel_path).parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(path.join(rel_path), content).expect("seed file");
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .expect("git init");
        std::process::Command::new("git")
            .args(["config", "user.email", "slopo@example.test"])
            .current_dir(path)
            .output()
            .expect("git email");
        std::process::Command::new("git")
            .args(["config", "user.name", "Sloposcope Test"])
            .current_dir(path)
            .output()
            .expect("git name");
        std::process::Command::new("git")
            .args(["add", rel_path])
            .current_dir(path)
            .output()
            .expect("git add");
        std::process::Command::new("git")
            .args(["commit", "-m", "seed"])
            .current_dir(path)
            .output()
            .expect("git commit");

        MissionRecord {
            id: "mission-test".into(),
            repo_path: path.display().to_string(),
            worktree_path: path.display().to_string(),
            graph_snapshot: current_repo_revision(path),
            title: "Mission".into(),
            brief: "Test".into(),
            validation_command: "cargo test".into(),
            planner_notes: None,
            planner_profile_id: None,
            planner_prompt_override: None,
            issue_key: None,
            issue_url: None,
            instruction_pack_hash: "abc123".into(),
            state: "running".into(),
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        }
    }

    #[test]
    fn apply_patch_text_updates_clean_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mission = init_git_repo_with_file(dir.path(), "app.js", "export const answer = 1;\n");
        ensure_apply_freshness(
            dir.path(),
            &mission,
            &["app.js".to_string()],
            &mission.graph_snapshot,
        )
        .expect("freshness");
        let patch = "diff --git a/app.js b/app.js\nindex 863ac03..3da4fa4 100644\n--- a/app.js\n+++ b/app.js\n@@ -1 +1 @@\n-export const answer = 1;\n+export const answer = 2;\n";
        apply_patch_text(dir.path(), &mission.id, "objective-1", patch).expect("apply patch");
        let updated = std::fs::read_to_string(dir.path().join("app.js")).expect("read file");
        assert_eq!(updated, "export const answer = 2;\n");
    }

    #[test]
    fn ensure_apply_freshness_rejects_stale_revision() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mission = init_git_repo_with_file(dir.path(), "app.js", "export const answer = 1;\n");
        std::fs::write(dir.path().join("app.js"), "export const answer = 3;\n")
            .expect("mutate working tree");
        let err = ensure_apply_freshness(dir.path(), &mission, &["app.js".to_string()], "deadbeef")
            .expect_err("stale revision should fail");
        assert!(err.contains("stale map"));
    }

    #[test]
    fn patch_gate_is_ready_for_valid_in_scope_patch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mission =
            init_git_repo_with_file(dir.path(), "src/app.js", "export const answer = 1;\n");
        let objective = work_store::ObjectiveRecord {
            id: "objective-1".into(),
            mission_id: mission.id.clone(),
            title: "Objective".into(),
            prompt: "Implement".into(),
            status: WorkUnitStatus::Done,
            claimed_by: Some("worker".into()),
            claimed_at: Some("unix:2".into()),
            created_at: "unix:1".into(),
            meta: work_store::ObjectiveMeta {
                scope_paths: vec!["src/".into()],
                expected_validation: vec!["cargo test".into()],
                done_criteria: vec!["done".into()],
                test_intent: Some("green".into()),
            },
        };
        let session = AgentSessionRuntimeRecord {
            id: "sess-1".into(),
            mission_id: mission.id.clone(),
            objective_id: Some(objective.id.clone()),
            profile_id: "local-code-worker".into(),
            assignment_kind: "execute_objective".into(),
            state: "completed".into(),
            touched_paths: vec!["src/app.js".into()],
            workspace_path: "/tmp/worktree".into(),
            lease_expires_at: None,
            last_heartbeat_at: Some("unix:3".into()),
            last_event_at: Some("unix:3".into()),
            last_error: None,
            created_at: "unix:2".into(),
            updated_at: "unix:3".into(),
        };
        let verification = VerificationRecord {
            id: 1,
            mission_id: mission.id.clone(),
            objective_id: Some(objective.id.clone()),
            session_id: Some(session.id.clone()),
            status: "passed".into(),
            summary: "ok".into(),
            details: serde_json::json!({
                "patch": "diff --git a/src/app.js b/src/app.js",
                "validation": { "success": true }
            }),
            created_at: "unix:3".into(),
            updated_at: "unix:3".into(),
        };
        let gate = evaluate_patch_gate_for_objective(
            dir.path(),
            &mission,
            &objective,
            &session,
            &verification,
        );
        assert_eq!(gate.status, "ready");
        assert!(gate.reasons.is_empty());
    }

    #[test]
    fn mission_view_frontend_exposes_session_runtime_slice() {
        let mission = MissionRecord {
            id: "mission-runtime".into(),
            repo_path: "/repo".into(),
            worktree_path: "/repo".into(),
            graph_snapshot: "@".into(),
            title: "Mission".into(),
            brief: "Track runtime".into(),
            validation_command: "cargo test".into(),
            planner_notes: None,
            planner_profile_id: None,
            planner_prompt_override: None,
            issue_key: None,
            issue_url: None,
            instruction_pack_hash: "abc123".into(),
            state: "running".into(),
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };
        let view = MissionStateView {
            mission: Some(mission),
            objectives: Vec::new(),
            sessions: vec![AgentSessionRuntimeRecord {
                id: "sess-runtime".into(),
                mission_id: "mission-runtime".into(),
                objective_id: Some("objective-1".into()),
                profile_id: "worker-1".into(),
                assignment_kind: "execute_objective".into(),
                state: "running".into(),
                touched_paths: vec!["src/lib.rs".into()],
                workspace_path: "/tmp/worktree".into(),
                lease_expires_at: Some("unix:9".into()),
                last_heartbeat_at: Some("unix:3".into()),
                last_event_at: Some("unix:4".into()),
                last_error: None,
                created_at: "unix:2".into(),
                updated_at: "unix:4".into(),
            }],
            profiles: Vec::new(),
            patch_decisions: Vec::new(),
            patch_gates: Vec::new(),
            verifications: Vec::new(),
            events: Vec::new(),
            summary: work_store::MissionSummary {
                total_objectives: 0,
                pending_objectives: 0,
                claimed_objectives: 0,
                done_objectives: 0,
                failed_objectives: 0,
                active_sessions: 1,
                last_event_at: Some("unix:4".into()),
            },
        };

        let payload = mission_view_to_frontend_at(view, 10);
        let session = &payload["sessions"][0];
        assert_eq!(session["runtimeStatus"], "stuck");
        assert_eq!(session["heartbeatAgeSec"], 7);
        assert_eq!(session["eventAgeSec"], 6);
        assert_eq!(session["leaseExpired"], true);
    }

    #[test]
    fn resolve_requested_agent_profile_prefers_explicit_and_rejects_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = WorkStore::open_or_create(dir.path()).expect("store");
        store
            .upsert_agent_profile(&AgentProfile {
                id: "internal-default".into(),
                label: "Internal".into(),
                provider: "internal".into(),
                model: None,
                command: String::new(),
                args: Vec::new(),
                env: serde_json::json!({}),
                capabilities: vec!["can_edit".into()],
                transport_kind: "internal".into(),
                active: true,
                created_at: "unix:1".into(),
                updated_at: "unix:1".into(),
            })
            .expect("insert active");
        store
            .upsert_agent_profile(&AgentProfile {
                id: "cli-selected".into(),
                label: "CLI".into(),
                provider: "local_cli".into(),
                model: Some("shell".into()),
                command: "/bin/sh".into(),
                args: vec!["-lc".into(), "echo ok".into()],
                env: serde_json::json!({}),
                capabilities: vec!["can_edit".into()],
                transport_kind: "pty".into(),
                active: false,
                created_at: "unix:2".into(),
                updated_at: "unix:2".into(),
            })
            .expect("insert selected");

        let active = resolve_requested_agent_profile(&store, None)
            .expect("active profile")
            .expect("active exists");
        assert_eq!(active.id, "internal-default");

        let explicit = resolve_requested_agent_profile(&store, Some("cli-selected"))
            .expect("explicit profile")
            .expect("explicit exists");
        assert_eq!(explicit.id, "cli-selected");

        let err = resolve_requested_agent_profile(&store, Some("missing-profile"))
            .expect_err("missing explicit profile must fail");
        assert!(err.contains("Agent profile not found"));
    }

    #[test]
    fn resolve_mission_planner_profile_accepts_pi_cli_as_nim_backed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = WorkStore::open_or_create(dir.path()).expect("store");
        store
            .upsert_agent_profile(&AgentProfile {
                id: "pi-planner".into(),
                label: "PI Planner".into(),
                provider: "pi_cli".into(),
                model: Some("openai/gpt-oss-20b".into()),
                command: "pi".into(),
                args: Vec::new(),
                env: serde_json::json!({
                    "NVIDIA_API_KEY": "nim-key"
                }),
                capabilities: vec!["can_plan".into()],
                transport_kind: "pipe".into(),
                active: true,
                created_at: "unix:1".into(),
                updated_at: "unix:1".into(),
            })
            .expect("insert pi planner");

        let planner = resolve_mission_planner_profile(&store, Some("pi-planner"))
            .expect("planner profile")
            .expect("planner exists");
        assert_eq!(planner.id, "pi-planner");
        assert_eq!(planner.provider, "pi_cli");
    }

    #[test]
    fn profiled_cli_worker_edits_isolated_worktree_and_captures_patch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut mission =
            init_git_repo_with_file(dir.path(), "app.js", "export const answer = 1;\n");
        mission.validation_command = "test \"$(cat app.js)\" = \"export const answer = 2;\"".into();
        let objective = work_store::ObjectiveRecord {
            id: "objective-cli".into(),
            mission_id: mission.id.clone(),
            title: "Objective".into(),
            prompt: "Implement".into(),
            status: WorkUnitStatus::Claimed,
            claimed_by: Some("worker".into()),
            claimed_at: Some("unix:2".into()),
            created_at: "unix:1".into(),
            meta: work_store::ObjectiveMeta {
                scope_paths: vec!["app.js".into()],
                expected_validation: vec![],
                done_criteria: vec!["done".into()],
                test_intent: Some("green".into()),
            },
        };
        let profile = AgentProfile {
            id: "local-cli".into(),
            label: "Local CLI".into(),
            provider: "local_cli".into(),
            model: Some("shell".into()),
            command: "/bin/sh".into(),
            args: vec![
                "-lc".into(),
                "printf 'export const answer = 2;\\n' > app.js".into(),
            ],
            env: serde_json::json!({}),
            capabilities: vec!["can_edit".into()],
            transport_kind: "pty".into(),
            active: true,
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };

        let result =
            execute_profiled_code_worker(dir.path(), &mission, &objective, "session-cli", &profile)
                .expect("run profiled cli worker");
        assert_eq!(result.profile_id, "local-cli");
        assert_eq!(result.worker_kind, "cli_worker");
        assert_eq!(result.touched_paths, vec!["app.js"]);
        assert!(result.patch_text.contains("+export const answer = 2;"));
        assert!(result.validation.success);
    }

    #[test]
    fn profiled_cli_worker_harness_writes_bundle_and_reports_agent_events() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut mission =
            init_git_repo_with_file(dir.path(), "app.js", "export const answer = 1;\n");
        mission.validation_command = "test \"$(cat app.js)\" = \"export const answer = 2;\"".into();
        let objective = work_store::ObjectiveRecord {
            id: "objective-harness".into(),
            mission_id: mission.id.clone(),
            title: "Harness objective".into(),
            prompt: "Implement with harness".into(),
            status: WorkUnitStatus::Claimed,
            claimed_by: Some("worker".into()),
            claimed_at: Some("unix:2".into()),
            created_at: "unix:1".into(),
            meta: work_store::ObjectiveMeta {
                scope_paths: vec!["app.js".into()],
                expected_validation: vec![],
                done_criteria: vec!["done".into()],
                test_intent: Some("outer_red".into()),
            },
        };
        let profile = AgentProfile {
            id: "local-cli-harness".into(),
            label: "Local CLI Harness".into(),
            provider: "local_cli".into(),
            model: Some("shell".into()),
            command: "/bin/sh".into(),
            args: vec![
                "-lc".into(),
                "cp {prompt_path} prompt_seen.md && printf 'export const answer = 2;\\n' > app.js"
                    .into(),
            ],
            env: serde_json::json!({}),
            capabilities: vec!["can_edit".into()],
            transport_kind: "pty".into(),
            active: true,
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };
        let mut store = WorkStore::open_or_create(dir.path()).expect("store");
        store.upsert_mission(&mission).expect("upsert mission");
        store
            .plan_task(
                &mission.id,
                &mission.title,
                &mission.brief,
                &[work_store::NewWorkUnit {
                    id: objective.id.clone(),
                    title: objective.title.clone(),
                    prompt: objective.prompt.clone(),
                }],
            )
            .expect("plan task");
        drop(store);

        let result = execute_profiled_code_worker(
            dir.path(),
            &mission,
            &objective,
            "session-harness",
            &profile,
        )
        .expect("run profiled cli worker with harness");
        assert!(result.validation.success);
        let prompt_seen =
            std::fs::read_to_string(Path::new(&result.workspace_path).join("prompt_seen.md"))
                .expect("prompt file copied");
        assert!(prompt_seen.contains("Mission: Mission"));
        assert!(prompt_seen.contains("agent_login"));
        assert!(prompt_seen.contains("agent_request_context"));

        let diagnostics = result.diagnostics.expect("diagnostics");
        let context_path = diagnostics
            .get("contextPath")
            .and_then(serde_json::Value::as_str)
            .expect("contextPath");
        assert!(Path::new(context_path).is_file());
        let bundle_dir = diagnostics
            .get("bundleDir")
            .and_then(serde_json::Value::as_str)
            .expect("bundleDir");
        assert!(Path::new(bundle_dir).is_dir());

        let store = WorkStore::open_or_create(dir.path()).expect("store");
        let events = store
            .get_run_events(&mission.id, Some(&objective.id), 20)
            .expect("run events");
        let event_types = events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>();
        assert!(event_types.contains(&"process_started"));
        assert!(event_types.contains(&"process_exited"));
        assert!(event_types.contains(&"touched_files"));
        let verification = store
            .get_latest_verification_for_objective(&mission.id, &objective.id)
            .expect("latest verification")
            .expect("verification exists");
        assert_eq!(verification.session_id.as_deref(), Some("session-harness"));
        assert_eq!(
            verification
                .details
                .get("submittedByAgent")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            verification
                .details
                .get("validation")
                .and_then(|value| value.get("success"))
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn profiled_cli_worker_uses_harness_validation_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut mission =
            init_git_repo_with_file(dir.path(), "app.js", "export const answer = 1;\n");
        mission.validation_command = "echo mission-should-not-run".into();
        let objective = work_store::ObjectiveRecord {
            id: "objective-validation-once".into(),
            mission_id: mission.id.clone(),
            title: "Validation once".into(),
            prompt: "Edit app".into(),
            status: WorkUnitStatus::Claimed,
            claimed_by: Some("worker".into()),
            claimed_at: Some("unix:2".into()),
            created_at: "unix:1".into(),
            meta: work_store::ObjectiveMeta {
                scope_paths: vec!["app.js".into()],
                expected_validation: vec![
                    "count=$(cat .validate-count 2>/dev/null || echo 0); next=$((count+1)); printf '%s' \"$next\" > .validate-count; test \"$next\" = \"1\"".into(),
                ],
                done_criteria: vec!["done".into()],
                test_intent: Some("outer_red".into()),
            },
        };
        let profile = AgentProfile {
            id: "local-cli-validate-once".into(),
            label: "Local CLI Validate Once".into(),
            provider: "local_cli".into(),
            model: Some("shell".into()),
            command: "/bin/sh".into(),
            args: vec![
                "-lc".into(),
                "printf 'export const answer = 2;\\n' > app.js".into(),
            ],
            env: serde_json::json!({}),
            capabilities: vec!["can_edit".into()],
            transport_kind: "pty".into(),
            active: true,
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };

        let result = execute_profiled_code_worker(
            dir.path(),
            &mission,
            &objective,
            "session-validate-once",
            &profile,
        )
        .expect("run profiled cli worker");
        assert!(result.validation.success);
        assert_eq!(
            std::fs::read_to_string(Path::new(&result.workspace_path).join(".validate-count"))
                .expect("validation count"),
            "1"
        );
    }

    #[test]
    fn profiled_opencode_worker_writes_local_config_and_runs_opencode_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut mission =
            init_git_repo_with_file(dir.path(), "app.js", "export const answer = 1;\n");
        mission.validation_command = "test \"$(cat app.js)\" = \"export const answer = 2;\"".into();
        let objective = work_store::ObjectiveRecord {
            id: "objective-opencode".into(),
            mission_id: mission.id.clone(),
            title: "OpenCode objective".into(),
            prompt: "Implement with opencode".into(),
            status: WorkUnitStatus::Claimed,
            claimed_by: Some("worker".into()),
            claimed_at: Some("unix:2".into()),
            created_at: "unix:1".into(),
            meta: work_store::ObjectiveMeta {
                scope_paths: vec!["app.js".into()],
                expected_validation: vec![],
                done_criteria: vec!["done".into()],
                test_intent: Some("outer_red".into()),
            },
        };
        let fake_opencode = dir.path().join("fake-opencode.sh");
        std::fs::write(
            &fake_opencode,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > opencode-argv.txt\nprintf '%s' \"$OPENCODE_CONFIG\" > opencode-config-path.txt\ncp \"$OPENCODE_CONFIG\" opencode-config-seen.json\nprintf 'export const answer = 2;\\n' > app.js\n",
        )
        .expect("write fake opencode");
        assert!(std::process::Command::new("chmod")
            .arg("+x")
            .arg(&fake_opencode)
            .status()
            .expect("chmod")
            .success());
        let profile = AgentProfile {
            id: "opencode-cli".into(),
            label: "OpenCode CLI".into(),
            provider: "opencode_cli".into(),
            model: Some("meta/test-model".into()),
            command: fake_opencode.display().to_string(),
            args: vec!["--format".into(), "json".into()],
            env: serde_json::json!({
                "NVIDIA_API_KEY": "nim-key",
                "NVIDIA_NIM_BASE_URL": "https://nim.example.test/v1"
            }),
            capabilities: vec!["can_edit".into()],
            transport_kind: "pipe".into(),
            active: true,
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };

        let result = execute_profiled_code_worker(
            dir.path(),
            &mission,
            &objective,
            "session-opencode",
            &profile,
        )
        .expect("run profiled opencode worker");
        assert!(result.validation.success);
        let workspace = Path::new(&result.workspace_path);
        let argv = std::fs::read_to_string(workspace.join("opencode-argv.txt")).expect("argv");
        assert!(argv.contains("run"));
        assert!(argv.contains("--model"));
        assert!(argv.contains("sloposcope_nim/meta/test-model"));
        assert!(argv.contains("Mission: Mission"));
        assert!(argv.contains("--format"));
        assert!(argv.contains("json"));
        assert_eq!(
            std::fs::read_to_string(workspace.join("opencode-config-path.txt"))
                .expect("config path"),
            workspace
                .join(".sloposcope")
                .join("opencode.json")
                .display()
                .to_string()
        );
        let config: serde_json::Value = serde_json::from_slice(
            &std::fs::read(workspace.join("opencode-config-seen.json")).expect("config"),
        )
        .expect("json");
        assert_eq!(
            config
                .get("mcp")
                .and_then(|value| value.get("sloposcope"))
                .and_then(|value| value.get("type"))
                .and_then(serde_json::Value::as_str),
            Some("local")
        );
        assert_eq!(
            config
                .get("provider")
                .and_then(|value| value.get("sloposcope_nim"))
                .and_then(|value| value.get("options"))
                .and_then(|value| value.get("apiKey"))
                .and_then(serde_json::Value::as_str),
            Some("nim-key")
        );
    }

    #[test]
    fn profiled_pi_worker_writes_local_config_and_runs_with_provider_model_and_prompt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut mission =
            init_git_repo_with_file(dir.path(), "app.js", "export const answer = 1;\n");
        mission.validation_command = "test \"$(cat app.js)\" = \"export const answer = 3;\"".into();
        let objective = work_store::ObjectiveRecord {
            id: "objective-pi".into(),
            mission_id: mission.id.clone(),
            title: "Edit code".into(),
            prompt: "Implement with pi".into(),
            status: work_store::WorkUnitStatus::Pending,
            claimed_by: None,
            claimed_at: None,
            created_at: "unix:1".into(),
            meta: work_store::ObjectiveMeta {
                scope_paths: vec!["app.js".into()],
                expected_validation: vec![],
                done_criteria: vec!["done".into()],
                test_intent: Some("outer_red".into()),
            },
        };
        let fake_pi = dir.path().join("fake-pi.sh");
        std::fs::write(
            &fake_pi,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > pi-argv.txt\nprintf '%s' \"$PI_CODING_AGENT_DIR\" > pi-config-dir.txt\nprintf '%s' \"$NVIDIA_API_KEY\" > pi-api-key.txt\ncp \"$PI_CODING_AGENT_DIR/config.yml\" pi-config-seen.yml\ncp \"$PI_CODING_AGENT_DIR/models.yml\" pi-models-seen.yml\nprintf 'export const answer = 3;\\n' > app.js\n",
        )
        .expect("write fake pi");
        assert!(std::process::Command::new("chmod")
            .arg("+x")
            .arg(&fake_pi)
            .status()
            .expect("chmod")
            .success());
        let profile = AgentProfile {
            id: "pi-cli".into(),
            label: "PI CLI".into(),
            provider: "pi_cli".into(),
            model: Some("openai/gpt-oss-20b".into()),
            command: fake_pi.display().to_string(),
            args: vec!["--tools".into(), "read,grep,find,ls,edit,bash".into()],
            env: serde_json::json!({
                "NGC_API_KEY": "nim-key",
                "NVIDIA_NIM_BASE_URL": "https://nim.example.test/v1"
            }),
            capabilities: vec!["can_edit".into()],
            transport_kind: "pipe".into(),
            active: true,
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };

        let result =
            execute_profiled_code_worker(dir.path(), &mission, &objective, "session-pi", &profile)
                .expect("run profiled pi worker");
        assert!(result.validation.success);
        let workspace = Path::new(&result.workspace_path);
        let argv = std::fs::read_to_string(workspace.join("pi-argv.txt")).expect("argv");
        assert!(argv.contains("--provider"));
        assert!(argv.contains("sloposcope_nim"));
        assert!(argv.contains("--model"));
        assert!(argv.contains("openai/gpt-oss-20b"));
        assert!(argv.contains("-p"));
        assert!(argv.contains("Mission: Mission"));
        assert!(argv.contains("--tools"));
        assert!(argv.contains("--no-session"));
        let config_dir = workspace.join(".sloposcope").join("pi-agent");
        assert_eq!(
            std::fs::read_to_string(workspace.join("pi-config-dir.txt")).expect("config dir"),
            config_dir.display().to_string()
        );
        assert_eq!(
            std::fs::read_to_string(workspace.join("pi-api-key.txt")).expect("api key"),
            "nim-key"
        );
        let config: serde_yaml::Value = serde_yaml::from_slice(
            &std::fs::read(workspace.join("pi-config-seen.yml")).expect("config"),
        )
        .expect("yaml");
        let models: serde_yaml::Value = serde_yaml::from_slice(
            &std::fs::read(workspace.join("pi-models-seen.yml")).expect("models"),
        )
        .expect("yaml");
        assert_eq!(
            config
                .get("modelRoles")
                .and_then(serde_yaml::Value::as_mapping)
                .and_then(|value| value.get(serde_yaml::Value::String("default".to_string())))
                .and_then(serde_yaml::Value::as_str),
            Some("sloposcope_nim/openai/gpt-oss-20b")
        );
        assert_eq!(
            models
                .get("providers")
                .and_then(serde_yaml::Value::as_mapping)
                .and_then(|value| value.get(serde_yaml::Value::String("sloposcope_nim".to_string())))
                .and_then(serde_yaml::Value::as_mapping)
                .and_then(|value| value.get(serde_yaml::Value::String("models".to_string())))
                .and_then(serde_yaml::Value::as_sequence)
                .and_then(|value| value.first())
                .and_then(serde_yaml::Value::as_mapping)
                .and_then(|value| value.get(serde_yaml::Value::String("id".to_string())))
                .and_then(serde_yaml::Value::as_str),
            Some("openai/gpt-oss-20b")
        );
        let diagnostics = result.diagnostics.expect("diagnostics");
        assert_eq!(
            diagnostics.get("piConfigDir").and_then(serde_json::Value::as_str),
            Some(config_dir.display().to_string().as_str())
        );
        assert_eq!(
            diagnostics
                .get("piConfigPath")
                .and_then(serde_json::Value::as_str),
            Some(config_dir.join("config.yml").display().to_string().as_str())
        );
        assert_eq!(
            diagnostics
                .get("piModelsPath")
                .and_then(serde_json::Value::as_str),
            Some(config_dir.join("models.yml").display().to_string().as_str())
        );
    }
}

// --- Entry point ---

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let boot_start = Instant::now();
    let repo = resolve_initial_repo();
    let repo = repo.canonicalize().unwrap_or(repo);
    if debug_enabled() {
        println!("[BACKEND][boot] run start repo={}", repo.display());
    }

    let parsers: Vec<Box<dyn Parser>> = vec![
        Box::new(PluggableParser {
            extensions: vec!["ex", "exs"],
            language: tree_sitter::Language::new(tree_sitter_elixir::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "call",
            queries: r#"
                (call target: (identifier) @kw (#eq? @kw "defmodule") (arguments (alias) @node.name))
                (call target: (identifier) @func.kind (#match? @func.kind "^(def|defp)$") (arguments [(identifier) @func.name (call target: (identifier) @func.name)]))
                (call target: (dot left: (alias) @calls.receiver right: (identifier) @calls.method))
                (call target: (identifier) @edge.kind (#match? @edge.kind "^(use|import|alias)$") (arguments (alias) @edge.target))
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["rb", "rake"],
            language: tree_sitter::Language::new(tree_sitter_ruby::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "call",
            queries: r#"
                (module name: [(constant) @node.name (scope_resolution) @node.name])
                (class name: [(constant) @node.name (scope_resolution) @node.name])
                (class (superclass (constant) @extends.target))
                (class (superclass (scope_resolution) @extends.target))
                (method name: (identifier) @func.name)
                (call receiver: (constant) @calls.receiver method: (identifier) @calls.method)
                (call receiver: (scope_resolution) @calls.receiver method: (identifier) @calls.method)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["js", "jsx"],
            language: tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            default_node_kind: "lib",
            default_edge_kind: "import",
            queries: r#"
                (import_statement source: (string (string_fragment) @edge.target))
                (class_declaration name: (identifier) @node.name)
                (class_declaration (class_heritage (identifier) @extends.target))
                (export_statement source: (string (string_fragment) @edge.target))
                (function_declaration name: (identifier) @func.name)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["ts", "tsx"],
            language: tree_sitter::Language::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
            default_node_kind: "lib",
            default_edge_kind: "import",
            queries: r#"
                (import_statement source: (string (string_fragment) @edge.target))
                (class_declaration name: (type_identifier) @node.name)
                (class_declaration (class_heritage (extends_clause (identifier) @extends.target)))
                (class_declaration (class_heritage (implements_clause (type_identifier) @implements.target)))
                (export_statement source: (string (string_fragment) @edge.target))
                (function_declaration name: (identifier) @func.name)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["svelte"],
            language: tree_sitter::Language::new(tree_sitter_javascript::LANGUAGE),
            default_node_kind: "component",
            default_edge_kind: "import",
            queries: r#"
                (import_statement source: (string (string_fragment) @edge.target))
                (import_statement (import_clause) source: (string (string_fragment) @edge.target))
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["py"],
            language: tree_sitter::Language::new(tree_sitter_python::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "import",
            queries: r#"
                (class_definition name: (identifier) @node.name)
                (class_definition superclasses: (argument_list (identifier) @extends.target))
                (function_definition name: (identifier) @func.name)
                (import_from_statement module: (dotted_name) @edge.target)
                (import_statement name: (dotted_name) @edge.target)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["rs"],
            language: tree_sitter::Language::new(tree_sitter_rust::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "use",
            queries: r#"
                (struct_item name: (type_identifier) @node.name)
                (enum_item name: (type_identifier) @node.name)
                (function_item name: (identifier) @func.name)
                (mod_item name: (identifier) @node.name)
                (use_declaration argument: [(scoped_identifier) @edge.target (identifier) @edge.target])
                (impl_item trait: (type_identifier) @implements.target)
                (use_declaration argument: (use_tree) @edge.target)
                (use_declaration argument: (scoped_use_list) @edge.target)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["go"],
            language: tree_sitter::Language::new(tree_sitter_go::LANGUAGE),
            default_node_kind: "package",
            default_edge_kind: "import",
            queries: r#"
                (package_clause (package_identifier) @node.name)
                (function_declaration name: (identifier) @func.name)
                (method_declaration name: (field_identifier) @func.name)
                (import_spec path: (string_literal) @edge.target)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["java"],
            language: tree_sitter::Language::new(tree_sitter_java::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "import",
            queries: r#"
                (class_declaration name: (identifier) @node.name)
                (class_declaration (superclass (type_identifier) @extends.target))
                (class_declaration (super_interfaces (type_list (type_identifier) @implements.target)))
                (interface_declaration name: (identifier) @node.name)
                (method_declaration name: (identifier) @func.name)
                (import_declaration (scoped_identifier) @edge.target)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["cpp", "hpp", "cc", "h"],
            language: tree_sitter::Language::new(tree_sitter_cpp::LANGUAGE),
            default_node_kind: "module",
            default_edge_kind: "include",
            queries: r#"
                (class_specifier name: (type_identifier) @node.name)
                (function_definition declarator: (function_declarator declarator: (identifier) @func.name))
                (preproc_include path: [(string_literal) @edge.target (system_lib_string) @edge.target])
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["php"],
            language: tree_sitter::Language::new(tree_sitter_php::LANGUAGE_PHP),
            default_node_kind: "class",
            default_edge_kind: "use",
            // PHP AST: class name uses node type 'name' (not 'identifier')
            // (base_clause (name)) for extends, (class_interface_clause (name)) for implements
            queries: r#"
                (class_declaration name: (name) @node.name)
                (class_declaration (base_clause (name) @extends.target))
                (class_declaration (class_interface_clause (name) @implements.target))
                (method_declaration name: (identifier) @func.name)
                (namespace_use_declaration (namespace_use_clause (qualified_name) @edge.target))
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["cs"],
            language: tree_sitter::Language::new(tree_sitter_c_sharp::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "using",
            // AST: (class_declaration (base_list (identifier))) — all bases
            queries: r#"
                (class_declaration name: (identifier) @node.name)
                (class_declaration (base_list (identifier) @extends.target))
                (method_declaration name: (identifier) @func.name)
                (using_directive [(identifier) (qualified_name)] @edge.target)
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["clj", "cljs", "cljc", "edn"],
            language: tree_sitter::Language::new(tree_sitter_clojure::LANGUAGE),
            default_node_kind: "namespace",
            default_edge_kind: "require",
            queries: r#"
                (list_lit (symbol) @ns_kw (#eq? @ns_kw "ns") (symbol) @node.name)
                (list_lit (symbol) @defn_kw (#match? @defn_kw "^defn-?$") (symbol) @func.name)
                (list_lit (keyword) @req_kw (#eq? @req_kw ":require") (list_lit (symbol) @edge.target))
            "#,
        }),
        Box::new(PluggableParser {
            extensions: vec!["kt", "kts"],
            language: tree_sitter::Language::new(tree_sitter_kotlin_ng::LANGUAGE),
            default_node_kind: "class",
            default_edge_kind: "import",
            queries: r#"
                (package_header (identifier) @node.name)
                (class_declaration (type_identifier) @node.name)
                (class_declaration (delegation_specifiers (delegation_specifier (constructor_invocation (user_type (identifier) @extends.target)))))
                (class_declaration (delegation_specifiers (delegation_specifier (user_type (identifier) @extends.target))))
                (function_declaration (identifier) @func.name)
                (import_header (identifier) @edge.target)
                (import_header (qualified_identifier) @edge.target)
            "#,
        }),
    ];

    let graph_start = Instant::now();
    let initial_graph_request = GraphViewRequest::default_working_copy();
    // Use cached graph if one exists; background rebuild (started in .setup) always
    // freshens the store and emits graph-updated when done.  This keeps startup
    // instant for large repos that previously froze before the window could open.
    let cached_view = derive_graph_view(&repo, &initial_graph_request).ok();
    let g = cached_view.map(|v| v.graph).unwrap_or_else(Graph::new);
    let branch = vcs::get_current_branch(&repo);
    let revision = vcs::get_current_revision(&repo);
    if debug_enabled() {
        println!(
            "[BACKEND][boot] boot state ready nodes={} edges={} branch='{}' rev='{}' took={}ms",
            g.nodes.len(),
            g.edges.len(),
            branch,
            if revision.len() > 12 {
                &revision[..12]
            } else {
                &revision
            },
            graph_start.elapsed().as_millis()
        );
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            repo: Mutex::new(repo),
            parsers,
            watcher: Mutex::new(None),
            graph: Mutex::new(g),
            last_graph_view_request: Mutex::new(initial_graph_request),
            last_since: Mutex::new(Some("@".to_string())),
            last_include_neighbors: Mutex::new(false),
            last_branch: Mutex::new(branch),
            last_revision: Mutex::new(revision),
            watcher_flush_scheduled: Mutex::new(false),
            watcher_pending_vcs_meta: Mutex::new(false),
            watcher_last_vcs_meta_at: Mutex::new(None),
            watcher_pending_paths: Mutex::new(Vec::new()),
            cluster_refresh_scheduled: Mutex::new(false),
            watchdog: Mutex::new(watchdog::WatchdogRuntimeState::default()),
            conductor: Mutex::new(conductor::ConductorRuntimeState::default()),
        })
        .invoke_handler(tauri::generate_handler![
            get_graph_view,
            get_graph,
            search_graph_entities,
            refresh_graph_clusters,
            get_changes,
            get_bookmarks,
            get_current_branch,
            get_file_diff,
            get_file_source,
            save_file,
            get_repo_path,
            get_watchdog_state,
            set_watchdog_active_issue,
            set_watchdog_active_issue_context,
            record_watchdog_validation,
            run_watchdog_validation_command,
            check_watchdog_focus,
            check_watchdog_before_push,
            get_watchdog_traces,
            run_watchdog_replay_eval,
            mcp_watchdog_get_status,
            mcp_watchdog_check_focus,
            mcp_watchdog_get_alerts,
            mcp_watchdog_get_issue_context,
            mcp_watchdog_get_evidence_snapshot,
            get_mission_state,
            save_mission_input,
            plan_mission,
            run_fixture_worker,
            run_code_worker,
            run_next_code_worker,
            sweep_stale_sessions,
            list_agent_profiles,
            save_agent_profile,
            delete_agent_profile,
            evaluate_worker_patch_gate,
            accept_worker_patch,
            reject_worker_patch,
            apply_worker_patch,
            agent_login,
            agent_activity_set,
            agent_report_event,
            agent_report_touched_files,
            agent_submit_verdict,
            agent_logout,
            conductor_get_status,
            conductor_refresh,
            conductor_enqueue_task,
            conductor_prepare_launch,
            conductor_ingest_runner_event,
            conductor_dispatch_launch,
            get_conductor_workflow_contract,
            save_conductor_workflow_settings,
            save_watchdog_repo_config,
            select_repo,
            pick_folder,
            set_repo_path,
        ])
        .setup(|app| {
            if debug_enabled() {
                println!("[BACKEND][boot] tauri setup start");
            }
            let handle = app.handle().clone();
            let poll_interval = vcs_poll_interval();
            if debug_enabled() {
                println!(
                    "[BACKEND][boot] polling refresh_vcs_state every {}ms",
                    poll_interval.as_millis()
                );
            }
            std::thread::spawn(move || loop {
                std::thread::sleep(poll_interval);
                refresh_vcs_state(&handle);
            });

            let watcher = setup_watcher(app.handle());
            let state = app.state::<AppState>();
            *state.watcher.lock() = watcher;

            // Always rebuild the graph in the background so the store stays fresh.
            // The frontend sees cached data (or an empty graph) immediately and
            // receives graph-updated once the build finishes.
            let handle_for_rebuild = app.handle().clone();
            let repo_for_rebuild = state.repo.lock().clone();
            std::thread::spawn(move || {
                background_rebuild_and_emit(handle_for_rebuild, repo_for_rebuild);
            });

            if debug_enabled() {
                println!("[BACKEND][boot] tauri setup complete");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run codegraph");

    if debug_enabled() {
        println!(
            "[BACKEND][boot] run exited after {}ms",
            boot_start.elapsed().as_millis()
        );
    }
}

use crate::graph_query::GraphIndex;
use crate::graph_store::{CanonicalGraphSnapshot, GraphStore, SqliteGraphStore};
use crate::vcs;
use crate::watchdog_config;
use crate::watchdog_types::{
    WatchdogAgentsContext, WatchdogAgentsLoadResult, WatchdogAlert, WatchdogConfigLoadResult,
    WatchdogIssueContext, WatchdogIssueKind, WatchdogIssueKindCount, WatchdogIssueRef,
    WatchdogReplayEvalReport, WatchdogReplayEvalTraceResult, WatchdogRepoConfig, WatchdogRunTrace,
    WatchdogSeverity, WatchdogSnapshot, WatchdogSpamRateReport, WatchdogStateView, WatchdogStatus,
    WatchdogValidationRecord, WatchdogVerdict, WatchdogVerdictIssue,
};
use serde::Deserialize;
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default)]
pub struct WatchdogRuntimeState {
    pub active_issue: Option<WatchdogIssueRef>,
    pub active_issue_context: Option<WatchdogIssueContext>,
    pub last_snapshot: Option<WatchdogSnapshot>,
    pub last_validation: Option<WatchdogValidationRecord>,
    pub last_verdict: Option<WatchdogVerdict>,
    pub last_policy_trigger_key: Option<String>,
    pub last_policy_trigger_at: Option<u64>,
    pub last_check_at: Option<u64>,
    pub last_verdict_hash: Option<String>,
    pub last_snapshot_cache_key: Option<String>,
    pub traces: Vec<WatchdogRunTrace>,
    pub alerts: Vec<WatchdogAlert>,
    pub alert_last_seen: HashMap<String, u64>,
}

pub trait WatchdogSemanticEvaluator {
    fn evaluate(
        &self,
        snapshot: &WatchdogSnapshot,
        config: &WatchdogRepoConfig,
    ) -> Option<WatchdogVerdictIssue>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct WatchdogEvalQualityGate {
    pub passed: bool,
    pub reason: String,
    pub traces_evaluated: usize,
    pub deduped_issues_per_run: f32,
    pub status_change_rate: f32,
}

pub fn find_agents_md(repo: &Path) -> Option<PathBuf> {
    let mut cursor = Some(repo);
    while let Some(dir) = cursor {
        let candidate = dir.join("AGENTS.md");
        if candidate.is_file() {
            return Some(candidate);
        }
        cursor = dir.parent();
    }
    None
}

fn strip_list_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim_start();
        }
    }

    let bytes = trimmed.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx > 0 && idx < bytes.len() && bytes[idx] == b'.' {
        idx += 1;
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        return &trimmed[idx..];
    }

    trimmed
}

fn parse_agents_rules(content: &str) -> Vec<String> {
    let mut rules = Vec::new();
    let mut in_code_fence = false;

    for raw in content.lines() {
        let line = raw.trim();
        if line.starts_with("```") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence || line.is_empty() || line.starts_with('#') {
            continue;
        }

        let normalized = strip_list_prefix(line).trim();
        if normalized.len() < 3 {
            continue;
        }
        rules.push(normalized.to_string());
        if rules.len() >= 200 {
            break;
        }
    }

    rules
}

pub fn load_agents_context(repo: &Path) -> WatchdogAgentsContext {
    let path = find_agents_md(repo);
    let Some(path_buf) = path else {
        return WatchdogAgentsContext {
            load: WatchdogAgentsLoadResult {
                path: None,
                exists: false,
                loaded: false,
                error: None,
            },
            rules: Vec::new(),
        };
    };

    let path_label = path_buf.display().to_string();
    match fs::read_to_string(&path_buf) {
        Ok(content) => WatchdogAgentsContext {
            load: WatchdogAgentsLoadResult {
                path: Some(path_label),
                exists: true,
                loaded: true,
                error: None,
            },
            rules: parse_agents_rules(&content),
        },
        Err(err) => WatchdogAgentsContext {
            load: WatchdogAgentsLoadResult {
                path: Some(path_label),
                exists: true,
                loaded: false,
                error: Some(format!("Failed to read AGENTS.md: {err}")),
            },
            rules: Vec::new(),
        },
    }
}

fn now_unix_seconds() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => 0,
    }
}

fn now_label() -> String {
    format!("unix:{}", now_unix_seconds())
}

fn parse_unix_label_seconds(label: &str) -> Option<u64> {
    let value = label.strip_prefix("unix:")?;
    value.parse::<u64>().ok()
}

fn build_issue_context(runtime: &WatchdogRuntimeState) -> Option<WatchdogIssueContext> {
    if let Some(ctx) = &runtime.active_issue_context {
        return Some(ctx.clone());
    }
    runtime
        .active_issue
        .as_ref()
        .map(|issue| WatchdogIssueContext {
            issue: issue.clone(),
            summary: None,
            acceptance_criteria: Vec::new(),
            non_goals: Vec::new(),
            labels: Vec::new(),
            priority: None,
        })
}

fn filtered_diff_lines(diff: &str) -> Vec<String> {
    diff.lines()
        .filter(|line| {
            !line.starts_with("diff --git")
                && !line.starts_with("index ")
                && !line.starts_with("--- ")
                && !line.starts_with("+++ ")
        })
        .map(|line| line.to_string())
        .collect()
}

fn has_negative_constraint(rule_lower: &str) -> bool {
    [
        "never ", "never.", "never,", "do not", "don't", "must not", "cannot", "can't", "avoid ",
    ]
    .iter()
    .any(|needle| rule_lower.contains(needle))
}

fn extract_backtick_literals(rule: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut current = String::new();
    let mut in_tick = false;

    for ch in rule.chars() {
        if ch == '`' {
            if in_tick {
                let value = current.trim();
                if value.len() >= 2 {
                    literals.push(value.to_string());
                }
                current.clear();
                in_tick = false;
            } else {
                in_tick = true;
            }
            continue;
        }
        if in_tick {
            current.push(ch);
        }
    }

    literals
}

fn truncate_label(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    input
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>()
        + "..."
}

fn detect_instruction_violations(snapshot: &WatchdogSnapshot) -> Vec<String> {
    let mut evidence: Vec<String> = Vec::new();
    if snapshot.agents_rules.is_empty() {
        return evidence;
    }

    for rule in &snapshot.agents_rules {
        if evidence.len() >= 6 {
            break;
        }

        let rule_lower = rule.to_lowercase();
        if !has_negative_constraint(&rule_lower) {
            continue;
        }

        let mut matched = false;
        for literal in extract_backtick_literals(rule) {
            let literal_lower = literal.to_lowercase();
            for file in &snapshot.changed_files {
                let path_lower = file.path.to_lowercase();
                if path_lower.contains(&literal_lower)
                    || file
                        .diff_excerpt
                        .iter()
                        .any(|line| line.to_lowercase().contains(&literal_lower))
                {
                    evidence.push(format!(
                        "rule='{}' matched='{}' file={}",
                        truncate_label(rule, 80),
                        literal,
                        file.path
                    ));
                    matched = true;
                    break;
                }
            }
            if matched {
                break;
            }
        }
        if matched {
            continue;
        }

        if rule_lower.contains("ascii") {
            for file in &snapshot.changed_files {
                if file
                    .diff_excerpt
                    .iter()
                    .any(|line| line.starts_with('+') && !line.is_ascii())
                {
                    evidence.push(format!(
                        "rule='{}' non_ascii_addition file={}",
                        truncate_label(rule, 80),
                        file.path
                    ));
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            continue;
        }

        if rule_lower.contains("em dash") || rule_lower.contains("em-dash") {
            for file in &snapshot.changed_files {
                if file
                    .diff_excerpt
                    .iter()
                    .any(|line| line.starts_with('+') && line.contains('—'))
                {
                    evidence.push(format!(
                        "rule='{}' em_dash_addition file={}",
                        truncate_label(rule, 80),
                        file.path
                    ));
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            continue;
        }

        if rule_lower.contains("nested bullet") {
            for file in &snapshot.changed_files {
                let is_markdown = file.path.ends_with(".md") || file.path.ends_with(".markdown");
                if !is_markdown {
                    continue;
                }
                if file.diff_excerpt.iter().any(|line| {
                    line.starts_with('+')
                        && (line[1..].starts_with("  - ")
                            || line[1..].starts_with("  * ")
                            || line[1..].starts_with("    - ")
                            || line[1..].starts_with("    * "))
                }) {
                    evidence.push(format!(
                        "rule='{}' nested_bullet_addition file={}",
                        truncate_label(rule, 80),
                        file.path
                    ));
                    break;
                }
            }
        }
    }

    evidence
}

struct SnapshotBuildInputs<'a> {
    repo: &'a Path,
    since: &'a str,
    branch: &'a str,
    revision: &'a str,
    issue_context: &'a Option<WatchdogIssueContext>,
    validation: &'a Option<WatchdogValidationRecord>,
    agents: &'a WatchdogAgentsContext,
    changed_entries: &'a [(String, String)],
    max_files: usize,
    max_diff_lines_per_file: usize,
    repo_config: &'a WatchdogRepoConfig,
    repo_config_load: &'a WatchdogConfigLoadResult,
    graph_snapshot: Option<&'a CanonicalGraphSnapshot>,
}

fn hash_repo_file_state(hasher: &mut DefaultHasher, repo: &Path, relative_path: &str) {
    let full_path = repo.join(relative_path);
    match fs::metadata(full_path) {
        Ok(meta) => {
            true.hash(hasher);
            meta.len().hash(hasher);
            match meta.modified() {
                Ok(modified) => match modified.duration_since(UNIX_EPOCH) {
                    Ok(duration) => {
                        duration.as_secs().hash(hasher);
                        duration.subsec_nanos().hash(hasher);
                    }
                    Err(_) => {
                        0u64.hash(hasher);
                        0u32.hash(hasher);
                    }
                },
                Err(_) => {
                    0u64.hash(hasher);
                    0u32.hash(hasher);
                }
            }
        }
        Err(_) => {
            false.hash(hasher);
        }
    }
}

fn build_snapshot_from_changed_entries(inputs: &SnapshotBuildInputs<'_>) -> WatchdogSnapshot {
    let mut changed_files = Vec::new();
    for (path, status) in inputs.changed_entries.iter().take(inputs.max_files) {
        let diff = vcs::get_file_diff(inputs.repo, path, Some(inputs.since));
        let lines = filtered_diff_lines(&diff);
        let excerpt = lines
            .iter()
            .take(inputs.max_diff_lines_per_file)
            .cloned()
            .collect::<Vec<_>>();
        changed_files.push(crate::watchdog_types::WatchdogChangedFileEvidence {
            path: path.clone(),
            status: status.clone(),
            diff_excerpt: excerpt,
            diff_line_count: lines.len(),
        });
    }

    let (affected_entities, related_entities, blast_radius_count) =
        build_graph_evidence(inputs, &changed_files);

    WatchdogSnapshot {
        schema_version: 1,
        created_at: now_label(),
        branch: inputs.branch.to_string(),
        revision: inputs.revision.to_string(),
        since: inputs.since.to_string(),
        repo_config: Some(inputs.repo_config.clone()),
        repo_config_load: Some(inputs.repo_config_load.clone()),
        issue_context: inputs.issue_context.clone(),
        agents_rules_count: inputs.agents.rules.len(),
        agents_rules: inputs.agents.rules.clone(),
        changed_files_total: inputs.changed_entries.len(),
        changed_files,
        validation: inputs.validation.clone(),
        affected_entities,
        related_entities,
        blast_radius_count,
    }
}

fn build_graph_evidence(
    inputs: &SnapshotBuildInputs<'_>,
    changed_files: &[crate::watchdog_types::WatchdogChangedFileEvidence],
) -> (Vec<String>, Vec<String>, usize) {
    let Some(graph_snap) = inputs.graph_snapshot else {
        return (vec![], vec![], 0);
    };
    let index = GraphIndex::build(graph_snap);
    let changed_paths: Vec<&str> = changed_files.iter().map(|f| f.path.as_str()).collect();
    let changed_path_set: HashSet<&str> = changed_paths.iter().copied().collect();

    // 1-hop blast gives us affected (in changed files) and related (1-hop neighbors outside)
    let one_hop = index.blast_radius(&changed_paths, 1);
    let affected_entities: Vec<String> = one_hop
        .iter()
        .filter(|e| changed_path_set.contains(e.file_path.as_str()))
        .map(|e| e.display_name.clone())
        .collect();
    let related_entities: Vec<String> = one_hop
        .iter()
        .filter(|e| !changed_path_set.contains(e.file_path.as_str()))
        .map(|e| e.display_name.clone())
        .take(50)
        .collect();

    // 2-hop gives the full blast radius count
    let two_hop = index.blast_radius(&changed_paths, 2);
    let blast_radius_count = two_hop.len();

    (affected_entities, related_entities, blast_radius_count)
}

fn build_snapshot_cache_key(inputs: &SnapshotBuildInputs<'_>) -> String {
    let mut hasher = DefaultHasher::new();
    inputs.since.hash(&mut hasher);
    inputs.branch.hash(&mut hasher);
    inputs.revision.hash(&mut hasher);
    inputs.max_files.hash(&mut hasher);
    inputs.max_diff_lines_per_file.hash(&mut hasher);
    inputs.repo_config.enabled.hash(&mut hasher);
    inputs.repo_config.trigger_mode.hash(&mut hasher);
    inputs.repo_config.require_active_issue.hash(&mut hasher);
    inputs.repo_config.max_files_in_snapshot.hash(&mut hasher);
    inputs.repo_config.max_diff_lines_per_file.hash(&mut hasher);
    inputs.repo_config.dedupe_window_seconds.hash(&mut hasher);
    inputs.repo_config.alert_threshold.hash(&mut hasher);
    inputs.repo_config.checks.issue_alignment.hash(&mut hasher);
    inputs
        .repo_config
        .checks
        .semantic_alignment
        .hash(&mut hasher);
    inputs
        .repo_config
        .checks
        .instruction_compliance
        .hash(&mut hasher);
    inputs
        .repo_config
        .checks
        .missing_validation
        .hash(&mut hasher);
    inputs.repo_config.checks.cost_risk.hash(&mut hasher);
    inputs.repo_config_load.exists.hash(&mut hasher);
    inputs.repo_config_load.loaded.hash(&mut hasher);
    inputs.repo_config_load.path.hash(&mut hasher);
    inputs.repo_config_load.error.hash(&mut hasher);

    for rule in &inputs.agents.rules {
        rule.hash(&mut hasher);
    }
    if let Some(ctx) = inputs.issue_context {
        ctx.issue.id.hash(&mut hasher);
        ctx.issue.title.hash(&mut hasher);
        ctx.issue.identifier.hash(&mut hasher);
        ctx.issue.url.hash(&mut hasher);
        ctx.summary.hash(&mut hasher);
        ctx.priority.hash(&mut hasher);
        for value in &ctx.acceptance_criteria {
            value.hash(&mut hasher);
        }
        for value in &ctx.non_goals {
            value.hash(&mut hasher);
        }
        for value in &ctx.labels {
            value.hash(&mut hasher);
        }
    } else {
        "no-issue-context".hash(&mut hasher);
    }
    if let Some(validation) = inputs.validation {
        validation.kind.hash(&mut hasher);
        validation.success.hash(&mut hasher);
        validation.ran_at.hash(&mut hasher);
        validation.details.hash(&mut hasher);
    } else {
        "no-validation".hash(&mut hasher);
    }
    for (path, status) in inputs.changed_entries {
        path.hash(&mut hasher);
        status.hash(&mut hasher);
    }
    for (path, status) in inputs.changed_entries.iter().take(inputs.max_files) {
        path.hash(&mut hasher);
        status.hash(&mut hasher);
        hash_repo_file_state(&mut hasher, inputs.repo, path);
    }
    format!("{:016x}", hasher.finish())
}

fn load_graph_snapshot(repo: &Path) -> Option<CanonicalGraphSnapshot> {
    SqliteGraphStore::open_or_create(repo)
        .ok()
        .and_then(|store| store.load_snapshot().ok())
}

#[allow(clippy::too_many_arguments)]
fn build_snapshot_for_repo_with_context(
    repo: &Path,
    since: &str,
    runtime: &mut WatchdogRuntimeState,
    max_files: usize,
    max_diff_lines_per_file: usize,
    repo_config: &WatchdogRepoConfig,
    repo_config_load: &WatchdogConfigLoadResult,
    graph_snapshot: Option<&CanonicalGraphSnapshot>,
) -> WatchdogSnapshot {
    let agents = load_agents_context(repo);
    let changed = vcs::get_changed_files(repo, since);
    let mut changed_entries: Vec<(String, String)> = changed.into_iter().collect();
    changed_entries.sort_by(|a, b| a.0.cmp(&b.0));
    let branch = vcs::get_current_branch(repo);
    let revision = vcs::get_current_revision(repo);
    let issue_context = build_issue_context(runtime);
    let validation = runtime.last_validation.clone();
    let inputs = SnapshotBuildInputs {
        repo,
        since,
        branch: &branch,
        revision: &revision,
        issue_context: &issue_context,
        validation: &validation,
        agents: &agents,
        changed_entries: &changed_entries,
        max_files,
        max_diff_lines_per_file,
        repo_config,
        repo_config_load,
        graph_snapshot,
    };
    let cache_key = build_snapshot_cache_key(&inputs);

    if runtime.last_snapshot_cache_key.as_deref() == Some(cache_key.as_str())
        && runtime.last_snapshot.is_some()
    {
        let mut cached = runtime
            .last_snapshot
            .as_ref()
            .cloned()
            .expect("cache key implies snapshot exists");
        cached.created_at = now_label();
        return cached;
    }

    runtime.last_snapshot_cache_key = Some(cache_key);
    build_snapshot_from_changed_entries(&inputs)
}

pub fn build_snapshot_for_repo(
    repo: &Path,
    since: &str,
    runtime: &mut WatchdogRuntimeState,
    max_files: usize,
    max_diff_lines_per_file: usize,
) -> WatchdogSnapshot {
    let (repo_config, repo_config_load) = watchdog_config::load_repo_config(repo);
    let graph_snapshot = load_graph_snapshot(repo);
    build_snapshot_for_repo_with_context(
        repo,
        since,
        runtime,
        max_files,
        max_diff_lines_per_file,
        &repo_config,
        &repo_config_load,
        graph_snapshot.as_ref(),
    )
}

fn extract_tokens(input: &str) -> Vec<String> {
    input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|t| t.trim().to_lowercase())
        .filter(|t| t.len() >= 4)
        .collect()
}

fn file_matches_tokens(
    file: &crate::watchdog_types::WatchdogChangedFileEvidence,
    tokens: &[String],
) -> bool {
    let path_lower = file.path.to_lowercase();
    if tokens.iter().any(|t| path_lower.contains(t)) {
        return true;
    }
    file.diff_excerpt.iter().any(|line| {
        let line_lower = line.to_lowercase();
        tokens.iter().any(|t| line_lower.contains(t))
    })
}

fn finalize_verdict_from_issues(
    changed_files_total: usize,
    issues: Vec<WatchdogVerdictIssue>,
) -> WatchdogVerdict {
    let has_critical = issues
        .iter()
        .any(|issue| issue.severity == WatchdogSeverity::Critical);
    let status = if changed_files_total == 0 && issues.is_empty() {
        WatchdogStatus::OnTrack
    } else if has_critical {
        WatchdogStatus::Blocked
    } else if issues.is_empty() {
        WatchdogStatus::OnTrack
    } else {
        WatchdogStatus::Drift
    };
    let confidence = if issues.is_empty() { 0.85 } else { 0.62 };
    let summary = match status {
        WatchdogStatus::OnTrack if changed_files_total == 0 => {
            "No working-copy changes detected.".to_string()
        }
        WatchdogStatus::OnTrack => "Current changes appear on-track.".to_string(),
        WatchdogStatus::Drift => "Potential drift detected; review issues.".to_string(),
        WatchdogStatus::Blocked => "Critical watchdog issue detected.".to_string(),
        WatchdogStatus::InsufficientContext => "Watchdog lacks enough context.".to_string(),
    };

    WatchdogVerdict {
        status,
        confidence,
        summary,
        issues,
    }
}

pub fn evaluate_snapshot(
    snapshot: &WatchdogSnapshot,
    config: &crate::watchdog_types::WatchdogRepoConfig,
) -> WatchdogVerdict {
    let mut issues: Vec<WatchdogVerdictIssue> = Vec::new();

    if config.require_active_issue && snapshot.issue_context.is_none() {
        issues.push(WatchdogVerdictIssue {
            kind: WatchdogIssueKind::UnclearMapping,
            severity: WatchdogSeverity::Warning,
            title: "Active issue is not set".to_string(),
            why: "Watchdog is configured to require an active issue context.".to_string(),
            evidence: vec!["issueContext=None".to_string()],
            recommended_next_step: "Set active issue context before running focus checks."
                .to_string(),
        });
    }

    if config.checks.cost_risk && snapshot.changed_files_total > config.max_files_in_snapshot {
        issues.push(WatchdogVerdictIssue {
            kind: WatchdogIssueKind::CostRisk,
            severity: WatchdogSeverity::Warning,
            title: "Large change batch may reduce signal quality".to_string(),
            why: "Changed file count exceeds snapshot cap configured for watchdog checks."
                .to_string(),
            evidence: vec![
                format!("changedFilesTotal={}", snapshot.changed_files_total),
                format!("maxFilesInSnapshot={}", config.max_files_in_snapshot),
            ],
            recommended_next_step:
                "Split work into smaller batches or raise snapshot cap deliberately.".to_string(),
        });
    }

    if config.checks.missing_validation
        && snapshot.changed_files_total > 0
        && snapshot.validation.is_none()
    {
        issues.push(WatchdogVerdictIssue {
            kind: WatchdogIssueKind::MissingValidation,
            severity: WatchdogSeverity::Warning,
            title: "No validation record for current changes".to_string(),
            why: "Working-copy changes exist, but watchdog has no recent build/test validation record.".to_string(),
            evidence: vec![
                format!("changedFilesTotal={}", snapshot.changed_files_total),
                "lastValidation=None".to_string(),
            ],
            recommended_next_step: "Run build/tests and record validation result.".to_string(),
        });
    }

    if config.checks.instruction_compliance
        && snapshot.changed_files_total > 0
        && !snapshot.agents_rules.is_empty()
    {
        let mut evidence = detect_instruction_violations(snapshot);
        if !evidence.is_empty() {
            evidence.insert(
                0,
                format!("agentsRulesCount={}", snapshot.agents_rules_count),
            );
            issues.push(WatchdogVerdictIssue {
                kind: WatchdogIssueKind::InstructionViolation,
                severity: WatchdogSeverity::Warning,
                title: "Potential AGENTS.md instruction violation".to_string(),
                why: "Changed content appears to conflict with explicit negative constraints from AGENTS.md.".to_string(),
                evidence,
                recommended_next_step: "Inspect matched rule/evidence and update the change or rule context.".to_string(),
            });
        }
    }

    if config.checks.issue_alignment && snapshot.changed_files_total > 0 {
        if let Some(issue) = &snapshot.issue_context {
            let mut issue_text = issue.issue.title.clone();
            if let Some(summary) = &issue.summary {
                issue_text.push(' ');
                issue_text.push_str(summary);
            }
            let tokens = extract_tokens(&issue_text);
            if !tokens.is_empty() {
                let mut matched_files = 0usize;
                for file in &snapshot.changed_files {
                    if file_matches_tokens(file, &tokens) {
                        matched_files = matched_files.saturating_add(1);
                    }
                }
                if matched_files == 0 {
                    // Graph-aware suppression: if 1-hop neighbors of changed entities
                    // match issue tokens, the change is likely relevant despite path mismatch.
                    let graph_match = snapshot.related_entities.iter().any(|name| {
                        let lower = name.to_ascii_lowercase();
                        tokens.iter().any(|t| lower.contains(t.as_str()))
                    });

                    if !graph_match {
                        issues.push(WatchdogVerdictIssue {
                            kind: WatchdogIssueKind::Drift,
                            severity: WatchdogSeverity::Warning,
                            title: "Changes look weakly aligned with active issue".to_string(),
                            why: "Issue keywords did not appear in changed file paths, diff excerpts, or adjacent graph entities.".to_string(),
                            evidence: vec![
                                format!("issueTitle={}", issue.issue.title),
                                format!(
                                    "sampleChangedFile={}",
                                    snapshot
                                        .changed_files
                                        .first()
                                        .map(|f| f.path.clone())
                                        .unwrap_or_else(|| "-".to_string())
                                ),
                            ],
                            recommended_next_step:
                                "Verify issue mapping or update active issue context.".to_string(),
                        });
                    }
                } else {
                    let coverage =
                        matched_files as f32 / snapshot.changed_files_total.max(1) as f32;
                    let broad_scope_min_files = (config.max_files_in_snapshot / 2).max(6);
                    if snapshot.changed_files_total >= broad_scope_min_files && coverage < 0.35 {
                        issues.push(WatchdogVerdictIssue {
                            kind: WatchdogIssueKind::CostRisk,
                            severity: WatchdogSeverity::Warning,
                            title: "Work appears broader than active issue scope".to_string(),
                            why: "Only a minority of changed files match active issue vocabulary, while the overall batch is wide.".to_string(),
                            evidence: vec![
                                format!("changedFilesTotal={}", snapshot.changed_files_total),
                                format!("matchedFiles={matched_files}"),
                                format!("coverage={:.2}", coverage),
                                format!("issueTitle={}", issue.issue.title),
                            ],
                            recommended_next_step:
                                "Split unrelated files into separate tasks or tighten issue mapping."
                                    .to_string(),
                        });
                    }
                }
            }
        }
    }

    finalize_verdict_from_issues(snapshot.changed_files_total, issues)
}

fn should_run_semantic_by_policy(
    snapshot: &WatchdogSnapshot,
    config: &WatchdogRepoConfig,
    deterministic: &WatchdogVerdict,
) -> bool {
    if !config.checks.semantic_alignment {
        return false;
    }
    if snapshot.changed_files_total == 0 {
        return false;
    }
    if deterministic.status == WatchdogStatus::Blocked {
        return false;
    }
    if config.require_active_issue && snapshot.issue_context.is_none() {
        return false;
    }
    !deterministic
        .issues
        .iter()
        .any(|issue| issue.kind == WatchdogIssueKind::UnclearMapping)
}

pub fn evaluate_snapshot_with_optional_semantic(
    snapshot: &WatchdogSnapshot,
    config: &WatchdogRepoConfig,
    semantic: Option<&dyn WatchdogSemanticEvaluator>,
) -> WatchdogVerdict {
    let mut verdict = evaluate_snapshot(snapshot, config);
    if !should_run_semantic_by_policy(snapshot, config, &verdict) {
        return verdict;
    }
    let Some(semantic) = semantic else {
        return verdict;
    };
    if let Some(issue) = semantic.evaluate(snapshot, config) {
        verdict.issues.push(issue);
    }
    finalize_verdict_from_issues(snapshot.changed_files_total, verdict.issues)
}

fn issue_kind_label(kind: &WatchdogIssueKind) -> &'static str {
    match kind {
        WatchdogIssueKind::Drift => "drift",
        WatchdogIssueKind::InstructionViolation => "instruction_violation",
        WatchdogIssueKind::MissingValidation => "missing_validation",
        WatchdogIssueKind::CostRisk => "cost_risk",
        WatchdogIssueKind::UnclearMapping => "unclear_mapping",
    }
}

fn issue_severity_label(severity: &WatchdogSeverity) -> &'static str {
    match severity {
        WatchdogSeverity::Info => "info",
        WatchdogSeverity::Warning => "warning",
        WatchdogSeverity::Critical => "critical",
    }
}

fn alert_fingerprint(issue: &WatchdogVerdictIssue) -> String {
    let mut hasher = DefaultHasher::new();
    issue_kind_label(&issue.kind).hash(&mut hasher);
    issue_severity_label(&issue.severity).hash(&mut hasher);
    issue.title.hash(&mut hasher);
    issue.why.hash(&mut hasher);
    issue.recommended_next_step.hash(&mut hasher);
    for evidence in &issue.evidence {
        evidence.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn verdict_fingerprint(verdict: &WatchdogVerdict) -> String {
    let mut hasher = DefaultHasher::new();
    verdict.status.hash(&mut hasher);
    verdict.summary.hash(&mut hasher);
    verdict.confidence.to_bits().hash(&mut hasher);
    for issue in &verdict.issues {
        issue.kind.hash(&mut hasher);
        issue.severity.hash(&mut hasher);
        issue.title.hash(&mut hasher);
        issue.why.hash(&mut hasher);
        issue.recommended_next_step.hash(&mut hasher);
        for evidence in &issue.evidence {
            evidence.hash(&mut hasher);
        }
    }
    format!("{:016x}", hasher.finish())
}

fn maybe_push_deduped_alert(
    runtime: &mut WatchdogRuntimeState,
    issue: &WatchdogVerdictIssue,
    trace_id: &str,
    created_at: &str,
    alert_index: usize,
    dedupe_window_seconds: u64,
) {
    let fingerprint = alert_fingerprint(issue);
    let now_seconds = parse_unix_label_seconds(created_at).unwrap_or_else(now_unix_seconds);

    if dedupe_window_seconds > 0
        && runtime
            .alert_last_seen
            .get(&fingerprint)
            .is_some_and(|last| now_seconds.saturating_sub(*last) < dedupe_window_seconds)
    {
        return;
    }

    runtime
        .alert_last_seen
        .insert(fingerprint.clone(), now_seconds);
    runtime.alerts.push(WatchdogAlert {
        id: format!(
            "{}-{}-{}-{}",
            trace_id,
            issue_kind_label(&issue.kind),
            issue_severity_label(&issue.severity),
            alert_index
        ),
        kind: issue.kind.clone(),
        severity: issue.severity.clone(),
        title: issue.title.clone(),
        message: issue.why.clone(),
        evidence: issue.evidence.clone(),
        created_at: created_at.to_string(),
        verdict_id: Some(trace_id.to_string()),
    });
}

fn new_trace_id() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("wd-{}", d.as_millis()),
        Err(_) => "wd-0".to_string(),
    }
}

pub fn record_external_check(
    repo: &Path,
    runtime: &mut WatchdogRuntimeState,
    snapshot: WatchdogSnapshot,
    verdict: WatchdogVerdict,
) -> WatchdogRunTrace {
    let (config, _) = watchdog_config::load_repo_config(repo);
    let created_at = now_label();
    let now_seconds = parse_unix_label_seconds(&created_at).unwrap_or_else(now_unix_seconds);
    let id = new_trace_id();
    let verdict_hash = verdict_fingerprint(&verdict);
    let trace = WatchdogRunTrace {
        id: id.clone(),
        created_at: created_at.clone(),
        snapshot: snapshot.clone(),
        verdict: verdict.clone(),
    };

    runtime.last_snapshot = Some(snapshot);
    runtime.last_verdict = Some(verdict.clone());
    runtime.last_check_at = Some(now_seconds);
    runtime.last_verdict_hash = Some(verdict_hash);
    runtime.traces.push(trace.clone());
    if runtime.traces.len() > 100 {
        let drop_count = runtime.traces.len() - 100;
        runtime.traces.drain(0..drop_count);
    }

    for (idx, issue) in verdict.issues.iter().enumerate() {
        maybe_push_deduped_alert(
            runtime,
            issue,
            &id,
            &created_at,
            idx,
            config.dedupe_window_seconds,
        );
    }
    if runtime.alerts.len() > 200 {
        let drop_count = runtime.alerts.len() - 200;
        runtime.alerts.drain(0..drop_count);
    }

    trace
}

pub fn run_manual_check(
    repo: &Path,
    since: &str,
    runtime: &mut WatchdogRuntimeState,
) -> WatchdogRunTrace {
    let (config, config_load) = watchdog_config::load_repo_config(repo);
    let graph_snapshot = load_graph_snapshot(repo);
    let snapshot = build_snapshot_for_repo_with_context(
        repo,
        since,
        runtime,
        config.max_files_in_snapshot,
        config.max_diff_lines_per_file,
        &config,
        &config_load,
        graph_snapshot.as_ref(),
    );
    let verdict = evaluate_snapshot_with_optional_semantic(&snapshot, &config, None);

    record_external_check(repo, runtime, snapshot, verdict)
}

pub fn get_recent_traces(runtime: &WatchdogRuntimeState, limit: usize) -> Vec<WatchdogRunTrace> {
    let take = limit.clamp(1, 100);
    runtime.traces.iter().rev().take(take).cloned().collect()
}

pub fn build_replay_eval_report(
    traces: &[WatchdogRunTrace],
    config: &crate::watchdog_types::WatchdogRepoConfig,
) -> WatchdogReplayEvalReport {
    let runs_total = traces.len();
    let mut dedupe_last_seen: HashMap<String, u64> = HashMap::new();
    let mut unique_fingerprints: HashSet<String> = HashSet::new();
    let mut kind_counts: HashMap<String, usize> = HashMap::new();
    let mut runs_with_issues = 0usize;
    let mut traces_with_status_change = 0usize;
    let mut issues_total_raw = 0usize;
    let mut issues_total_deduped = 0usize;
    let mut results: Vec<WatchdogReplayEvalTraceResult> = Vec::with_capacity(runs_total);

    for (idx, trace) in traces.iter().enumerate() {
        let verdict = evaluate_snapshot(&trace.snapshot, config);
        let trace_time = parse_unix_label_seconds(&trace.created_at).unwrap_or(idx as u64);

        if !verdict.issues.is_empty() {
            runs_with_issues = runs_with_issues.saturating_add(1);
        }

        let status_changed = verdict.status != trace.verdict.status;
        if status_changed {
            traces_with_status_change = traces_with_status_change.saturating_add(1);
        }

        let mut deduped_issue_count = 0usize;
        for issue in &verdict.issues {
            let kind = issue_kind_label(&issue.kind).to_string();
            *kind_counts.entry(kind).or_insert(0) += 1;

            let fingerprint = alert_fingerprint(issue);
            unique_fingerprints.insert(fingerprint.clone());
            let should_count = if config.dedupe_window_seconds == 0 {
                true
            } else {
                match dedupe_last_seen.get(&fingerprint) {
                    Some(last) => trace_time.saturating_sub(*last) >= config.dedupe_window_seconds,
                    None => true,
                }
            };
            if should_count {
                dedupe_last_seen.insert(fingerprint, trace_time);
                deduped_issue_count = deduped_issue_count.saturating_add(1);
            }
        }

        let raw_issue_count = verdict.issues.len();
        issues_total_raw = issues_total_raw.saturating_add(raw_issue_count);
        issues_total_deduped = issues_total_deduped.saturating_add(deduped_issue_count);

        results.push(WatchdogReplayEvalTraceResult {
            trace_id: trace.id.clone(),
            expected_status: trace.verdict.status.clone(),
            actual_status: verdict.status,
            status_changed,
            raw_issue_count,
            deduped_issue_count,
        });
    }

    let denominator = runs_total.max(1) as f32;
    let mut issue_kind_counts: Vec<WatchdogIssueKindCount> = kind_counts
        .into_iter()
        .map(|(kind, count)| WatchdogIssueKindCount { kind, count })
        .collect();
    issue_kind_counts.sort_by(|a, b| b.count.cmp(&a.count).then(a.kind.cmp(&b.kind)));

    WatchdogReplayEvalReport {
        generated_at: now_label(),
        traces_evaluated: runs_total,
        config_dedupe_window_seconds: config.dedupe_window_seconds,
        spam: WatchdogSpamRateReport {
            runs_total,
            runs_with_issues,
            traces_with_status_change,
            issues_total_raw,
            issues_total_deduped,
            unique_issue_fingerprints: unique_fingerprints.len(),
            average_issues_per_run_raw: issues_total_raw as f32 / denominator,
            average_issues_per_run_deduped: issues_total_deduped as f32 / denominator,
            issue_kind_counts,
        },
        results,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WatchdogReplayFixtureForGate {
    config: WatchdogRepoConfig,
    traces: Vec<WatchdogRunTrace>,
}

pub fn evaluate_auto_trigger_quality_gate() -> WatchdogEvalQualityGate {
    const MAX_DEDUPED_ISSUES_PER_RUN: f32 = 0.80;
    const MAX_STATUS_CHANGE_RATE: f32 = 0.60;

    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("watchdog_replay_cases");
    if !fixture_dir.is_dir() {
        return WatchdogEvalQualityGate {
            passed: true,
            reason: format!(
                "replay fixture dir missing at {} (fail-open)",
                fixture_dir.display()
            ),
            traces_evaluated: 0,
            deduped_issues_per_run: 0.0,
            status_change_rate: 0.0,
        };
    }

    let mut traces_evaluated = 0usize;
    let mut issues_total_deduped = 0usize;
    let mut status_changes = 0usize;
    let mut parsed_cases = 0usize;
    let entries = match fs::read_dir(&fixture_dir) {
        Ok(entries) => entries,
        Err(err) => {
            return WatchdogEvalQualityGate {
                passed: true,
                reason: format!(
                    "failed to read replay fixture dir {}: {} (fail-open)",
                    fixture_dir.display(),
                    err
                ),
                traces_evaluated: 0,
                deduped_issues_per_run: 0.0,
                status_change_rate: 0.0,
            };
        }
    };

    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let fixture: WatchdogReplayFixtureForGate = match serde_json::from_str(&raw) {
            Ok(fixture) => fixture,
            Err(_) => continue,
        };
        parsed_cases = parsed_cases.saturating_add(1);
        let report = build_replay_eval_report(&fixture.traces, &fixture.config);
        traces_evaluated = traces_evaluated.saturating_add(report.traces_evaluated);
        issues_total_deduped =
            issues_total_deduped.saturating_add(report.spam.issues_total_deduped);
        status_changes = status_changes.saturating_add(report.spam.traces_with_status_change);
    }

    if parsed_cases == 0 || traces_evaluated == 0 {
        return WatchdogEvalQualityGate {
            passed: true,
            reason: "no parseable replay fixtures found (fail-open)".to_string(),
            traces_evaluated: 0,
            deduped_issues_per_run: 0.0,
            status_change_rate: 0.0,
        };
    }

    let runs = traces_evaluated.max(1) as f32;
    let deduped_issues_per_run = issues_total_deduped as f32 / runs;
    let status_change_rate = status_changes as f32 / runs;
    let passed = deduped_issues_per_run <= MAX_DEDUPED_ISSUES_PER_RUN
        && status_change_rate <= MAX_STATUS_CHANGE_RATE;
    let reason = if passed {
        format!(
            "gate passed: deduped_issues_per_run={:.2} (<= {:.2}), status_change_rate={:.2} (<= {:.2})",
            deduped_issues_per_run,
            MAX_DEDUPED_ISSUES_PER_RUN,
            status_change_rate,
            MAX_STATUS_CHANGE_RATE
        )
    } else {
        format!(
            "gate failed: deduped_issues_per_run={:.2} (limit {:.2}), status_change_rate={:.2} (limit {:.2})",
            deduped_issues_per_run,
            MAX_DEDUPED_ISSUES_PER_RUN,
            status_change_rate,
            MAX_STATUS_CHANGE_RATE
        )
    };

    WatchdogEvalQualityGate {
        passed,
        reason,
        traces_evaluated,
        deduped_issues_per_run,
        status_change_rate,
    }
}

pub fn build_state_view(repo: &Path, runtime: &WatchdogRuntimeState) -> WatchdogStateView {
    let (config, config_load) = watchdog_config::load_repo_config(repo);
    let agents = load_agents_context(repo);
    let agents_path = agents.load.path.clone();
    let active_issue = runtime.active_issue.clone().or_else(|| {
        runtime
            .active_issue_context
            .as_ref()
            .map(|ctx| ctx.issue.clone())
    });

    WatchdogStateView {
        config,
        config_load,
        agents,
        agents_path,
        active_issue,
        active_issue_context: runtime.active_issue_context.clone(),
        last_snapshot: runtime.last_snapshot.clone(),
        last_validation: runtime.last_validation.clone(),
        last_trace_id: runtime.traces.last().map(|trace| trace.id.clone()),
        last_verdict: runtime.last_verdict.clone(),
        alerts: runtime.alerts.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watchdog_types::{
        WatchdogIssueContext, WatchdogIssueKind, WatchdogIssueRef, WatchdogRepoConfig,
        WatchdogSeverity, WatchdogStatus, WatchdogVerdictIssue,
    };
    use serde::Deserialize;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WatchdogEvalCaseFixture {
        name: String,
        snapshot: WatchdogSnapshot,
        #[serde(default)]
        config: Option<WatchdogRepoConfig>,
        expected: WatchdogEvalExpected,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WatchdogEvalExpected {
        status: WatchdogStatus,
        issue_kinds: Vec<WatchdogIssueKind>,
        #[serde(default)]
        allow_extra_issues: bool,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WatchdogReplayFixture {
        name: String,
        config: WatchdogRepoConfig,
        traces: Vec<WatchdogRunTrace>,
        expected: WatchdogReplayExpected,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WatchdogReplayExpected {
        traces_evaluated: usize,
        runs_with_issues: usize,
        traces_with_status_change: usize,
        issues_total_raw: usize,
        issues_total_deduped: usize,
        unique_issue_fingerprints: usize,
    }

    fn eval_fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("watchdog_eval_cases")
    }

    fn replay_fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("watchdog_replay_cases")
    }

    fn sorted_issue_kinds(mut kinds: Vec<WatchdogIssueKind>) -> Vec<WatchdogIssueKind> {
        kinds.sort_by(|a, b| issue_kind_label(a).cmp(issue_kind_label(b)));
        kinds
    }

    fn fixture_name_from_path(path: &Path) -> String {
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("<unknown>")
            .to_string()
    }

    #[test]
    fn finds_agents_md_in_repo_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("AGENTS.md"), "rules").expect("write agents");
        let found = find_agents_md(dir.path()).expect("agents path");
        assert!(found.ends_with("AGENTS.md"));
    }

    #[test]
    fn finds_nearest_agents_md_from_parent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo_dir = dir.path().join("repo");
        let nested = repo_dir.join("nested").join("deep");
        fs::create_dir_all(&nested).expect("create nested repo");
        fs::write(dir.path().join("AGENTS.md"), "top-rules").expect("write parent agents");

        let found = find_agents_md(&nested).expect("agents path");
        assert_eq!(found, dir.path().join("AGENTS.md"));
    }

    #[test]
    fn parse_agents_rules_ignores_headers_and_code_fences() {
        let content = r#"
# Header
- First rule
1. Second rule
```md
- ignore this one
```
plain sentence rule
"#;

        let rules = parse_agents_rules(content);
        assert_eq!(
            rules,
            vec![
                "First rule".to_string(),
                "Second rule".to_string(),
                "plain sentence rule".to_string()
            ]
        );
    }

    #[test]
    fn state_view_carries_runtime_issue_context() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime = WatchdogRuntimeState {
            active_issue: None,
            active_issue_context: Some(WatchdogIssueContext {
                issue: WatchdogIssueRef {
                    id: "issue-1".to_string(),
                    title: "Fix graph drift".to_string(),
                    identifier: Some("LIN-123".to_string()),
                    url: None,
                },
                summary: Some("Watch for drift from active issue".to_string()),
                acceptance_criteria: vec!["alerts include evidence".to_string()],
                non_goals: vec!["no code generation".to_string()],
                labels: vec!["watchdog".to_string()],
                priority: Some("high".to_string()),
            }),
            ..WatchdogRuntimeState::default()
        };

        let view = build_state_view(dir.path(), &runtime);
        let issue = view.active_issue.expect("active issue");
        assert_eq!(issue.id, "issue-1");
        assert_eq!(issue.identifier.as_deref(), Some("LIN-123"));
        let issue_context = view.active_issue_context.expect("active issue context");
        assert_eq!(
            issue_context.summary.as_deref(),
            Some("Watch for drift from active issue")
        );
    }

    #[test]
    fn evaluate_snapshot_reports_missing_issue_when_required() {
        let snapshot = WatchdogSnapshot {
            schema_version: 1,
            created_at: "unix:1".to_string(),
            branch: "main".to_string(),
            revision: "abc".to_string(),
            since: "@".to_string(),
            repo_config: None,
            repo_config_load: None,
            issue_context: None,
            agents_rules_count: 1,
            agents_rules: vec![],
            changed_files_total: 1,
            changed_files: vec![crate::watchdog_types::WatchdogChangedFileEvidence {
                path: "src/app.rs".to_string(),
                status: "modified".to_string(),
                diff_excerpt: vec!["+fn test() {}".to_string()],
                diff_line_count: 1,
            }],
            validation: None,
            affected_entities: vec![],
            related_entities: vec![],
            blast_radius_count: 0,
        };
        let verdict = evaluate_snapshot(&snapshot, &WatchdogRepoConfig::default());
        assert_eq!(verdict.status, WatchdogStatus::Drift);
        assert!(verdict
            .issues
            .iter()
            .any(|issue| issue.kind == WatchdogIssueKind::UnclearMapping));
    }

    #[test]
    fn evaluate_snapshot_reports_instruction_violation_for_forbidden_literal() {
        let config = WatchdogRepoConfig {
            require_active_issue: false,
            checks: crate::watchdog_types::WatchdogChecks {
                issue_alignment: false,
                semantic_alignment: false,
                instruction_compliance: true,
                missing_validation: false,
                cost_risk: false,
            },
            ..WatchdogRepoConfig::default()
        };

        let snapshot = WatchdogSnapshot {
            schema_version: 1,
            created_at: "unix:2".to_string(),
            branch: "main".to_string(),
            revision: "def".to_string(),
            since: "@".to_string(),
            repo_config: None,
            repo_config_load: None,
            issue_context: None,
            agents_rules_count: 1,
            agents_rules: vec![
                "NEVER use `git reset --hard` unless explicitly requested.".to_string()
            ],
            changed_files_total: 1,
            changed_files: vec![crate::watchdog_types::WatchdogChangedFileEvidence {
                path: "docs/recovery.md".to_string(),
                status: "modified".to_string(),
                diff_excerpt: vec!["+Use `git reset --hard` to recover quickly.".to_string()],
                diff_line_count: 1,
            }],
            validation: None,
            affected_entities: vec![],
            related_entities: vec![],
            blast_radius_count: 0,
        };

        let verdict = evaluate_snapshot(&snapshot, &config);
        assert_eq!(verdict.status, WatchdogStatus::Drift);
        let issue = verdict
            .issues
            .iter()
            .find(|issue| issue.kind == WatchdogIssueKind::InstructionViolation)
            .expect("instruction violation issue");
        assert!(issue
            .evidence
            .iter()
            .any(|entry| entry.contains("git reset --hard")));
    }

    #[test]
    fn drift_suppressed_when_related_entities_match_issue_tokens() {
        let config = WatchdogRepoConfig {
            require_active_issue: false,
            checks: crate::watchdog_types::WatchdogChecks {
                issue_alignment: true,
                semantic_alignment: false,
                instruction_compliance: false,
                missing_validation: false,
                cost_risk: false,
            },
            ..WatchdogRepoConfig::default()
        };
        // Changed file path doesn't mention "invoice" or "parser",
        // but related_entities does — drift should be suppressed.
        let snapshot = WatchdogSnapshot {
            schema_version: 1,
            created_at: "unix:9".to_string(),
            branch: "main".to_string(),
            revision: "fff".to_string(),
            since: "@".to_string(),
            repo_config: None,
            repo_config_load: None,
            issue_context: Some(crate::watchdog_types::WatchdogIssueContext {
                issue: crate::watchdog_types::WatchdogIssueRef {
                    id: "issue-42".to_string(),
                    title: "Invoice parser reliability".to_string(),
                    identifier: None,
                    url: None,
                },
                summary: None,
                acceptance_criteria: vec![],
                non_goals: vec![],
                labels: vec![],
                priority: None,
            }),
            agents_rules_count: 0,
            agents_rules: vec![],
            changed_files_total: 1,
            changed_files: vec![crate::watchdog_types::WatchdogChangedFileEvidence {
                path: "src/render/canvas.rs".to_string(),
                status: "modified".to_string(),
                diff_excerpt: vec!["+fn simulate_force() {}".to_string()],
                diff_line_count: 1,
            }],
            validation: None,
            affected_entities: vec!["simulate_force".to_string()],
            // 1-hop neighbor is the invoice parser — graph says this change IS relevant
            related_entities: vec!["parse_invoice".to_string(), "invoice_reader".to_string()],
            blast_radius_count: 3,
        };
        let verdict = evaluate_snapshot(&snapshot, &config);
        assert_eq!(
            verdict.status,
            WatchdogStatus::OnTrack,
            "drift should be suppressed when related_entities match issue tokens"
        );
    }

    #[test]
    fn drift_flagged_when_neither_files_nor_related_entities_match() {
        let config = WatchdogRepoConfig {
            require_active_issue: false,
            checks: crate::watchdog_types::WatchdogChecks {
                issue_alignment: true,
                semantic_alignment: false,
                instruction_compliance: false,
                missing_validation: false,
                cost_risk: false,
            },
            ..WatchdogRepoConfig::default()
        };
        let snapshot = WatchdogSnapshot {
            schema_version: 1,
            created_at: "unix:10".to_string(),
            branch: "main".to_string(),
            revision: "ggg".to_string(),
            since: "@".to_string(),
            repo_config: None,
            repo_config_load: None,
            issue_context: Some(crate::watchdog_types::WatchdogIssueContext {
                issue: crate::watchdog_types::WatchdogIssueRef {
                    id: "issue-43".to_string(),
                    title: "Invoice parser reliability".to_string(),
                    identifier: None,
                    url: None,
                },
                summary: None,
                acceptance_criteria: vec![],
                non_goals: vec![],
                labels: vec![],
                priority: None,
            }),
            agents_rules_count: 0,
            agents_rules: vec![],
            changed_files_total: 1,
            changed_files: vec![crate::watchdog_types::WatchdogChangedFileEvidence {
                path: "src/render/canvas.rs".to_string(),
                status: "modified".to_string(),
                diff_excerpt: vec!["+fn simulate_force() {}".to_string()],
                diff_line_count: 1,
            }],
            validation: None,
            affected_entities: vec!["simulate_force".to_string()],
            // neighbors are also unrelated
            related_entities: vec!["render_frame".to_string(), "animate_sprite".to_string()],
            blast_radius_count: 2,
        };
        let verdict = evaluate_snapshot(&snapshot, &config);
        assert_eq!(
            verdict.status,
            WatchdogStatus::Drift,
            "drift should be flagged when neither files nor related entities match issue"
        );
        assert!(
            verdict
                .issues
                .iter()
                .any(|i| i.kind == WatchdogIssueKind::Drift),
            "expected drift issue kind"
        );
    }

    #[test]
    fn dedupes_alerts_inside_cooldown_window() {
        let mut runtime = WatchdogRuntimeState::default();
        let issue = WatchdogVerdictIssue {
            kind: WatchdogIssueKind::MissingValidation,
            severity: WatchdogSeverity::Warning,
            title: "No validation record".to_string(),
            why: "Validation is missing".to_string(),
            evidence: vec!["lastValidation=None".to_string()],
            recommended_next_step: "Run tests".to_string(),
        };

        maybe_push_deduped_alert(&mut runtime, &issue, "trace-1", "unix:100", 0, 300);
        assert_eq!(runtime.alerts.len(), 1);

        maybe_push_deduped_alert(&mut runtime, &issue, "trace-2", "unix:250", 1, 300);
        assert_eq!(runtime.alerts.len(), 1);

        maybe_push_deduped_alert(&mut runtime, &issue, "trace-3", "unix:401", 2, 300);
        assert_eq!(runtime.alerts.len(), 2);
    }

    #[test]
    fn different_evidence_creates_distinct_alert_fingerprint() {
        let mut runtime = WatchdogRuntimeState::default();
        let issue_a = WatchdogVerdictIssue {
            kind: WatchdogIssueKind::Drift,
            severity: WatchdogSeverity::Warning,
            title: "Potential drift".to_string(),
            why: "Weak alignment".to_string(),
            evidence: vec!["sampleChangedFile=src/a.rs".to_string()],
            recommended_next_step: "Re-check mapping".to_string(),
        };
        let mut issue_b = issue_a.clone();
        issue_b.evidence = vec!["sampleChangedFile=src/b.rs".to_string()];

        maybe_push_deduped_alert(&mut runtime, &issue_a, "trace-a", "unix:100", 0, 300);
        maybe_push_deduped_alert(&mut runtime, &issue_b, "trace-b", "unix:120", 1, 300);

        assert_eq!(runtime.alerts.len(), 2);
    }

    #[test]
    fn evaluate_snapshot_matches_fixture_corpus() {
        let fixture_dir = eval_fixture_dir();
        let entries = fs::read_dir(&fixture_dir).expect("read watchdog eval fixture dir");
        let mut case_paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect();
        case_paths.sort();

        assert!(
            !case_paths.is_empty(),
            "expected at least one watchdog eval fixture in {}",
            fixture_dir.display()
        );

        for path in case_paths {
            let fixture_name = fixture_name_from_path(&path);
            let raw = fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("read fixture {fixture_name}: {err}"));
            let case: WatchdogEvalCaseFixture = serde_json::from_str(&raw)
                .unwrap_or_else(|err| panic!("parse fixture {fixture_name}: {err}"));

            let config = case.config.unwrap_or_default();
            let verdict = evaluate_snapshot(&case.snapshot, &config);
            assert_eq!(
                verdict.status, case.expected.status,
                "fixture {} ({}): unexpected status",
                fixture_name, case.name
            );

            let expected = sorted_issue_kinds(case.expected.issue_kinds);
            let actual = sorted_issue_kinds(
                verdict
                    .issues
                    .iter()
                    .map(|issue| issue.kind.clone())
                    .collect(),
            );

            if case.expected.allow_extra_issues {
                for required_kind in expected {
                    assert!(
                        actual.contains(&required_kind),
                        "fixture {} ({}): expected issue kind '{}' not found in {:?}",
                        fixture_name,
                        case.name,
                        issue_kind_label(&required_kind),
                        actual
                    );
                }
            } else {
                assert_eq!(
                    actual, expected,
                    "fixture {} ({}): unexpected issue kinds",
                    fixture_name, case.name
                );
            }
        }
    }

    #[test]
    fn replay_eval_report_tracks_spam_rate_and_status_regressions() {
        let config = WatchdogRepoConfig {
            require_active_issue: false,
            dedupe_window_seconds: 300,
            checks: crate::watchdog_types::WatchdogChecks {
                issue_alignment: false,
                semantic_alignment: false,
                instruction_compliance: true,
                missing_validation: true,
                cost_risk: false,
            },
            ..WatchdogRepoConfig::default()
        };

        let snapshot = WatchdogSnapshot {
            schema_version: 1,
            created_at: "unix:1".to_string(),
            branch: "main".to_string(),
            revision: "abc".to_string(),
            since: "@".to_string(),
            repo_config: None,
            repo_config_load: None,
            issue_context: None,
            agents_rules_count: 0,
            agents_rules: vec![],
            changed_files_total: 1,
            changed_files: vec![crate::watchdog_types::WatchdogChangedFileEvidence {
                path: "src/lib.rs".to_string(),
                status: "modified".to_string(),
                diff_excerpt: vec!["+fn thing() {}".to_string()],
                diff_line_count: 1,
            }],
            validation: None,
            affected_entities: vec![],
            related_entities: vec![],
            blast_radius_count: 0,
        };

        let traces = vec![
            WatchdogRunTrace {
                id: "trace-1".to_string(),
                created_at: "unix:100".to_string(),
                snapshot: snapshot.clone(),
                verdict: WatchdogVerdict {
                    status: WatchdogStatus::Drift,
                    confidence: 0.62,
                    summary: "old".to_string(),
                    issues: vec![],
                },
            },
            WatchdogRunTrace {
                id: "trace-2".to_string(),
                created_at: "unix:120".to_string(),
                snapshot,
                verdict: WatchdogVerdict {
                    status: WatchdogStatus::OnTrack,
                    confidence: 0.85,
                    summary: "old".to_string(),
                    issues: vec![],
                },
            },
        ];

        let report = build_replay_eval_report(&traces, &config);
        assert_eq!(report.traces_evaluated, 2);
        assert_eq!(report.spam.runs_with_issues, 2);
        assert_eq!(report.spam.issues_total_raw, 2);
        assert_eq!(report.spam.issues_total_deduped, 1);
        assert_eq!(report.spam.unique_issue_fingerprints, 1);
        assert_eq!(report.spam.traces_with_status_change, 1);
        assert!((report.spam.average_issues_per_run_raw - 1.0).abs() < f32::EPSILON);
        assert!((report.spam.average_issues_per_run_deduped - 0.5).abs() < f32::EPSILON);
        assert_eq!(report.results.len(), 2);
        assert_eq!(
            report
                .results
                .iter()
                .filter(|result| result.status_changed)
                .count(),
            1
        );
    }

    #[test]
    fn replay_eval_matches_fixture_corpus() {
        let fixture_dir = replay_fixture_dir();
        let entries = fs::read_dir(&fixture_dir).expect("read watchdog replay fixture dir");
        let mut case_paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect();
        case_paths.sort();

        assert!(
            !case_paths.is_empty(),
            "expected at least one watchdog replay fixture in {}",
            fixture_dir.display()
        );

        for path in case_paths {
            let fixture_name = fixture_name_from_path(&path);
            let raw = fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("read replay fixture {fixture_name}: {err}"));
            let case: WatchdogReplayFixture = serde_json::from_str(&raw)
                .unwrap_or_else(|err| panic!("parse replay fixture {fixture_name}: {err}"));
            let report = build_replay_eval_report(&case.traces, &case.config);

            assert_eq!(
                report.traces_evaluated, case.expected.traces_evaluated,
                "replay fixture {} ({}): traces_evaluated mismatch",
                fixture_name, case.name
            );
            assert_eq!(
                report.spam.runs_with_issues, case.expected.runs_with_issues,
                "replay fixture {} ({}): runs_with_issues mismatch",
                fixture_name, case.name
            );
            assert_eq!(
                report.spam.traces_with_status_change, case.expected.traces_with_status_change,
                "replay fixture {} ({}): traces_with_status_change mismatch",
                fixture_name, case.name
            );
            assert_eq!(
                report.spam.issues_total_raw, case.expected.issues_total_raw,
                "replay fixture {} ({}): issues_total_raw mismatch",
                fixture_name, case.name
            );
            assert_eq!(
                report.spam.issues_total_deduped, case.expected.issues_total_deduped,
                "replay fixture {} ({}): issues_total_deduped mismatch",
                fixture_name, case.name
            );
            assert_eq!(
                report.spam.unique_issue_fingerprints, case.expected.unique_issue_fingerprints,
                "replay fixture {} ({}): unique_issue_fingerprints mismatch",
                fixture_name, case.name
            );
        }
    }
}

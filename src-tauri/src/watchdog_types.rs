use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogIssueRef {
    pub id: String,
    pub title: String,
    pub identifier: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogIssueContext {
    pub issue: WatchdogIssueRef,
    pub summary: Option<String>,
    pub acceptance_criteria: Vec<String>,
    pub non_goals: Vec<String>,
    pub labels: Vec<String>,
    pub priority: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogValidationRecord {
    pub kind: String,
    pub success: bool,
    pub ran_at: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogStatus {
    OnTrack,
    Drift,
    Blocked,
    InsufficientContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogIssueKind {
    Drift,
    InstructionViolation,
    MissingValidation,
    CostRisk,
    UnclearMapping,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogVerdictIssue {
    pub kind: WatchdogIssueKind,
    pub severity: WatchdogSeverity,
    pub title: String,
    pub why: String,
    pub evidence: Vec<String>,
    pub recommended_next_step: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogVerdict {
    pub status: WatchdogStatus,
    pub confidence: f32,
    pub summary: String,
    pub issues: Vec<WatchdogVerdictIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogChangedFileEvidence {
    pub path: String,
    pub status: String,
    pub diff_excerpt: Vec<String>,
    pub diff_line_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogSnapshot {
    pub schema_version: u32,
    pub created_at: String,
    pub branch: String,
    pub revision: String,
    pub since: String,
    #[serde(default)]
    pub repo_config: Option<WatchdogRepoConfig>,
    #[serde(default)]
    pub repo_config_load: Option<WatchdogConfigLoadResult>,
    pub issue_context: Option<WatchdogIssueContext>,
    pub agents_rules_count: usize,
    #[serde(default)]
    pub agents_rules: Vec<String>,
    pub changed_files_total: usize,
    pub changed_files: Vec<WatchdogChangedFileEvidence>,
    pub validation: Option<WatchdogValidationRecord>,
    /// Display names of entities whose file is among the changed files.
    #[serde(default)]
    pub affected_entities: Vec<String>,
    /// Display names of entities adjacent (1-hop) to affected entities but not in changed files.
    #[serde(default)]
    pub related_entities: Vec<String>,
    /// Total count of entities reachable (2-hop) from changed files in the canonical graph.
    #[serde(default)]
    pub blast_radius_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogAlert {
    pub id: String,
    pub kind: WatchdogIssueKind,
    pub severity: WatchdogSeverity,
    pub title: String,
    pub message: String,
    pub evidence: Vec<String>,
    pub created_at: String,
    pub verdict_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogRunTrace {
    pub id: String,
    pub created_at: String,
    pub snapshot: WatchdogSnapshot,
    pub verdict: WatchdogVerdict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogIssueKindCount {
    pub kind: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogSpamRateReport {
    pub runs_total: usize,
    pub runs_with_issues: usize,
    pub traces_with_status_change: usize,
    pub issues_total_raw: usize,
    pub issues_total_deduped: usize,
    pub unique_issue_fingerprints: usize,
    pub average_issues_per_run_raw: f32,
    pub average_issues_per_run_deduped: f32,
    pub issue_kind_counts: Vec<WatchdogIssueKindCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogReplayEvalTraceResult {
    pub trace_id: String,
    pub expected_status: WatchdogStatus,
    pub actual_status: WatchdogStatus,
    pub status_changed: bool,
    pub raw_issue_count: usize,
    pub deduped_issue_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogReplayEvalReport {
    pub generated_at: String,
    pub traces_evaluated: usize,
    pub config_dedupe_window_seconds: u64,
    pub spam: WatchdogSpamRateReport,
    pub results: Vec<WatchdogReplayEvalTraceResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogTriggerMode {
    ManualOnly,
    BeforePush,
    OnLargeChangeBatch,
    OnBranchSwitchAfterQuietPeriod,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogAlertThreshold {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogChecks {
    pub issue_alignment: bool,
    #[serde(default)]
    pub semantic_alignment: bool,
    pub instruction_compliance: bool,
    pub missing_validation: bool,
    pub cost_risk: bool,
}

impl Default for WatchdogChecks {
    fn default() -> Self {
        Self {
            issue_alignment: true,
            semantic_alignment: false,
            instruction_compliance: true,
            missing_validation: true,
            cost_risk: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogRepoConfig {
    pub enabled: bool,
    pub trigger_mode: WatchdogTriggerMode,
    pub require_active_issue: bool,
    pub max_files_in_snapshot: usize,
    pub max_diff_lines_per_file: usize,
    pub dedupe_window_seconds: u64,
    pub alert_threshold: WatchdogAlertThreshold,
    pub checks: WatchdogChecks,
}

impl Default for WatchdogRepoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trigger_mode: WatchdogTriggerMode::ManualOnly,
            require_active_issue: true,
            max_files_in_snapshot: 20,
            max_diff_lines_per_file: 200,
            dedupe_window_seconds: 300,
            alert_threshold: WatchdogAlertThreshold::Warning,
            checks: WatchdogChecks::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogConfigLoadResult {
    pub path: String,
    pub exists: bool,
    pub loaded: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogAgentsLoadResult {
    pub path: Option<String>,
    pub exists: bool,
    pub loaded: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogAgentsContext {
    pub load: WatchdogAgentsLoadResult,
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogStateView {
    pub config: WatchdogRepoConfig,
    pub config_load: WatchdogConfigLoadResult,
    pub agents: WatchdogAgentsContext,
    pub agents_path: Option<String>,
    pub active_issue: Option<WatchdogIssueRef>,
    pub active_issue_context: Option<WatchdogIssueContext>,
    pub last_snapshot: Option<WatchdogSnapshot>,
    pub last_validation: Option<WatchdogValidationRecord>,
    pub last_trace_id: Option<String>,
    pub last_verdict: Option<WatchdogVerdict>,
    pub alerts: Vec<WatchdogAlert>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchdogMcpStatus {
    pub generated_at: String,
    pub status: WatchdogStatus,
    pub summary: String,
    pub confidence: f32,
    pub active_issue: Option<WatchdogIssueRef>,
    pub alerts_count: usize,
    pub last_trace_id: Option<String>,
    pub branch: String,
    pub revision: String,
}

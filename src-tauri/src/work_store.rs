/// SQLite-backed store for conductor work plans and work units.
///
/// Schema lives in `.sloposcope/conductor.sqlite3` next to `graph.sqlite3`.
///
/// Lifecycle:
///   plan_task  → creates a Plan + N WorkUnits (status=pending)
///   claim_work_unit → atomically picks the oldest pending unit, marks it claimed
///   submit_verdict  → records the outcome, marks the unit done or failed
///   list_work_units → read-only snapshot of all units in a plan
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkUnitStatus {
    Pending,
    Claimed,
    Running,
    RetryScheduled,
    Done,
    Failed,
}

impl WorkUnitStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Running => "running",
            Self::RetryScheduled => "retry_scheduled",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "claimed" => Self::Claimed,
            "running" => Self::Running,
            "retry_scheduled" => Self::RetryScheduled,
            "done" => Self::Done,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkUnit {
    pub id: String,
    pub task_id: String,
    pub title: String,
    pub prompt: String,
    pub status: WorkUnitStatus,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub started_at: Option<String>,
    pub retry_at: Option<String>,
    pub reconciled_at: Option<String>,
    pub run_attempt: u32,
    pub retry_count: u32,
    pub max_retries: u32,
    pub last_error: Option<String>,
    pub reconciled_outcome: Option<String>,
    pub reconciled_note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRecord {
    pub task_id: String,
    pub workflow_id: String,
    pub title: String,
    pub prompt: String,
    pub max_retries: u32,
    pub timeout_seconds: u64,
    pub issue_id: Option<String>,
    pub issue_title: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskLifecycleEventType {
    #[serde(rename = "task.created")]
    TaskCreated,
    #[serde(rename = "plan.proposed")]
    PlanProposed,
    #[serde(rename = "plan.approved")]
    PlanApproved,
    #[serde(rename = "worker.started")]
    WorkerStarted,
    #[serde(rename = "verification.failed")]
    VerificationFailed,
    #[serde(rename = "task.retry_scheduled")]
    TaskRetryScheduled,
    #[serde(rename = "session.stale")]
    SessionStale,
    #[serde(rename = "workers.launched")]
    WorkersLaunched,
    #[serde(rename = "coordinator.reported")]
    CoordinatorReported,
    #[serde(rename = "review.accepted")]
    ReviewAccepted,
    #[serde(rename = "review.rejected")]
    ReviewRejected,
    #[serde(rename = "patch.applied")]
    PatchApplied,
}

impl TaskLifecycleEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TaskCreated => "task.created",
            Self::PlanProposed => "plan.proposed",
            Self::PlanApproved => "plan.approved",
            Self::WorkerStarted => "worker.started",
            Self::VerificationFailed => "verification.failed",
            Self::TaskRetryScheduled => "task.retry_scheduled",
            Self::SessionStale => "session.stale",
            Self::WorkersLaunched => "workers.launched",
            Self::CoordinatorReported => "coordinator.reported",
            Self::ReviewAccepted => "review.accepted",
            Self::ReviewRejected => "review.rejected",
            Self::PatchApplied => "patch.applied",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLifecycleEvent {
    pub id: String,
    pub task_id: String,
    pub thread_id: String,
    #[serde(rename = "type")]
    pub r#type: TaskLifecycleEventType,
    pub actor: String,
    pub source: Option<String>,
    pub correlation: Option<String>,
    pub causation: Option<String>,
    pub occurred_at: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitVerdict {
    pub unit_id: String,
    pub task_id: String,
    pub status: String, // "done" | "failed"
    pub summary: String,
    pub submitted_at: String,
    pub artifacts: Vec<VerdictArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerdictArtifact {
    pub kind: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewWorkUnit {
    pub id: String,
    pub title: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionInput {
    pub id: Option<String>,
    pub title: String,
    pub brief: String,
    pub validation_command: String,
    pub planner_notes: Option<String>,
    pub planner_profile_id: Option<String>,
    pub planner_prompt_override: Option<String>,
    pub issue_key: Option<String>,
    pub issue_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionRecord {
    pub id: String,
    pub repo_path: String,
    pub worktree_path: String,
    pub graph_snapshot: String,
    pub title: String,
    pub brief: String,
    pub validation_command: String,
    pub planner_notes: Option<String>,
    pub planner_profile_id: Option<String>,
    pub planner_prompt_override: Option<String>,
    pub issue_key: Option<String>,
    pub issue_url: Option<String>,
    pub instruction_pack_hash: String,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectiveMeta {
    pub scope_paths: Vec<String>,
    pub expected_validation: Vec<String>,
    pub done_criteria: Vec<String>,
    pub test_intent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectiveRecord {
    pub id: String,
    pub mission_id: String,
    pub title: String,
    pub prompt: String,
    pub status: WorkUnitStatus,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub created_at: String,
    pub meta: ObjectiveMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewObjective {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub scope_paths: Vec<String>,
    pub expected_validation: Vec<String>,
    pub done_criteria: Vec<String>,
    pub test_intent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRecord {
    pub id: String,
    pub mission_id: String,
    pub objective_id: Option<String>,
    pub profile_id: String,
    pub assignment_kind: String,
    pub state: String,
    pub touched_paths: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRuntimeRecord {
    pub id: String,
    pub mission_id: String,
    pub objective_id: Option<String>,
    pub profile_id: String,
    pub assignment_kind: String,
    pub state: String,
    pub touched_paths: Vec<String>,
    pub workspace_path: String,
    pub lease_expires_at: Option<String>,
    pub last_heartbeat_at: Option<String>,
    pub last_event_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub const AGENT_SESSION_RUNNING_WINDOW_SECS: u64 = 30;
pub const AGENT_SESSION_STUCK_HEARTBEAT_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRuntimeSlice {
    pub runtime_status: String,
    pub heartbeat_age_sec: Option<u64>,
    pub event_age_sec: Option<u64>,
    pub lease_expired: bool,
}

fn current_unix_seconds() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn session_lease_label_from_now(now_unix_seconds: u64) -> String {
    format!(
        "unix:{}",
        now_unix_seconds.saturating_add(AGENT_SESSION_STUCK_HEARTBEAT_TIMEOUT_SECS)
    )
}

fn parse_unix_label_seconds(label: Option<&str>) -> Option<u64> {
    let raw = label?.strip_prefix("unix:")?;
    raw.parse::<u64>().ok()
}

fn normalize_terminal_runtime_status(state: &str) -> Option<&'static str> {
    match state.trim().to_ascii_lowercase().as_str() {
        "completed" | "done" | "passed" | "logged_out" => Some("completed"),
        "failed" => Some("failed"),
        "applied" => Some("applied"),
        _ => None,
    }
}

pub fn derive_agent_session_runtime_slice_at(
    session: &AgentSessionRuntimeRecord,
    now_unix_seconds: u64,
) -> AgentSessionRuntimeSlice {
    let heartbeat_age_sec = parse_unix_label_seconds(session.last_heartbeat_at.as_deref())
        .map(|timestamp| now_unix_seconds.saturating_sub(timestamp));
    let event_age_sec = parse_unix_label_seconds(session.last_event_at.as_deref())
        .map(|timestamp| now_unix_seconds.saturating_sub(timestamp));
    let lease_expired = parse_unix_label_seconds(session.lease_expires_at.as_deref())
        .map(|timestamp| timestamp <= now_unix_seconds)
        .unwrap_or(false);

    let runtime_status = if let Some(status) = normalize_terminal_runtime_status(&session.state) {
        status.to_string()
    } else if lease_expired
        || heartbeat_age_sec
            .map(|age| age > AGENT_SESSION_STUCK_HEARTBEAT_TIMEOUT_SECS)
            .unwrap_or(false)
    {
        "stuck".to_string()
    } else if heartbeat_age_sec
        .map(|age| age <= AGENT_SESSION_RUNNING_WINDOW_SECS)
        .unwrap_or(false)
        || event_age_sec
            .map(|age| age <= AGENT_SESSION_RUNNING_WINDOW_SECS)
            .unwrap_or(false)
    {
        "running".to_string()
    } else {
        "idle".to_string()
    };

    AgentSessionRuntimeSlice {
        runtime_status,
        heartbeat_age_sec,
        event_age_sec,
        lease_expired,
    }
}

pub fn derive_agent_session_runtime_slice(
    session: &AgentSessionRuntimeRecord,
) -> AgentSessionRuntimeSlice {
    derive_agent_session_runtime_slice_at(session, current_unix_seconds())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub model: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub env: serde_json::Value,
    pub capabilities: Vec<String>,
    pub transport_kind: String,
    pub active: bool,
    pub is_reviewer: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchDecisionRecord {
    pub mission_id: String,
    pub objective_id: String,
    pub session_id: Option<String>,
    pub status: String,
    pub reason: Option<String>,
    pub patch_base_revision: Option<String>,
    pub apply_target_path: Option<String>,
    pub apply_branch: Option<String>,
    pub applied_at: Option<String>,
    pub apply_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchGateRecord {
    pub mission_id: String,
    pub objective_id: String,
    pub session_id: Option<String>,
    pub status: String,
    pub reasons: Vec<String>,
    pub watchdog_status: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewVerificationRecord {
    pub mission_id: String,
    pub objective_id: Option<String>,
    pub session_id: Option<String>,
    pub status: String,
    pub summary: String,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationRecord {
    pub id: i64,
    pub mission_id: String,
    pub objective_id: Option<String>,
    pub session_id: Option<String>,
    pub status: String,
    pub summary: String,
    pub details: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionSummary {
    pub total_objectives: usize,
    pub pending_objectives: usize,
    pub claimed_objectives: usize,
    pub done_objectives: usize,
    pub failed_objectives: usize,
    pub active_sessions: usize,
    pub last_event_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionStateView {
    pub mission: Option<MissionRecord>,
    pub objectives: Vec<ObjectiveRecord>,
    pub sessions: Vec<AgentSessionRuntimeRecord>,
    pub profiles: Vec<AgentProfile>,
    pub patch_decisions: Vec<PatchDecisionRecord>,
    pub patch_gates: Vec<PatchGateRecord>,
    pub verifications: Vec<VerificationRecord>,
    pub events: Vec<RunEventRecord>,
    pub summary: MissionSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorDraftMission {
    pub title: String,
    pub brief: String,
    pub validation_command: String,
    pub issue_key: Option<String>,
    pub issue_url: Option<String>,
    pub planner_profile_id: Option<String>,
    pub planner_notes: Option<String>,
    pub planner_prompt_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorThreadRecord {
    pub id: String,
    pub repo_path: String,
    pub mission_id: Option<String>,
    pub persona_name: String,
    pub title: String,
    pub state: String,
    pub draft_mission: CoordinatorDraftMission,
    pub pending_plan: Vec<NewObjective>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorThreadSummary {
    pub id: String,
    pub mission_id: Option<String>,
    pub title: String,
    pub state: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorMessageRecord {
    pub id: i64,
    pub thread_id: String,
    pub mission_id: Option<String>,
    pub objective_id: Option<String>,
    pub session_id: Option<String>,
    pub role: String,
    pub kind: String,
    pub content: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewCoordinatorMessage<'a> {
    pub thread_id: &'a str,
    pub mission_id: Option<&'a str>,
    pub objective_id: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub role: &'a str,
    pub kind: &'a str,
    pub content: &'a str,
    pub payload: serde_json::Value,
}

/// A single event emitted by an external runner for a work unit session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEventRecord {
    pub id: i64,
    pub task_id: String,
    pub unit_id: String,
    pub session_id: String,
    pub event_type: String, // started | progress | completed | failed
    pub payload: serde_json::Value,
    pub created_at: String,
}

fn lifecycle_payload_with_base(
    payload: serde_json::Value,
    objective_id: Option<&str>,
    session_id: Option<&str>,
) -> serde_json::Value {
    let mut object = match payload {
        serde_json::Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("value".to_string(), other);
            map
        }
    };
    if let Some(objective_id) = objective_id.filter(|value| !value.is_empty()) {
        object
            .entry("objectiveId".to_string())
            .or_insert_with(|| serde_json::Value::String(objective_id.to_string()));
    }
    if let Some(session_id) = session_id.filter(|value| !value.is_empty()) {
        object
            .entry("sessionId".to_string())
            .or_insert_with(|| serde_json::Value::String(session_id.to_string()));
    }
    serde_json::Value::Object(object)
}

fn lifecycle_message_type(message: &CoordinatorMessageRecord) -> Option<TaskLifecycleEventType> {
    match (message.role.as_str(), message.kind.as_str()) {
        ("user", "prompt") => Some(TaskLifecycleEventType::TaskCreated),
        (_, "plan") => Some(TaskLifecycleEventType::PlanProposed),
        (_, "report") => Some(TaskLifecycleEventType::CoordinatorReported),
        (_, "status")
            if message
                .content
                .to_ascii_lowercase()
                .starts_with("plan approved")
                || message.payload.get("missionId").is_some() =>
        {
            Some(TaskLifecycleEventType::PlanApproved)
        }
        (_, "status")
            if message
                .content
                .to_ascii_lowercase()
                .starts_with("workers launched") =>
        {
            Some(TaskLifecycleEventType::WorkersLaunched)
        }
        _ => None,
    }
}

fn lifecycle_run_event_type(event: &RunEventRecord) -> Option<TaskLifecycleEventType> {
    match event.event_type.as_str() {
        "started" => Some(TaskLifecycleEventType::WorkerStarted),
        "failed" => Some(TaskLifecycleEventType::VerificationFailed),
        "retry_scheduled" | "retry" => Some(TaskLifecycleEventType::TaskRetryScheduled),
        "stale" | "stale_swept" => Some(TaskLifecycleEventType::SessionStale),
        _ => None,
    }
}

pub fn project_task_lifecycle_events_at(
    task_id: &str,
    thread_id: &str,
    messages: &[CoordinatorMessageRecord],
    run_events: &[RunEventRecord],
    sessions: &[AgentSessionRuntimeRecord],
    patch_decisions: &[PatchDecisionRecord],
    verifications: &[VerificationRecord],
    now_unix_seconds: u64,
) -> Vec<TaskLifecycleEvent> {
    let mut events = Vec::new();
    for message in messages {
        if let Some(event_type) = lifecycle_message_type(message) {
            events.push(TaskLifecycleEvent {
                id: format!("coordinator-message-{}", message.id),
                task_id: message
                    .mission_id
                    .clone()
                    .unwrap_or_else(|| task_id.to_string()),
                thread_id: message.thread_id.clone(),
                r#type: event_type,
                actor: message.role.clone(),
                source: Some("coordinator_message".to_string()),
                correlation: Some(thread_id.to_string()),
                causation: None,
                occurred_at: message.created_at.clone(),
                payload: lifecycle_payload_with_base(
                    serde_json::json!({
                        "messageId": message.id,
                        "kind": message.kind,
                        "content": message.content,
                        "payload": message.payload,
                    }),
                    message.objective_id.as_deref(),
                    message.session_id.as_deref(),
                ),
            });
        }
    }
    for event in run_events {
        if let Some(event_type) = lifecycle_run_event_type(event) {
            events.push(TaskLifecycleEvent {
                id: format!("run-event-{}", event.id),
                task_id: event.task_id.clone(),
                thread_id: thread_id.to_string(),
                r#type: event_type,
                actor: event.session_id.clone(),
                source: Some("run_event".to_string()),
                correlation: Some(event.task_id.clone()),
                causation: None,
                occurred_at: event.created_at.clone(),
                payload: lifecycle_payload_with_base(
                    event.payload.clone(),
                    Some(&event.unit_id),
                    Some(&event.session_id),
                ),
            });
        }
    }
    for session in sessions {
        events.push(TaskLifecycleEvent {
            id: format!("session-{}-started", session.id),
            task_id: session.mission_id.clone(),
            thread_id: thread_id.to_string(),
            r#type: TaskLifecycleEventType::WorkerStarted,
            actor: session.profile_id.clone(),
            source: Some("agent_session".to_string()),
            correlation: Some(session.mission_id.clone()),
            causation: None,
            occurred_at: session.created_at.clone(),
            payload: lifecycle_payload_with_base(
                serde_json::json!({
                    "profileId": session.profile_id,
                    "state": session.state,
                    "workspacePath": session.workspace_path,
                    "touchedPaths": session.touched_paths,
                }),
                session.objective_id.as_deref(),
                Some(&session.id),
            ),
        });
        let runtime = derive_agent_session_runtime_slice_at(session, now_unix_seconds);
        if runtime.runtime_status == "stuck"
            || session
                .last_error
                .as_deref()
                .map(|error| error.contains("stale"))
                .unwrap_or(false)
        {
            events.push(TaskLifecycleEvent {
                id: format!("session-{}-stale", session.id),
                task_id: session.mission_id.clone(),
                thread_id: thread_id.to_string(),
                r#type: TaskLifecycleEventType::SessionStale,
                actor: "scheduler".to_string(),
                source: Some("agent_session".to_string()),
                correlation: Some(session.mission_id.clone()),
                causation: Some(format!("session-{}-started", session.id)),
                occurred_at: session
                    .last_event_at
                    .clone()
                    .unwrap_or_else(|| session.updated_at.clone()),
                payload: lifecycle_payload_with_base(
                    serde_json::json!({
                        "runtimeStatus": runtime.runtime_status,
                        "heartbeatAgeSec": runtime.heartbeat_age_sec,
                        "eventAgeSec": runtime.event_age_sec,
                        "leaseExpired": runtime.lease_expired,
                        "lastError": session.last_error,
                    }),
                    session.objective_id.as_deref(),
                    Some(&session.id),
                ),
            });
        }
    }
    for verification in verifications {
        let validation_failed = verification
            .details
            .get("validation")
            .and_then(|entry| entry.get("success"))
            .and_then(serde_json::Value::as_bool)
            .map(|ok| !ok)
            .unwrap_or(false);
        if verification.status == "failed" || validation_failed {
            events.push(TaskLifecycleEvent {
                id: format!("verification-{}", verification.id),
                task_id: verification.mission_id.clone(),
                thread_id: thread_id.to_string(),
                r#type: TaskLifecycleEventType::VerificationFailed,
                actor: "verifier".to_string(),
                source: Some("verification".to_string()),
                correlation: Some(verification.mission_id.clone()),
                causation: verification
                    .session_id
                    .as_ref()
                    .map(|session_id| format!("session-{session_id}-started")),
                occurred_at: verification.created_at.clone(),
                payload: lifecycle_payload_with_base(
                    serde_json::json!({
                        "verificationId": format!("verification-{}", verification.id),
                        "status": verification.status,
                        "summary": verification.summary,
                        "details": verification.details,
                    }),
                    verification.objective_id.as_deref(),
                    verification.session_id.as_deref(),
                ),
            });
        }
        if verification
            .details
            .get("retryScheduled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            || verification.details.get("retryAt").is_some()
        {
            events.push(TaskLifecycleEvent {
                id: format!("verification-{}-retry", verification.id),
                task_id: verification.mission_id.clone(),
                thread_id: thread_id.to_string(),
                r#type: TaskLifecycleEventType::TaskRetryScheduled,
                actor: "scheduler".to_string(),
                source: Some("verification".to_string()),
                correlation: Some(verification.mission_id.clone()),
                causation: Some(format!("verification-{}", verification.id)),
                occurred_at: verification.updated_at.clone(),
                payload: lifecycle_payload_with_base(
                    serde_json::json!({
                        "verificationId": format!("verification-{}", verification.id),
                        "retryAt": verification.details.get("retryAt"),
                    }),
                    verification.objective_id.as_deref(),
                    verification.session_id.as_deref(),
                ),
            });
        }
    }
    for decision in patch_decisions {
        let status = decision.status.as_str();
        if matches!(status, "accepted" | "applied" | "apply_failed") {
            events.push(TaskLifecycleEvent {
                id: format!(
                    "patch-decision-{}-{}-review-accepted",
                    decision.mission_id, decision.objective_id
                ),
                task_id: decision.mission_id.clone(),
                thread_id: thread_id.to_string(),
                r#type: TaskLifecycleEventType::ReviewAccepted,
                actor: "reviewer".to_string(),
                source: Some("patch_decision".to_string()),
                correlation: Some(decision.mission_id.clone()),
                causation: decision
                    .session_id
                    .as_ref()
                    .map(|session_id| format!("session-{session_id}-started")),
                occurred_at: decision.created_at.clone(),
                payload: lifecycle_payload_with_base(
                    serde_json::json!({
                        "status": decision.status,
                        "reason": decision.reason,
                        "patchBaseRevision": decision.patch_base_revision,
                    }),
                    Some(&decision.objective_id),
                    decision.session_id.as_deref(),
                ),
            });
        }
        if status == "rejected" {
            events.push(TaskLifecycleEvent {
                id: format!(
                    "patch-decision-{}-{}-review-rejected",
                    decision.mission_id, decision.objective_id
                ),
                task_id: decision.mission_id.clone(),
                thread_id: thread_id.to_string(),
                r#type: TaskLifecycleEventType::ReviewRejected,
                actor: "reviewer".to_string(),
                source: Some("patch_decision".to_string()),
                correlation: Some(decision.mission_id.clone()),
                causation: decision
                    .session_id
                    .as_ref()
                    .map(|session_id| format!("session-{session_id}-started")),
                occurred_at: decision.created_at.clone(),
                payload: lifecycle_payload_with_base(
                    serde_json::json!({
                        "status": decision.status,
                        "reason": decision.reason,
                    }),
                    Some(&decision.objective_id),
                    decision.session_id.as_deref(),
                ),
            });
        }
        if status == "applied" {
            events.push(TaskLifecycleEvent {
                id: format!(
                    "patch-decision-{}-{}-patch-applied",
                    decision.mission_id, decision.objective_id
                ),
                task_id: decision.mission_id.clone(),
                thread_id: thread_id.to_string(),
                r#type: TaskLifecycleEventType::PatchApplied,
                actor: "system".to_string(),
                source: Some("patch_decision".to_string()),
                correlation: Some(decision.mission_id.clone()),
                causation: Some(format!(
                    "patch-decision-{}-{}-review-accepted",
                    decision.mission_id, decision.objective_id
                )),
                occurred_at: decision
                    .applied_at
                    .clone()
                    .unwrap_or_else(|| decision.updated_at.clone()),
                payload: lifecycle_payload_with_base(
                    serde_json::json!({
                        "status": decision.status,
                        "branch": decision.apply_branch,
                        "targetPath": decision.apply_target_path,
                    }),
                    Some(&decision.objective_id),
                    decision.session_id.as_deref(),
                ),
            });
        }
    }

    events.sort_by(|left, right| {
        lifecycle_sort_key(&left.occurred_at)
            .cmp(&lifecycle_sort_key(&right.occurred_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut previous_id: Option<String> = None;
    for event in &mut events {
        if event.causation.is_none() {
            event.causation = previous_id.clone();
        }
        previous_id = Some(event.id.clone());
    }
    events
}

fn lifecycle_sort_key(label: &str) -> (u8, u64, &str) {
    if let Some(value) = label
        .strip_prefix("unix:")
        .and_then(|raw| raw.parse::<u64>().ok())
    {
        return (0, value, "");
    }
    (1, 0, label)
}

pub fn project_task_lifecycle_events(
    task_id: &str,
    thread_id: &str,
    messages: &[CoordinatorMessageRecord],
    run_events: &[RunEventRecord],
    sessions: &[AgentSessionRuntimeRecord],
    verifications: &[VerificationRecord],
) -> Vec<TaskLifecycleEvent> {
    project_task_lifecycle_events_at(
        task_id,
        thread_id,
        messages,
        run_events,
        sessions,
        &[],
        verifications,
        current_unix_seconds(),
    )
}

// ── Store ─────────────────────────────────────────────────────────────────────

pub struct WorkStore {
    conn: Connection,
}

impl WorkStore {
    pub fn open_or_create(repo: &Path) -> Result<Self, String> {
        let dir = repo.join(".sloposcope");
        std::fs::create_dir_all(&dir).map_err(|e| format!("create .sloposcope dir: {e}"))?;
        let path = dir.join("conductor.sqlite3");
        let conn = Connection::open(&path).map_err(|e| format!("open conductor db: {e}"))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), String> {
        self.conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS plan (
                task_id           TEXT PRIMARY KEY,
                workflow_id       TEXT NOT NULL DEFAULT '',
                title             TEXT NOT NULL,
                prompt            TEXT NOT NULL,
                max_retries       INTEGER NOT NULL DEFAULT 2,
                timeout_seconds   INTEGER NOT NULL DEFAULT 900,
                issue_id          TEXT,
                issue_title       TEXT,
                created_at        TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS work_unit (
                id                TEXT NOT NULL,
                task_id           TEXT NOT NULL,
                title             TEXT NOT NULL,
                prompt            TEXT NOT NULL,
                status            TEXT NOT NULL DEFAULT 'pending',
                claimed_by        TEXT,
                claimed_at        TEXT,
                started_at        TEXT,
                retry_at          TEXT,
                reconciled_at     TEXT,
                run_attempt       INTEGER NOT NULL DEFAULT 0,
                retry_count       INTEGER NOT NULL DEFAULT 0,
                max_retries       INTEGER NOT NULL DEFAULT 0,
                last_error        TEXT,
                reconciled_outcome TEXT,
                reconciled_note   TEXT,
                created_at        TEXT NOT NULL,
                PRIMARY KEY (id, task_id)
            );
            CREATE TABLE IF NOT EXISTS unit_verdict (
                unit_id        TEXT NOT NULL,
                task_id        TEXT NOT NULL,
                status         TEXT NOT NULL,
                summary        TEXT NOT NULL,
                submitted_at   TEXT NOT NULL,
                artifacts_json TEXT NOT NULL DEFAULT '[]',
                PRIMARY KEY (unit_id, task_id)
            );
            CREATE TABLE IF NOT EXISTS verification (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                mission_id      TEXT NOT NULL,
                objective_id    TEXT,
                session_id      TEXT,
                status          TEXT NOT NULL,
                summary         TEXT NOT NULL,
                details_json    TEXT NOT NULL DEFAULT '{}',
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS run_event (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id      TEXT NOT NULL,
                unit_id      TEXT NOT NULL DEFAULT '',
                session_id   TEXT NOT NULL DEFAULT '',
                event_type   TEXT NOT NULL,
                payload_json TEXT NOT NULL DEFAULT '{}',
                created_at   TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS mission (
                id                     TEXT PRIMARY KEY,
                repo_path              TEXT NOT NULL,
                worktree_path          TEXT NOT NULL,
                graph_snapshot         TEXT NOT NULL DEFAULT '',
                title                  TEXT NOT NULL,
                brief                  TEXT NOT NULL,
                validation_command     TEXT NOT NULL,
                planner_notes          TEXT,
                planner_profile_id     TEXT,
                planner_prompt_override TEXT,
                issue_key              TEXT,
                issue_url              TEXT,
                instruction_pack_hash  TEXT NOT NULL DEFAULT '',
                state                  TEXT NOT NULL DEFAULT 'draft',
                created_at             TEXT NOT NULL,
                updated_at             TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS objective_meta (
                unit_id                   TEXT NOT NULL,
                task_id                   TEXT NOT NULL,
                scope_paths_json          TEXT NOT NULL DEFAULT '[]',
                expected_validation_json  TEXT NOT NULL DEFAULT '[]',
                done_criteria_json        TEXT NOT NULL DEFAULT '[]',
                test_intent               TEXT,
                PRIMARY KEY (unit_id, task_id)
            );
            CREATE TABLE IF NOT EXISTS agent_session (
                id               TEXT PRIMARY KEY,
                mission_id       TEXT NOT NULL,
                objective_id     TEXT,
                profile_id       TEXT NOT NULL,
                assignment_kind  TEXT NOT NULL,
                state            TEXT NOT NULL,
                touched_paths_json TEXT NOT NULL DEFAULT '[]',
                workspace_path   TEXT NOT NULL DEFAULT '',
                lease_expires_at TEXT,
                last_heartbeat_at TEXT,
                last_event_at    TEXT,
                last_error       TEXT,
                created_at       TEXT NOT NULL,
                updated_at       TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS agent_profile (
                id               TEXT PRIMARY KEY,
                label            TEXT NOT NULL,
                provider         TEXT NOT NULL,
                model            TEXT,
                command          TEXT NOT NULL DEFAULT '',
                args_json        TEXT NOT NULL DEFAULT '[]',
                env_json         TEXT NOT NULL DEFAULT '{}',
                capabilities_json TEXT NOT NULL DEFAULT '[]',
                transport_kind   TEXT NOT NULL DEFAULT 'internal',
                active           INTEGER NOT NULL DEFAULT 0,
                is_reviewer      INTEGER NOT NULL DEFAULT 0,
                created_at       TEXT NOT NULL,
                updated_at       TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS patch_decision (
                mission_id          TEXT NOT NULL,
                objective_id        TEXT NOT NULL,
                session_id          TEXT,
                status              TEXT NOT NULL,
                reason              TEXT,
                patch_base_revision TEXT,
                apply_target_path   TEXT,
                apply_branch        TEXT,
                applied_at          TEXT,
                apply_error         TEXT,
                created_at          TEXT NOT NULL,
                updated_at          TEXT NOT NULL,
                PRIMARY KEY (mission_id, objective_id)
            );
            CREATE TABLE IF NOT EXISTS patch_gate (
                mission_id       TEXT NOT NULL,
                objective_id     TEXT NOT NULL,
                session_id       TEXT,
                status           TEXT NOT NULL,
                reasons_json     TEXT NOT NULL DEFAULT '[]',
                watchdog_status  TEXT,
                created_at       TEXT NOT NULL,
                updated_at       TEXT NOT NULL,
                PRIMARY KEY (mission_id, objective_id)
            );
            CREATE TABLE IF NOT EXISTS coordinator_thread (
                id                 TEXT PRIMARY KEY,
                repo_path          TEXT NOT NULL,
                mission_id         TEXT,
                persona_name       TEXT NOT NULL,
                title              TEXT NOT NULL,
                state              TEXT NOT NULL,
                draft_mission_json TEXT NOT NULL DEFAULT '{}',
                pending_plan_json  TEXT NOT NULL DEFAULT '[]',
                created_at         TEXT NOT NULL,
                updated_at         TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS coordinator_message (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id    TEXT NOT NULL,
                mission_id   TEXT,
                objective_id TEXT,
                session_id   TEXT,
                role         TEXT NOT NULL,
                kind         TEXT NOT NULL,
                content      TEXT NOT NULL,
                payload_json TEXT NOT NULL DEFAULT '{}',
                created_at   TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS run_event_task ON run_event (task_id, unit_id, id);
            CREATE INDEX IF NOT EXISTS run_event_session ON run_event (task_id, session_id, id);
            CREATE INDEX IF NOT EXISTS mission_updated_at ON mission (updated_at DESC);
            CREATE INDEX IF NOT EXISTS agent_session_mission ON agent_session (mission_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS verification_mission ON verification (mission_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS patch_decision_mission ON patch_decision (mission_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS patch_gate_mission ON patch_gate (mission_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS coordinator_thread_repo ON coordinator_thread (repo_path, updated_at DESC);
            CREATE INDEX IF NOT EXISTS coordinator_thread_mission ON coordinator_thread (mission_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS coordinator_message_thread ON coordinator_message (thread_id, id ASC);
            ",
        )
        .map_err(|e| format!("init schema: {e}"))
        .and_then(|_| {
            self.ensure_column("mission", "planner_notes", "ALTER TABLE mission ADD COLUMN planner_notes TEXT")
        })
        .and_then(|_| {
            self.ensure_column("mission", "planner_profile_id", "ALTER TABLE mission ADD COLUMN planner_profile_id TEXT")
        })
        .and_then(|_| {
            self.ensure_column("mission", "planner_prompt_override", "ALTER TABLE mission ADD COLUMN planner_prompt_override TEXT")
        })
        .and_then(|_| {
            self.ensure_column("agent_session", "workspace_path", "ALTER TABLE agent_session ADD COLUMN workspace_path TEXT NOT NULL DEFAULT ''")
        })
        .and_then(|_| {
            self.ensure_column("agent_session", "lease_expires_at", "ALTER TABLE agent_session ADD COLUMN lease_expires_at TEXT")
        })
        .and_then(|_| {
            self.ensure_column("agent_session", "last_heartbeat_at", "ALTER TABLE agent_session ADD COLUMN last_heartbeat_at TEXT")
        })
        .and_then(|_| {
            self.ensure_column("agent_session", "last_event_at", "ALTER TABLE agent_session ADD COLUMN last_event_at TEXT")
        })
        .and_then(|_| {
            self.ensure_column("agent_session", "last_error", "ALTER TABLE agent_session ADD COLUMN last_error TEXT")
        })
        .and_then(|_| {
            self.ensure_column("agent_profile", "model", "ALTER TABLE agent_profile ADD COLUMN model TEXT")
        })
        .and_then(|_| {
            self.ensure_column("agent_profile", "command", "ALTER TABLE agent_profile ADD COLUMN command TEXT NOT NULL DEFAULT ''")
        })
        .and_then(|_| {
            self.ensure_column("agent_profile", "args_json", "ALTER TABLE agent_profile ADD COLUMN args_json TEXT NOT NULL DEFAULT '[]'")
        })
        .and_then(|_| {
            self.ensure_column("agent_profile", "env_json", "ALTER TABLE agent_profile ADD COLUMN env_json TEXT NOT NULL DEFAULT '{}'")
        })
        .and_then(|_| {
            self.ensure_column("agent_profile", "capabilities_json", "ALTER TABLE agent_profile ADD COLUMN capabilities_json TEXT NOT NULL DEFAULT '[]'")
        })
        .and_then(|_| {
            self.ensure_column("agent_profile", "transport_kind", "ALTER TABLE agent_profile ADD COLUMN transport_kind TEXT NOT NULL DEFAULT 'internal'")
        })
        .and_then(|_| {
            self.ensure_column("agent_profile", "active", "ALTER TABLE agent_profile ADD COLUMN active INTEGER NOT NULL DEFAULT 0")
        })
        .and_then(|_| {
            self.ensure_column("agent_profile", "is_reviewer", "ALTER TABLE agent_profile ADD COLUMN is_reviewer INTEGER NOT NULL DEFAULT 0")
        })
        .and_then(|_| {
            self.ensure_column("agent_profile", "created_at", "ALTER TABLE agent_profile ADD COLUMN created_at TEXT NOT NULL DEFAULT ''")
        })
        .and_then(|_| {
            self.ensure_column("agent_profile", "updated_at", "ALTER TABLE agent_profile ADD COLUMN updated_at TEXT NOT NULL DEFAULT ''")
        })
        .and_then(|_| {
            self.ensure_column("plan", "workflow_id", "ALTER TABLE plan ADD COLUMN workflow_id TEXT NOT NULL DEFAULT ''")
        })
        .and_then(|_| {
            self.ensure_column("plan", "max_retries", "ALTER TABLE plan ADD COLUMN max_retries INTEGER NOT NULL DEFAULT 2")
        })
        .and_then(|_| {
            self.ensure_column("plan", "timeout_seconds", "ALTER TABLE plan ADD COLUMN timeout_seconds INTEGER NOT NULL DEFAULT 900")
        })
        .and_then(|_| {
            self.ensure_column("plan", "issue_id", "ALTER TABLE plan ADD COLUMN issue_id TEXT")
        })
        .and_then(|_| {
            self.ensure_column("plan", "issue_title", "ALTER TABLE plan ADD COLUMN issue_title TEXT")
        })
        .and_then(|_| {
            self.ensure_column("work_unit", "started_at", "ALTER TABLE work_unit ADD COLUMN started_at TEXT")
        })
        .and_then(|_| {
            self.ensure_column("work_unit", "retry_at", "ALTER TABLE work_unit ADD COLUMN retry_at TEXT")
        })
        .and_then(|_| {
            self.ensure_column("work_unit", "reconciled_at", "ALTER TABLE work_unit ADD COLUMN reconciled_at TEXT")
        })
        .and_then(|_| {
            self.ensure_column("work_unit", "run_attempt", "ALTER TABLE work_unit ADD COLUMN run_attempt INTEGER NOT NULL DEFAULT 0")
        })
        .and_then(|_| {
            self.ensure_column("work_unit", "retry_count", "ALTER TABLE work_unit ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0")
        })
        .and_then(|_| {
            self.ensure_column("work_unit", "max_retries", "ALTER TABLE work_unit ADD COLUMN max_retries INTEGER NOT NULL DEFAULT 0")
        })
        .and_then(|_| {
            self.ensure_column("work_unit", "last_error", "ALTER TABLE work_unit ADD COLUMN last_error TEXT")
        })
        .and_then(|_| {
            self.ensure_column("work_unit", "reconciled_outcome", "ALTER TABLE work_unit ADD COLUMN reconciled_outcome TEXT")
        })
        .and_then(|_| {
            self.ensure_column("work_unit", "reconciled_note", "ALTER TABLE work_unit ADD COLUMN reconciled_note TEXT")
        })
        .and_then(|_| {
            self.conn
                .execute(
                    "CREATE INDEX IF NOT EXISTS agent_profile_active ON agent_profile (active, updated_at DESC)",
                    [],
                )
                .map_err(|e| format!("create agent_profile_active index: {e}"))?;
            Ok(())
        })
    }

    fn ensure_column(&self, table: &str, column: &str, alter_sql: &str) -> Result<(), String> {
        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(|e| format!("prepare table info for {table}.{column}: {e}"))?;
        let has_column = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("inspect columns for {table}.{column}: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect columns for {table}.{column}: {e}"))?
            .into_iter()
            .any(|name| name == column);
        if !has_column {
            self.conn
                .execute(alter_sql, [])
                .map_err(|e| format!("migrate {table}.{column}: {e}"))?;
        }
        Ok(())
    }

    pub fn upsert_mission(&mut self, mission: &MissionRecord) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO mission (
                    id, repo_path, worktree_path, graph_snapshot, title, brief, validation_command,
                    planner_notes, planner_profile_id, planner_prompt_override,
                    issue_key, issue_url, instruction_pack_hash, state, created_at, updated_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
                ON CONFLICT(id) DO UPDATE SET
                    repo_path=excluded.repo_path,
                    worktree_path=excluded.worktree_path,
                    graph_snapshot=excluded.graph_snapshot,
                    title=excluded.title,
                    brief=excluded.brief,
                    validation_command=excluded.validation_command,
                    planner_notes=excluded.planner_notes,
                    planner_profile_id=excluded.planner_profile_id,
                    planner_prompt_override=excluded.planner_prompt_override,
                    issue_key=excluded.issue_key,
                    issue_url=excluded.issue_url,
                    instruction_pack_hash=excluded.instruction_pack_hash,
                    state=excluded.state,
                    updated_at=excluded.updated_at",
                params![
                    mission.id,
                    mission.repo_path,
                    mission.worktree_path,
                    mission.graph_snapshot,
                    mission.title,
                    mission.brief,
                    mission.validation_command,
                    mission.planner_notes,
                    mission.planner_profile_id,
                    mission.planner_prompt_override,
                    mission.issue_key,
                    mission.issue_url,
                    mission.instruction_pack_hash,
                    mission.state,
                    mission.created_at,
                    mission.updated_at,
                ],
            )
            .map_err(|e| format!("upsert mission: {e}"))?;
        Ok(())
    }

    pub fn get_mission(&self, mission_id: &str) -> Result<Option<MissionRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, repo_path, worktree_path, graph_snapshot, title, brief, validation_command,
                        planner_notes, planner_profile_id, planner_prompt_override,
                        issue_key, issue_url, instruction_pack_hash, state, created_at, updated_at
                 FROM mission WHERE id=?1 LIMIT 1",
            )
            .map_err(|e| format!("prepare get mission: {e}"))?;

        let mission = stmt
            .query_row(params![mission_id], |row| {
                Ok(MissionRecord {
                    id: row.get(0)?,
                    repo_path: row.get(1)?,
                    worktree_path: row.get(2)?,
                    graph_snapshot: row.get(3)?,
                    title: row.get(4)?,
                    brief: row.get(5)?,
                    validation_command: row.get(6)?,
                    planner_notes: row.get(7)?,
                    planner_profile_id: row.get(8)?,
                    planner_prompt_override: row.get(9)?,
                    issue_key: row.get(10)?,
                    issue_url: row.get(11)?,
                    instruction_pack_hash: row.get(12)?,
                    state: row.get(13)?,
                    created_at: row.get(14)?,
                    updated_at: row.get(15)?,
                })
            })
            .optional()
            .map_err(|e| format!("get mission: {e}"))?;
        Ok(mission)
    }

    pub fn get_latest_mission(&self) -> Result<Option<MissionRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, repo_path, worktree_path, graph_snapshot, title, brief, validation_command,
                        planner_notes, planner_profile_id, planner_prompt_override,
                        issue_key, issue_url, instruction_pack_hash, state, created_at, updated_at
                 FROM mission ORDER BY updated_at DESC, id DESC LIMIT 1",
            )
            .map_err(|e| format!("prepare latest mission: {e}"))?;

        let mission = stmt
            .query_row([], |row| {
                Ok(MissionRecord {
                    id: row.get(0)?,
                    repo_path: row.get(1)?,
                    worktree_path: row.get(2)?,
                    graph_snapshot: row.get(3)?,
                    title: row.get(4)?,
                    brief: row.get(5)?,
                    validation_command: row.get(6)?,
                    planner_notes: row.get(7)?,
                    planner_profile_id: row.get(8)?,
                    planner_prompt_override: row.get(9)?,
                    issue_key: row.get(10)?,
                    issue_url: row.get(11)?,
                    instruction_pack_hash: row.get(12)?,
                    state: row.get(13)?,
                    created_at: row.get(14)?,
                    updated_at: row.get(15)?,
                })
            })
            .optional()
            .map_err(|e| format!("latest mission: {e}"))?;
        Ok(mission)
    }

    pub fn plan_mission(
        &mut self,
        mission: &MissionRecord,
        prompt: &str,
        objectives: &[NewObjective],
    ) -> Result<(), String> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("begin plan mission tx: {e}"))?;
        let units: Vec<NewWorkUnit> = objectives
            .iter()
            .map(|objective| NewWorkUnit {
                id: objective.id.clone(),
                title: objective.title.clone(),
                prompt: objective.prompt.clone(),
            })
            .collect();
        Self::plan_task_tx(
            &tx,
            &mission.id,
            &mission.title,
            prompt,
            &units,
            &mission.updated_at,
        )?;
        tx.execute(
            "DELETE FROM objective_meta WHERE task_id=?1",
            params![mission.id],
        )
        .map_err(|e| format!("clear objective meta: {e}"))?;
        for objective in objectives {
            let scope_paths_json =
                serde_json::to_string(&objective.scope_paths).unwrap_or_else(|_| "[]".to_string());
            let expected_validation_json = serde_json::to_string(&objective.expected_validation)
                .unwrap_or_else(|_| "[]".to_string());
            let done_criteria_json = serde_json::to_string(&objective.done_criteria)
                .unwrap_or_else(|_| "[]".to_string());
            tx.execute(
                "INSERT INTO objective_meta (
                    unit_id, task_id, scope_paths_json, expected_validation_json, done_criteria_json, test_intent
                 ) VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    objective.id,
                    mission.id,
                    scope_paths_json,
                    expected_validation_json,
                    done_criteria_json,
                    objective.test_intent,
                ],
            )
            .map_err(|e| format!("insert objective meta '{}': {e}", objective.id))?;
        }
        tx.execute(
            "UPDATE mission SET state='planned', updated_at=?2 WHERE id=?1",
            params![mission.id, mission.updated_at],
        )
        .map_err(|e| format!("mark mission planned: {e}"))?;
        tx.commit().map_err(|e| format!("commit plan mission: {e}"))
    }

    /// Create a plan with its initial set of work units.
    /// Idempotent: if task_id already exists, it is overwritten.
    pub fn plan_task(
        &mut self,
        task_id: &str,
        title: &str,
        prompt: &str,
        units: &[NewWorkUnit],
    ) -> Result<(), String> {
        let now = now_utc();
        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("begin tx: {e}"))?;
        Self::plan_task_tx(&tx, task_id, title, prompt, units, &now)?;
        tx.commit().map_err(|e| format!("commit plan_task: {e}"))
    }

    fn plan_task_tx(
        tx: &Transaction<'_>,
        task_id: &str,
        title: &str,
        prompt: &str,
        units: &[NewWorkUnit],
        now: &str,
    ) -> Result<(), String> {
        tx.execute(
            "INSERT OR REPLACE INTO plan (task_id, title, prompt, created_at) VALUES (?1,?2,?3,?4)",
            params![task_id, title, prompt, now],
        )
        .map_err(|e| format!("insert plan: {e}"))?;

        // Delete old units for this task before inserting new ones
        tx.execute("DELETE FROM work_unit WHERE task_id = ?1", params![task_id])
            .map_err(|e| format!("clear old units: {e}"))?;

        for unit in units {
            tx.execute(
                "INSERT INTO work_unit (id, task_id, title, prompt, status, created_at)
                 VALUES (?1,?2,?3,?4,'pending',?5)",
                params![unit.id, task_id, unit.title, unit.prompt, now],
            )
            .map_err(|e| format!("insert work_unit '{}': {e}", unit.id))?;
        }
        Ok(())
    }

    /// Claim the oldest pending work unit for a task.
    /// Returns None if all units are already claimed or done.
    pub fn claim_work_unit(
        &mut self,
        task_id: &str,
        agent_id: &str,
    ) -> Result<Option<WorkUnit>, String> {
        let now = now_utc();
        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("begin tx: {e}"))?;
        let result = Self::claim_work_unit_tx(&tx, task_id, agent_id, &now)?;
        tx.commit().map_err(|e| format!("commit claim: {e}"))?;
        Ok(result)
    }

    pub fn claim_specific_work_unit(
        &mut self,
        task_id: &str,
        unit_id: &str,
        agent_id: &str,
    ) -> Result<Option<WorkUnit>, String> {
        let now = now_utc();
        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("begin tx: {e}"))?;
        let result = Self::claim_specific_work_unit_tx(&tx, task_id, unit_id, agent_id, &now)?;
        tx.commit()
            .map_err(|e| format!("commit specific claim: {e}"))?;
        Ok(result)
    }

    fn claim_work_unit_tx(
        tx: &Transaction<'_>,
        task_id: &str,
        agent_id: &str,
        now: &str,
    ) -> Result<Option<WorkUnit>, String> {
        // Find oldest pending unit
        let unit_id: Option<String> = tx
            .query_row(
                "SELECT id FROM work_unit WHERE task_id=?1 AND status='pending'
                 ORDER BY created_at ASC, id ASC LIMIT 1",
                params![task_id],
                |row| row.get(0),
            )
            .ok();

        let Some(unit_id) = unit_id else {
            return Ok(None);
        };

        tx.execute(
            "UPDATE work_unit SET status='claimed', claimed_by=?1, claimed_at=?2
             WHERE id=?3 AND task_id=?4",
            params![agent_id, now, unit_id, task_id],
        )
        .map_err(|e| format!("claim unit: {e}"))?;

        let unit = Self::load_unit_tx(tx, task_id, &unit_id)?;
        Ok(Some(unit))
    }

    fn claim_specific_work_unit_tx(
        tx: &Transaction<'_>,
        task_id: &str,
        unit_id: &str,
        agent_id: &str,
        now: &str,
    ) -> Result<Option<WorkUnit>, String> {
        let status: Option<String> = tx
            .query_row(
                "SELECT status FROM work_unit WHERE id=?1 AND task_id=?2",
                params![unit_id, task_id],
                |row| row.get(0),
            )
            .ok();

        let Some(status) = status else {
            return Ok(None);
        };
        if status != "pending" {
            return Ok(None);
        }

        tx.execute(
            "UPDATE work_unit SET status='claimed', claimed_by=?1, claimed_at=?2
             WHERE id=?3 AND task_id=?4",
            params![agent_id, now, unit_id, task_id],
        )
        .map_err(|e| format!("claim specific unit: {e}"))?;

        let unit = Self::load_unit_tx(tx, task_id, unit_id)?;
        Ok(Some(unit))
    }

    pub fn requeue_work_unit(&mut self, task_id: &str, unit_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE work_unit
                 SET status='pending', claimed_by=NULL, claimed_at=NULL
                 WHERE id=?1 AND task_id=?2 AND status='claimed'",
                params![unit_id, task_id],
            )
            .map_err(|e| format!("requeue work unit: {e}"))?;
        Ok(())
    }

    /// Record a verdict for a work unit and advance its status.
    pub fn submit_verdict(&mut self, verdict: UnitVerdict) -> Result<(), String> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("begin tx: {e}"))?;
        Self::submit_verdict_tx(&tx, &verdict)?;
        tx.commit().map_err(|e| format!("commit verdict: {e}"))
    }

    fn submit_verdict_tx(tx: &Transaction<'_>, verdict: &UnitVerdict) -> Result<(), String> {
        let artifacts_json =
            serde_json::to_string(&verdict.artifacts).unwrap_or_else(|_| "[]".to_string());

        tx.execute(
            "INSERT OR REPLACE INTO unit_verdict
             (unit_id, task_id, status, summary, submitted_at, artifacts_json)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                verdict.unit_id,
                verdict.task_id,
                verdict.status,
                verdict.summary,
                verdict.submitted_at,
                artifacts_json,
            ],
        )
        .map_err(|e| format!("insert verdict: {e}"))?;

        let new_status = if verdict.status == "done" {
            "done"
        } else {
            "failed"
        };
        tx.execute(
            "UPDATE work_unit SET status=?1 WHERE id=?2 AND task_id=?3",
            params![new_status, verdict.unit_id, verdict.task_id],
        )
        .map_err(|e| format!("update unit status: {e}"))?;

        Ok(())
    }

    pub fn list_plans(&self) -> Result<Vec<PlanRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT task_id, workflow_id, title, prompt, max_retries, timeout_seconds,
                        issue_id, issue_title, created_at
                 FROM plan ORDER BY created_at DESC",
            )
            .map_err(|e| format!("prepare list plans: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PlanRecord {
                    task_id: row.get(0)?,
                    workflow_id: row.get(1)?,
                    title: row.get(2)?,
                    prompt: row.get(3)?,
                    max_retries: row.get(4)?,
                    timeout_seconds: row.get(5)?,
                    issue_id: row.get(6)?,
                    issue_title: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })
            .map_err(|e| format!("query list plans: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect plans: {e}"))
    }

    pub fn list_all_work_units(&self) -> Result<Vec<WorkUnit>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, task_id, title, prompt, status, claimed_by, claimed_at,
                        started_at, retry_at, reconciled_at, run_attempt, retry_count,
                        max_retries, last_error, reconciled_outcome, reconciled_note, created_at
                 FROM work_unit ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| format!("prepare list all units: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(WorkUnit {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    title: row.get(2)?,
                    prompt: row.get(3)?,
                    status: WorkUnitStatus::from_str(&row.get::<_, String>(4)?),
                    claimed_by: row.get(5)?,
                    claimed_at: row.get(6)?,
                    started_at: row.get(7)?,
                    retry_at: row.get(8)?,
                    reconciled_at: row.get(9)?,
                    run_attempt: row.get(10)?,
                    retry_count: row.get(11)?,
                    max_retries: row.get(12)?,
                    last_error: row.get(13)?,
                    reconciled_outcome: row.get(14)?,
                    reconciled_note: row.get(15)?,
                    created_at: row.get(16)?,
                })
            })
            .map_err(|e| format!("query list all units: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect all units: {e}"))
    }

    pub fn upsert_plan_record(&mut self, plan: &PlanRecord) -> Result<(), String> {
        self.conn.execute(
            "INSERT INTO plan (task_id, workflow_id, title, prompt, max_retries, timeout_seconds, issue_id, issue_title, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(task_id) DO UPDATE SET
                workflow_id=excluded.workflow_id,
                title=excluded.title,
                prompt=excluded.prompt,
                max_retries=excluded.max_retries,
                timeout_seconds=excluded.timeout_seconds,
                issue_id=excluded.issue_id,
                issue_title=excluded.issue_title,
                created_at=excluded.created_at",
            params![
                plan.task_id,
                plan.workflow_id,
                plan.title,
                plan.prompt,
                plan.max_retries,
                plan.timeout_seconds,
                plan.issue_id,
                plan.issue_title,
                plan.created_at,
            ],
        ).map_err(|e| format!("upsert plan record: {e}"))?;
        Ok(())
    }

    pub fn upsert_work_unit(&mut self, unit: &WorkUnit) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO work_unit (
                id, task_id, title, prompt, status, claimed_by, claimed_at,
                started_at, retry_at, reconciled_at, run_attempt, retry_count,
                max_retries, last_error, reconciled_outcome, reconciled_note, created_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
            ON CONFLICT(id, task_id) DO UPDATE SET
                title=excluded.title,
                prompt=excluded.prompt,
                status=excluded.status,
                claimed_by=excluded.claimed_by,
                claimed_at=excluded.claimed_at,
                started_at=excluded.started_at,
                retry_at=excluded.retry_at,
                reconciled_at=excluded.reconciled_at,
                run_attempt=excluded.run_attempt,
                retry_count=excluded.retry_count,
                max_retries=excluded.max_retries,
                last_error=excluded.last_error,
                reconciled_outcome=excluded.reconciled_outcome,
                reconciled_note=excluded.reconciled_note,
                created_at=excluded.created_at",
                params![
                    unit.id,
                    unit.task_id,
                    unit.title,
                    unit.prompt,
                    unit.status.as_str(),
                    unit.claimed_by,
                    unit.claimed_at,
                    unit.started_at,
                    unit.retry_at,
                    unit.reconciled_at,
                    unit.run_attempt,
                    unit.retry_count,
                    unit.max_retries,
                    unit.last_error,
                    unit.reconciled_outcome,
                    unit.reconciled_note,
                    unit.created_at,
                ],
            )
            .map_err(|e| format!("upsert work unit: {e}"))?;
        Ok(())
    }

    pub fn delete_plan(&mut self, task_id: &str) -> Result<(), String> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("begin delete plan tx: {e}"))?;
        tx.execute("DELETE FROM plan WHERE task_id=?1", params![task_id])
            .map_err(|e| format!("delete plan: {e}"))?;
        tx.execute("DELETE FROM work_unit WHERE task_id=?1", params![task_id])
            .map_err(|e| format!("delete units: {e}"))?;
        tx.commit().map_err(|e| format!("commit delete plan: {e}"))
    }

    /// Return all work units for a task, in creation order.
    pub fn list_work_units(&self, task_id: &str) -> Result<Vec<WorkUnit>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, task_id, title, prompt, status, claimed_by, claimed_at,
                        started_at, retry_at, reconciled_at, run_attempt, retry_count,
                        max_retries, last_error, reconciled_outcome, reconciled_note, created_at
             FROM work_unit WHERE task_id=?1 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| format!("prepare list: {e}"))?;

        let units = stmt
            .query_map(params![task_id], |row| {
                Ok(WorkUnit {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    title: row.get(2)?,
                    prompt: row.get(3)?,
                    status: WorkUnitStatus::from_str(&row.get::<_, String>(4)?),
                    claimed_by: row.get(5)?,
                    claimed_at: row.get(6)?,
                    started_at: row.get(7)?,
                    retry_at: row.get(8)?,
                    reconciled_at: row.get(9)?,
                    run_attempt: row.get(10)?,
                    retry_count: row.get(11)?,
                    max_retries: row.get(12)?,
                    last_error: row.get(13)?,
                    reconciled_outcome: row.get(14)?,
                    reconciled_note: row.get(15)?,
                    created_at: row.get(16)?,
                })
            })
            .map_err(|e| format!("list units query: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("list units collect: {e}"))?;

        Ok(units)
    }

    pub fn list_objectives(&self, mission_id: &str) -> Result<Vec<ObjectiveRecord>, String> {
        let mut stmt = self.conn.prepare(
            "SELECT wu.id, wu.task_id, wu.title, wu.prompt, wu.status, wu.claimed_by, wu.claimed_at, wu.created_at,
                    COALESCE(om.scope_paths_json, '[]'),
                    COALESCE(om.expected_validation_json, '[]'),
                    COALESCE(om.done_criteria_json, '[]'),
                    om.test_intent
             FROM work_unit wu
             LEFT JOIN objective_meta om ON om.unit_id = wu.id AND om.task_id = wu.task_id
             WHERE wu.task_id=?1
             ORDER BY wu.created_at ASC, wu.id ASC",
        )
        .map_err(|e| format!("prepare list objectives: {e}"))?;

        let objectives = stmt
            .query_map(params![mission_id], |row| {
                let scope_paths_json: String = row.get(8)?;
                let expected_validation_json: String = row.get(9)?;
                let done_criteria_json: String = row.get(10)?;
                Ok(ObjectiveRecord {
                    id: row.get(0)?,
                    mission_id: row.get(1)?,
                    title: row.get(2)?,
                    prompt: row.get(3)?,
                    status: WorkUnitStatus::from_str(&row.get::<_, String>(4)?),
                    claimed_by: row.get(5)?,
                    claimed_at: row.get(6)?,
                    created_at: row.get(7)?,
                    meta: ObjectiveMeta {
                        scope_paths: serde_json::from_str(&scope_paths_json).unwrap_or_default(),
                        expected_validation: serde_json::from_str(&expected_validation_json)
                            .unwrap_or_default(),
                        done_criteria: serde_json::from_str(&done_criteria_json)
                            .unwrap_or_default(),
                        test_intent: row.get(11)?,
                    },
                })
            })
            .map_err(|e| format!("query list objectives: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect objectives: {e}"))?;

        Ok(objectives)
    }

    pub fn upsert_agent_session(&mut self, session: &AgentSessionRecord) -> Result<(), String> {
        let runtime = AgentSessionRuntimeRecord {
            id: session.id.clone(),
            mission_id: session.mission_id.clone(),
            objective_id: session.objective_id.clone(),
            profile_id: session.profile_id.clone(),
            assignment_kind: session.assignment_kind.clone(),
            state: session.state.clone(),
            touched_paths: session.touched_paths.clone(),
            workspace_path: String::new(),
            lease_expires_at: None,
            last_heartbeat_at: None,
            last_event_at: None,
            last_error: None,
            created_at: session.created_at.clone(),
            updated_at: session.updated_at.clone(),
        };
        self.upsert_agent_session_runtime(&runtime)
    }

    pub fn upsert_agent_session_runtime(
        &mut self,
        session: &AgentSessionRuntimeRecord,
    ) -> Result<(), String> {
        let touched_paths_json =
            serde_json::to_string(&session.touched_paths).unwrap_or_else(|_| "[]".to_string());
        self.conn
            .execute(
                "INSERT INTO agent_session (
                    id, mission_id, objective_id, profile_id, assignment_kind, state,
                    touched_paths_json, workspace_path, lease_expires_at, last_heartbeat_at,
                    last_event_at, last_error, created_at, updated_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
                ON CONFLICT(id) DO UPDATE SET
                    mission_id=excluded.mission_id,
                    objective_id=excluded.objective_id,
                    profile_id=excluded.profile_id,
                    assignment_kind=excluded.assignment_kind,
                    state=excluded.state,
                    touched_paths_json=excluded.touched_paths_json,
                    workspace_path=excluded.workspace_path,
                    lease_expires_at=excluded.lease_expires_at,
                    last_heartbeat_at=excluded.last_heartbeat_at,
                    last_event_at=excluded.last_event_at,
                    last_error=excluded.last_error,
                    updated_at=excluded.updated_at",
                params![
                    session.id,
                    session.mission_id,
                    session.objective_id,
                    session.profile_id,
                    session.assignment_kind,
                    session.state,
                    touched_paths_json,
                    session.workspace_path,
                    session.lease_expires_at,
                    session.last_heartbeat_at,
                    session.last_event_at,
                    session.last_error,
                    session.created_at,
                    session.updated_at,
                ],
            )
            .map_err(|e| format!("upsert agent session: {e}"))?;
        Ok(())
    }

    pub fn list_agent_sessions(
        &self,
        mission_id: &str,
    ) -> Result<Vec<AgentSessionRuntimeRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, mission_id, objective_id, profile_id, assignment_kind, state,
                        touched_paths_json, workspace_path, lease_expires_at, last_heartbeat_at,
                        last_event_at, last_error, created_at, updated_at
                 FROM agent_session
                 WHERE mission_id=?1
                 ORDER BY updated_at DESC, id DESC",
            )
            .map_err(|e| format!("prepare list sessions: {e}"))?;

        let sessions = stmt
            .query_map(params![mission_id], |row| {
                let touched_paths_json: String = row.get(6)?;
                Ok(AgentSessionRuntimeRecord {
                    id: row.get(0)?,
                    mission_id: row.get(1)?,
                    objective_id: row.get(2)?,
                    profile_id: row.get(3)?,
                    assignment_kind: row.get(4)?,
                    state: row.get(5)?,
                    touched_paths: serde_json::from_str(&touched_paths_json).unwrap_or_default(),
                    workspace_path: row.get(7)?,
                    lease_expires_at: row.get(8)?,
                    last_heartbeat_at: row.get(9)?,
                    last_event_at: row.get(10)?,
                    last_error: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            })
            .map_err(|e| format!("query list sessions: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect sessions: {e}"))?;

        Ok(sessions)
    }

    pub fn get_agent_session_runtime(
        &self,
        session_id: &str,
    ) -> Result<Option<AgentSessionRuntimeRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, mission_id, objective_id, profile_id, assignment_kind, state,
                        touched_paths_json, workspace_path, lease_expires_at, last_heartbeat_at,
                        last_event_at, last_error, created_at, updated_at
                 FROM agent_session
                 WHERE id=?1
                 LIMIT 1",
            )
            .map_err(|e| format!("prepare get session runtime: {e}"))?;
        stmt.query_row(params![session_id], |row| {
            let touched_paths_json: String = row.get(6)?;
            Ok(AgentSessionRuntimeRecord {
                id: row.get(0)?,
                mission_id: row.get(1)?,
                objective_id: row.get(2)?,
                profile_id: row.get(3)?,
                assignment_kind: row.get(4)?,
                state: row.get(5)?,
                touched_paths: serde_json::from_str(&touched_paths_json).unwrap_or_default(),
                workspace_path: row.get(7)?,
                lease_expires_at: row.get(8)?,
                last_heartbeat_at: row.get(9)?,
                last_event_at: row.get(10)?,
                last_error: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
            })
        })
        .optional()
        .map_err(|e| format!("get session runtime: {e}"))
    }

    pub fn merge_agent_session_touched_paths(
        &mut self,
        session_id: &str,
        touched_paths: &[String],
        updated_at: &str,
    ) -> Result<(), String> {
        let Some(mut session) = self.get_agent_session_runtime(session_id)? else {
            return Err(format!("agent session not found: {session_id}"));
        };
        for path in touched_paths {
            if !session
                .touched_paths
                .iter()
                .any(|existing| existing == path)
            {
                session.touched_paths.push(path.clone());
            }
        }
        session.updated_at = updated_at.to_string();
        session.last_event_at = Some(updated_at.to_string());
        self.upsert_agent_session_runtime(&session)
    }

    pub fn update_agent_session_status(
        &mut self,
        session_id: &str,
        state: &str,
        updated_at: &str,
        last_error: Option<String>,
    ) -> Result<(), String> {
        let Some(mut session) = self.get_agent_session_runtime(session_id)? else {
            return Err(format!("agent session not found: {session_id}"));
        };
        session.state = state.to_string();
        session.updated_at = updated_at.to_string();
        session.last_event_at = Some(updated_at.to_string());
        session.last_heartbeat_at = Some(updated_at.to_string());
        session.last_error = last_error;
        if matches!(state, "completed" | "failed" | "logged_out") {
            session.lease_expires_at = None;
        } else {
            let now_unix_seconds =
                parse_unix_label_seconds(Some(updated_at)).unwrap_or_else(current_unix_seconds);
            session.lease_expires_at = Some(session_lease_label_from_now(now_unix_seconds));
        }
        self.upsert_agent_session_runtime(&session)
    }

    pub fn upsert_agent_profile(&mut self, profile: &AgentProfile) -> Result<(), String> {
        let args_json = serde_json::to_string(&profile.args).unwrap_or_else(|_| "[]".to_string());
        let env_json = serde_json::to_string(&profile.env).unwrap_or_else(|_| "{}".to_string());
        let capabilities_json =
            serde_json::to_string(&profile.capabilities).unwrap_or_else(|_| "[]".to_string());
        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("begin upsert agent profile tx: {e}"))?;
        if profile.active {
            tx.execute("UPDATE agent_profile SET active=0", [])
                .map_err(|e| format!("clear active profiles: {e}"))?;
        }
        tx.execute(
            "INSERT INTO agent_profile (
                id, label, provider, model, command, args_json, env_json, capabilities_json,
                transport_kind, active, is_reviewer, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(id) DO UPDATE SET
                label=excluded.label,
                provider=excluded.provider,
                model=excluded.model,
                command=excluded.command,
                args_json=excluded.args_json,
                env_json=excluded.env_json,
                capabilities_json=excluded.capabilities_json,
                transport_kind=excluded.transport_kind,
                active=excluded.active,
                is_reviewer=excluded.is_reviewer,
                updated_at=excluded.updated_at",
            params![
                profile.id,
                profile.label,
                profile.provider,
                profile.model,
                profile.command,
                args_json,
                env_json,
                capabilities_json,
                profile.transport_kind,
                if profile.active { 1 } else { 0 },
                if profile.is_reviewer { 1 } else { 0 },
                profile.created_at,
                profile.updated_at,
            ],
        )
        .map_err(|e| format!("upsert agent profile: {e}"))?;
        tx.commit()
            .map_err(|e| format!("commit agent profile: {e}"))
    }

    pub fn list_agent_profiles(&self) -> Result<Vec<AgentProfile>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, label, provider, model, command, args_json, env_json, capabilities_json,
                        transport_kind, active, is_reviewer, created_at, updated_at
                 FROM agent_profile
                 ORDER BY active DESC, updated_at DESC, id ASC",
            )
            .map_err(|e| format!("prepare list agent profiles: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                let args_json: String = row.get(5)?;
                let env_json: String = row.get(6)?;
                let capabilities_json: String = row.get(7)?;
                Ok(AgentProfile {
                    id: row.get(0)?,
                    label: row.get(1)?,
                    provider: row.get(2)?,
                    model: row.get(3)?,
                    command: row.get(4)?,
                    args: serde_json::from_str(&args_json).unwrap_or_default(),
                    env: serde_json::from_str(&env_json).unwrap_or_else(|_| serde_json::json!({})),
                    capabilities: serde_json::from_str(&capabilities_json).unwrap_or_default(),
                    transport_kind: row.get(8)?,
                    active: row.get::<_, i64>(9)? != 0,
                    is_reviewer: row.get::<_, i64>(10)? != 0,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                })
            })
            .map_err(|e| format!("query list agent profiles: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect agent profiles: {e}"))
    }

    pub fn get_agent_profile(&self, profile_id: &str) -> Result<Option<AgentProfile>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, label, provider, model, command, args_json, env_json, capabilities_json,
                        transport_kind, active, is_reviewer, created_at, updated_at
                 FROM agent_profile WHERE id=?1 LIMIT 1",
            )
            .map_err(|e| format!("prepare get agent profile: {e}"))?;
        stmt.query_row(params![profile_id], |row| {
            let args_json: String = row.get(5)?;
            let env_json: String = row.get(6)?;
            let capabilities_json: String = row.get(7)?;
            Ok(AgentProfile {
                id: row.get(0)?,
                label: row.get(1)?,
                provider: row.get(2)?,
                model: row.get(3)?,
                command: row.get(4)?,
                args: serde_json::from_str(&args_json).unwrap_or_default(),
                env: serde_json::from_str(&env_json).unwrap_or_else(|_| serde_json::json!({})),
                capabilities: serde_json::from_str(&capabilities_json).unwrap_or_default(),
                transport_kind: row.get(8)?,
                active: row.get::<_, i64>(9)? != 0,
                is_reviewer: row.get::<_, i64>(10)? != 0,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })
        .optional()
        .map_err(|e| format!("get agent profile: {e}"))
    }

    pub fn get_active_agent_profile(&self) -> Result<Option<AgentProfile>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, label, provider, model, command, args_json, env_json, capabilities_json,
                        transport_kind, active, is_reviewer, created_at, updated_at
                 FROM agent_profile WHERE active=1
                 ORDER BY updated_at DESC, id ASC
                 LIMIT 1",
            )
            .map_err(|e| format!("prepare active agent profile: {e}"))?;
        stmt.query_row([], |row| {
            let args_json: String = row.get(5)?;
            let env_json: String = row.get(6)?;
            let capabilities_json: String = row.get(7)?;
            Ok(AgentProfile {
                id: row.get(0)?,
                label: row.get(1)?,
                provider: row.get(2)?,
                model: row.get(3)?,
                command: row.get(4)?,
                args: serde_json::from_str(&args_json).unwrap_or_default(),
                env: serde_json::from_str(&env_json).unwrap_or_else(|_| serde_json::json!({})),
                capabilities: serde_json::from_str(&capabilities_json).unwrap_or_default(),
                transport_kind: row.get(8)?,
                active: row.get::<_, i64>(9)? != 0,
                is_reviewer: row.get::<_, i64>(10)? != 0,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })
        .optional()
        .map_err(|e| format!("active agent profile: {e}"))
    }

    pub fn delete_agent_profile(&mut self, profile_id: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM agent_profile WHERE id=?1", params![profile_id])
            .map_err(|e| format!("delete agent profile: {e}"))?;
        Ok(())
    }

    pub fn upsert_coordinator_thread(
        &mut self,
        thread: &CoordinatorThreadRecord,
    ) -> Result<(), String> {
        let draft_mission_json =
            serde_json::to_string(&thread.draft_mission).unwrap_or_else(|_| "{}".to_string());
        let pending_plan_json =
            serde_json::to_string(&thread.pending_plan).unwrap_or_else(|_| "[]".to_string());
        self.conn
            .execute(
                "INSERT INTO coordinator_thread (
                    id, repo_path, mission_id, persona_name, title, state,
                    draft_mission_json, pending_plan_json, created_at, updated_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                ON CONFLICT(id) DO UPDATE SET
                    repo_path=excluded.repo_path,
                    mission_id=excluded.mission_id,
                    persona_name=excluded.persona_name,
                    title=excluded.title,
                    state=excluded.state,
                    draft_mission_json=excluded.draft_mission_json,
                    pending_plan_json=excluded.pending_plan_json,
                    updated_at=excluded.updated_at",
                params![
                    thread.id,
                    thread.repo_path,
                    thread.mission_id,
                    thread.persona_name,
                    thread.title,
                    thread.state,
                    draft_mission_json,
                    pending_plan_json,
                    thread.created_at,
                    thread.updated_at,
                ],
            )
            .map_err(|e| format!("upsert coordinator thread: {e}"))?;
        Ok(())
    }

    pub fn get_coordinator_thread(
        &self,
        thread_id: &str,
    ) -> Result<Option<CoordinatorThreadRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, repo_path, mission_id, persona_name, title, state,
                        draft_mission_json, pending_plan_json, created_at, updated_at
                 FROM coordinator_thread
                 WHERE id=?1
                 LIMIT 1",
            )
            .map_err(|e| format!("prepare get coordinator thread: {e}"))?;
        stmt.query_row(params![thread_id], |row| {
            let draft_mission_json: String = row.get(6)?;
            let pending_plan_json: String = row.get(7)?;
            Ok(CoordinatorThreadRecord {
                id: row.get(0)?,
                repo_path: row.get(1)?,
                mission_id: row.get(2)?,
                persona_name: row.get(3)?,
                title: row.get(4)?,
                state: row.get(5)?,
                draft_mission: serde_json::from_str(&draft_mission_json).unwrap_or_default(),
                pending_plan: serde_json::from_str(&pending_plan_json).unwrap_or_default(),
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .optional()
        .map_err(|e| format!("get coordinator thread: {e}"))
    }

    pub fn get_latest_coordinator_thread(
        &self,
        repo_path: &str,
    ) -> Result<Option<CoordinatorThreadRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, repo_path, mission_id, persona_name, title, state,
                        draft_mission_json, pending_plan_json, created_at, updated_at
                 FROM coordinator_thread
                 WHERE repo_path=?1
                 ORDER BY
                    CASE state
                        WHEN 'resolved' THEN 1
                        WHEN 'abandoned' THEN 1
                        ELSE 0
                    END ASC,
                    updated_at DESC,
                    id DESC
                 LIMIT 1",
            )
            .map_err(|e| format!("prepare latest coordinator thread: {e}"))?;
        stmt.query_row(params![repo_path], |row| {
            let draft_mission_json: String = row.get(6)?;
            let pending_plan_json: String = row.get(7)?;
            Ok(CoordinatorThreadRecord {
                id: row.get(0)?,
                repo_path: row.get(1)?,
                mission_id: row.get(2)?,
                persona_name: row.get(3)?,
                title: row.get(4)?,
                state: row.get(5)?,
                draft_mission: serde_json::from_str(&draft_mission_json).unwrap_or_default(),
                pending_plan: serde_json::from_str(&pending_plan_json).unwrap_or_default(),
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .optional()
        .map_err(|e| format!("latest coordinator thread: {e}"))
    }

    pub fn list_coordinator_threads(
        &self,
        repo_path: &str,
        limit: usize,
    ) -> Result<Vec<CoordinatorThreadSummary>, String> {
        let limit = limit.max(1) as i64;
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, mission_id, title, state, updated_at
                 FROM coordinator_thread
                 WHERE repo_path = ?1
                 ORDER BY updated_at DESC
                 LIMIT ?2",
            )
            .map_err(|e| format!("prepare list coordinator threads: {e}"))?;
        let rows = stmt
            .query_map(params![repo_path, limit], |row| {
                Ok(CoordinatorThreadSummary {
                    id: row.get(0)?,
                    mission_id: row.get(1)?,
                    title: row.get(2)?,
                    state: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })
            .map_err(|e| format!("query list coordinator threads: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect coordinator threads: {e}"))
    }

    pub fn append_coordinator_message(
        &mut self,
        message: NewCoordinatorMessage<'_>,
    ) -> Result<CoordinatorMessageRecord, String> {
        let payload_json =
            serde_json::to_string(&message.payload).unwrap_or_else(|_| "{}".to_string());
        let created_at = crate::unix_now_label();
        self.conn
            .execute(
                "INSERT INTO coordinator_message (
                    thread_id, mission_id, objective_id, session_id, role, kind, content, payload_json, created_at
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    message.thread_id,
                    message.mission_id,
                    message.objective_id,
                    message.session_id,
                    message.role,
                    message.kind,
                    message.content,
                    payload_json,
                    created_at,
                ],
            )
            .map_err(|e| format!("insert coordinator message: {e}"))?;
        let id = self.conn.last_insert_rowid();
        Ok(CoordinatorMessageRecord {
            id,
            thread_id: message.thread_id.to_string(),
            mission_id: message.mission_id.map(str::to_string),
            objective_id: message.objective_id.map(str::to_string),
            session_id: message.session_id.map(str::to_string),
            role: message.role.to_string(),
            kind: message.kind.to_string(),
            content: message.content.to_string(),
            payload: message.payload,
            created_at,
        })
    }

    pub fn list_coordinator_messages(
        &self,
        thread_id: &str,
        limit: usize,
    ) -> Result<Vec<CoordinatorMessageRecord>, String> {
        let take = i64::try_from(limit.max(1)).unwrap_or(200);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, thread_id, mission_id, objective_id, session_id, role, kind, content, payload_json, created_at
                 FROM coordinator_message
                 WHERE thread_id=?1
                 ORDER BY id DESC
                 LIMIT ?2",
            )
            .map_err(|e| format!("prepare list coordinator messages: {e}"))?;
        let rows = stmt
            .query_map(params![thread_id, take], |row| {
                let payload_json: String = row.get(8)?;
                Ok(CoordinatorMessageRecord {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    mission_id: row.get(2)?,
                    objective_id: row.get(3)?,
                    session_id: row.get(4)?,
                    role: row.get(5)?,
                    kind: row.get(6)?,
                    content: row.get(7)?,
                    payload: serde_json::from_str(&payload_json)
                        .unwrap_or_else(|_| serde_json::json!({})),
                    created_at: row.get(9)?,
                })
            })
            .map_err(|e| format!("query coordinator messages: {e}"))?;
        let mut messages = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect coordinator messages: {e}"))?;
        messages.reverse();
        Ok(messages)
    }

    pub fn upsert_patch_decision(&mut self, decision: &PatchDecisionRecord) -> Result<(), String> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("begin patch decision tx: {e}"))?;
        tx.execute(
            "INSERT INTO patch_decision (
                mission_id, objective_id, session_id, status, reason, patch_base_revision,
                apply_target_path, apply_branch, applied_at, apply_error, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(mission_id, objective_id) DO UPDATE SET
                session_id=excluded.session_id,
                status=excluded.status,
                reason=excluded.reason,
                patch_base_revision=excluded.patch_base_revision,
                apply_target_path=excluded.apply_target_path,
                apply_branch=excluded.apply_branch,
                applied_at=excluded.applied_at,
                apply_error=excluded.apply_error,
                updated_at=excluded.updated_at",
            params![
                decision.mission_id,
                decision.objective_id,
                decision.session_id,
                decision.status,
                decision.reason,
                decision.patch_base_revision,
                decision.apply_target_path,
                decision.apply_branch,
                decision.applied_at,
                decision.apply_error,
                decision.created_at,
                decision.updated_at,
            ],
        )
        .map_err(|e| format!("upsert patch decision: {e}"))?;
        tx.commit()
            .map_err(|e| format!("commit patch decision: {e}"))
    }

    pub fn list_patch_decisions(
        &self,
        mission_id: &str,
    ) -> Result<Vec<PatchDecisionRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT mission_id, objective_id, session_id, status, reason, patch_base_revision,
                        apply_target_path, apply_branch, applied_at, apply_error, created_at, updated_at
                 FROM patch_decision
                 WHERE mission_id=?1
                 ORDER BY updated_at DESC, objective_id ASC",
            )
            .map_err(|e| format!("prepare list patch decisions: {e}"))?;
        let rows = stmt
            .query_map(params![mission_id], |row| {
                Ok(PatchDecisionRecord {
                    mission_id: row.get(0)?,
                    objective_id: row.get(1)?,
                    session_id: row.get(2)?,
                    status: row.get(3)?,
                    reason: row.get(4)?,
                    patch_base_revision: row.get(5)?,
                    apply_target_path: row.get(6)?,
                    apply_branch: row.get(7)?,
                    applied_at: row.get(8)?,
                    apply_error: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            })
            .map_err(|e| format!("query list patch decisions: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect patch decisions: {e}"))
    }

    pub fn get_patch_decision(
        &self,
        mission_id: &str,
        objective_id: &str,
    ) -> Result<Option<PatchDecisionRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT mission_id, objective_id, session_id, status, reason, patch_base_revision,
                        apply_target_path, apply_branch, applied_at, apply_error, created_at, updated_at
                 FROM patch_decision
                 WHERE mission_id=?1 AND objective_id=?2
                 LIMIT 1",
            )
            .map_err(|e| format!("prepare get patch decision: {e}"))?;
        stmt.query_row(params![mission_id, objective_id], |row| {
            Ok(PatchDecisionRecord {
                mission_id: row.get(0)?,
                objective_id: row.get(1)?,
                session_id: row.get(2)?,
                status: row.get(3)?,
                reason: row.get(4)?,
                patch_base_revision: row.get(5)?,
                apply_target_path: row.get(6)?,
                apply_branch: row.get(7)?,
                applied_at: row.get(8)?,
                apply_error: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })
        .optional()
        .map_err(|e| format!("get patch decision: {e}"))
    }

    pub fn upsert_patch_gate(&mut self, gate: &PatchGateRecord) -> Result<(), String> {
        let reasons_json =
            serde_json::to_string(&gate.reasons).unwrap_or_else(|_| "[]".to_string());
        let tx = self
            .conn
            .transaction()
            .map_err(|e| format!("begin patch gate tx: {e}"))?;
        tx.execute(
            "INSERT INTO patch_gate (
                mission_id, objective_id, session_id, status, reasons_json,
                watchdog_status, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(mission_id, objective_id) DO UPDATE SET
                session_id=excluded.session_id,
                status=excluded.status,
                reasons_json=excluded.reasons_json,
                watchdog_status=excluded.watchdog_status,
                updated_at=excluded.updated_at",
            params![
                gate.mission_id,
                gate.objective_id,
                gate.session_id,
                gate.status,
                reasons_json,
                gate.watchdog_status,
                gate.created_at,
                gate.updated_at,
            ],
        )
        .map_err(|e| format!("upsert patch gate: {e}"))?;
        tx.commit().map_err(|e| format!("commit patch gate: {e}"))
    }

    pub fn list_patch_gates(&self, mission_id: &str) -> Result<Vec<PatchGateRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT mission_id, objective_id, session_id, status, reasons_json, watchdog_status,
                        created_at, updated_at
                 FROM patch_gate
                 WHERE mission_id=?1
                 ORDER BY updated_at DESC, objective_id ASC",
            )
            .map_err(|e| format!("prepare list patch gates: {e}"))?;
        let rows = stmt
            .query_map(params![mission_id], |row| {
                let reasons_json: String = row.get(4)?;
                Ok(PatchGateRecord {
                    mission_id: row.get(0)?,
                    objective_id: row.get(1)?,
                    session_id: row.get(2)?,
                    status: row.get(3)?,
                    reasons: serde_json::from_str(&reasons_json).unwrap_or_default(),
                    watchdog_status: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .map_err(|e| format!("query list patch gates: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect patch gates: {e}"))
    }

    pub fn get_patch_gate(
        &self,
        mission_id: &str,
        objective_id: &str,
    ) -> Result<Option<PatchGateRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT mission_id, objective_id, session_id, status, reasons_json, watchdog_status,
                        created_at, updated_at
                 FROM patch_gate
                 WHERE mission_id=?1 AND objective_id=?2
                 LIMIT 1",
            )
            .map_err(|e| format!("prepare get patch gate: {e}"))?;
        stmt.query_row(params![mission_id, objective_id], |row| {
            let reasons_json: String = row.get(4)?;
            Ok(PatchGateRecord {
                mission_id: row.get(0)?,
                objective_id: row.get(1)?,
                session_id: row.get(2)?,
                status: row.get(3)?,
                reasons: serde_json::from_str(&reasons_json).unwrap_or_default(),
                watchdog_status: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .optional()
        .map_err(|e| format!("get patch gate: {e}"))
    }

    pub fn insert_verification(&self, verification: &NewVerificationRecord) -> Result<i64, String> {
        let now = now_utc();
        self.conn
            .execute(
                "INSERT INTO verification (
                    mission_id, objective_id, session_id, status, summary, details_json,
                    created_at, updated_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    verification.mission_id,
                    verification.objective_id,
                    verification.session_id,
                    verification.status,
                    verification.summary,
                    serde_json::to_string(&verification.details)
                        .unwrap_or_else(|_| "{}".to_string()),
                    now,
                    now,
                ],
            )
            .map_err(|e| format!("insert verification: {e}"))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_verification(
        &self,
        verification_id: i64,
        status: &str,
        summary: &str,
        details: &serde_json::Value,
    ) -> Result<(), String> {
        let now = now_utc();
        self.conn
            .execute(
                "UPDATE verification
                 SET status=?2, summary=?3, details_json=?4, updated_at=?5
                 WHERE id=?1",
                params![
                    verification_id,
                    status,
                    summary,
                    serde_json::to_string(details).unwrap_or_else(|_| "{}".to_string()),
                    now,
                ],
            )
            .map_err(|e| format!("update verification: {e}"))?;
        Ok(())
    }

    pub fn list_verifications(&self, mission_id: &str) -> Result<Vec<VerificationRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, mission_id, objective_id, session_id, status, summary, details_json, created_at, updated_at
                 FROM verification WHERE mission_id=?1 ORDER BY updated_at DESC, id DESC",
            )
            .map_err(|e| format!("prepare list verifications: {e}"))?;

        let verifications = stmt
            .query_map(params![mission_id], |row| {
                let details_json: String = row.get(6)?;
                Ok(VerificationRecord {
                    id: row.get(0)?,
                    mission_id: row.get(1)?,
                    objective_id: row.get(2)?,
                    session_id: row.get(3)?,
                    status: row.get(4)?,
                    summary: row.get(5)?,
                    details: serde_json::from_str(&details_json).unwrap_or(serde_json::Value::Null),
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })
            .map_err(|e| format!("query list verifications: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect verifications: {e}"))?;

        Ok(verifications)
    }

    pub fn get_latest_verification_for_objective(
        &self,
        mission_id: &str,
        objective_id: &str,
    ) -> Result<Option<VerificationRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, mission_id, objective_id, session_id, status, summary, details_json, created_at, updated_at
                 FROM verification
                 WHERE mission_id=?1 AND objective_id=?2
                 ORDER BY updated_at DESC, id DESC
                 LIMIT 1",
            )
            .map_err(|e| format!("prepare latest verification: {e}"))?;

        stmt.query_row(params![mission_id, objective_id], |row| {
            let details_json: String = row.get(6)?;
            Ok(VerificationRecord {
                id: row.get(0)?,
                mission_id: row.get(1)?,
                objective_id: row.get(2)?,
                session_id: row.get(3)?,
                status: row.get(4)?,
                summary: row.get(5)?,
                details: serde_json::from_str(&details_json).unwrap_or(serde_json::Value::Null),
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .optional()
        .map_err(|e| format!("latest verification: {e}"))
    }

    pub fn get_latest_verification_for_session(
        &self,
        mission_id: &str,
        session_id: &str,
    ) -> Result<Option<VerificationRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, mission_id, objective_id, session_id, status, summary, details_json, created_at, updated_at
                 FROM verification
                 WHERE mission_id=?1 AND session_id=?2
                 ORDER BY updated_at DESC, id DESC
                 LIMIT 1",
            )
            .map_err(|e| format!("prepare latest verification by session: {e}"))?;

        stmt.query_row(params![mission_id, session_id], |row| {
            let details_json: String = row.get(6)?;
            Ok(VerificationRecord {
                id: row.get(0)?,
                mission_id: row.get(1)?,
                objective_id: row.get(2)?,
                session_id: row.get(3)?,
                status: row.get(4)?,
                summary: row.get(5)?,
                details: serde_json::from_str(&details_json).unwrap_or(serde_json::Value::Null),
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .optional()
        .map_err(|e| format!("latest verification by session: {e}"))
    }

    pub fn load_mission_state(
        &self,
        mission_id: Option<&str>,
        event_limit: usize,
    ) -> Result<MissionStateView, String> {
        let mission = if let Some(mission_id) = mission_id {
            self.get_mission(mission_id)?
        } else {
            self.get_latest_mission()?
        };

        let Some(mission) = mission else {
            return Ok(MissionStateView {
                mission: None,
                objectives: Vec::new(),
                sessions: Vec::new(),
                profiles: Vec::new(),
                patch_decisions: Vec::new(),
                patch_gates: Vec::new(),
                verifications: Vec::new(),
                events: Vec::new(),
                summary: MissionSummary {
                    total_objectives: 0,
                    pending_objectives: 0,
                    claimed_objectives: 0,
                    done_objectives: 0,
                    failed_objectives: 0,
                    active_sessions: 0,
                    last_event_at: None,
                },
            });
        };

        let objectives = self.list_objectives(&mission.id)?;
        let sessions = self.list_agent_sessions(&mission.id)?;
        let profiles = self.list_agent_profiles()?;
        let patch_decisions = self.list_patch_decisions(&mission.id)?;
        let patch_gates = self.list_patch_gates(&mission.id)?;
        let verifications = self.list_verifications(&mission.id)?;
        let events = self.get_run_events(&mission.id, None, event_limit)?;
        let now_unix_seconds = current_unix_seconds();
        let summary = MissionSummary {
            total_objectives: objectives.len(),
            pending_objectives: objectives
                .iter()
                .filter(|objective| objective.status == WorkUnitStatus::Pending)
                .count(),
            claimed_objectives: objectives
                .iter()
                .filter(|objective| objective.status == WorkUnitStatus::Claimed)
                .count(),
            done_objectives: objectives
                .iter()
                .filter(|objective| objective.status == WorkUnitStatus::Done)
                .count(),
            failed_objectives: objectives
                .iter()
                .filter(|objective| objective.status == WorkUnitStatus::Failed)
                .count(),
            active_sessions: sessions
                .iter()
                .filter(|session| {
                    matches!(
                        derive_agent_session_runtime_slice_at(session, now_unix_seconds)
                            .runtime_status
                            .as_str(),
                        "running" | "idle" | "stuck"
                    )
                })
                .count(),
            last_event_at: events.last().map(|event| event.created_at.clone()),
        };

        Ok(MissionStateView {
            mission: Some(mission),
            objectives,
            sessions,
            profiles,
            patch_decisions,
            patch_gates,
            verifications,
            events,
            summary,
        })
    }

    /// Record a run event from an external runner for a specific work unit session.
    pub fn persist_run_event(
        &self,
        task_id: &str,
        unit_id: &str,
        session_id: &str,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<i64, String> {
        let now = now_utc();
        let payload_json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
        self.conn.execute(
            "INSERT INTO run_event (task_id, unit_id, session_id, event_type, payload_json, created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![task_id, unit_id, session_id, event_type, payload_json, now],
        )
        .map_err(|e| format!("insert run_event: {e}"))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Return up to `limit` run events for a task, optionally filtered by unit.
    /// Events are returned in insertion order (oldest first).
    pub fn get_run_events(
        &self,
        task_id: &str,
        unit_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RunEventRecord>, String> {
        let sql = if unit_id.is_some() {
            "SELECT id, task_id, unit_id, session_id, event_type, payload_json, created_at
             FROM run_event WHERE task_id=?1 AND unit_id=?2 ORDER BY id ASC LIMIT ?3"
        } else {
            "SELECT id, task_id, unit_id, session_id, event_type, payload_json, created_at
             FROM run_event WHERE task_id=?1 ORDER BY id ASC LIMIT ?3"
        };

        let unit_filter = unit_id.unwrap_or("");
        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| format!("prepare get_run_events: {e}"))?;
        let rows = stmt
            .query_map(params![task_id, unit_filter, limit as i64], |row| {
                let payload_str: String = row.get(5)?;
                Ok(RunEventRecord {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    unit_id: row.get(2)?,
                    session_id: row.get(3)?,
                    event_type: row.get(4)?,
                    payload: serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null),
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| format!("query get_run_events: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect run_events: {e}"))?;
        Ok(rows)
    }

    /// Return up to `limit` run events for a specific session within a task.
    pub fn get_run_events_for_session(
        &self,
        task_id: &str,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<RunEventRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, task_id, unit_id, session_id, event_type, payload_json, created_at
             FROM run_event WHERE task_id=?1 AND session_id=?2 ORDER BY id ASC LIMIT ?3",
            )
            .map_err(|e| format!("prepare get_run_events_for_session: {e}"))?;

        let rows = stmt
            .query_map(params![task_id, session_id, limit as i64], |row| {
                let payload_str: String = row.get(5)?;
                Ok(RunEventRecord {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    unit_id: row.get(2)?,
                    session_id: row.get(3)?,
                    event_type: row.get(4)?,
                    payload: serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null),
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| format!("query session events: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect session events: {e}"))?;
        Ok(rows)
    }

    fn load_unit_tx(
        tx: &Transaction<'_>,
        task_id: &str,
        unit_id: &str,
    ) -> Result<WorkUnit, String> {
        tx.query_row(
            "SELECT id, task_id, title, prompt, status, claimed_by, claimed_at,
                    started_at, retry_at, reconciled_at, run_attempt, retry_count,
                    max_retries, last_error, reconciled_outcome, reconciled_note, created_at
             FROM work_unit WHERE id=?1 AND task_id=?2",
            params![unit_id, task_id],
            |row| {
                Ok(WorkUnit {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    title: row.get(2)?,
                    prompt: row.get(3)?,
                    status: WorkUnitStatus::from_str(&row.get::<_, String>(4)?),
                    claimed_by: row.get(5)?,
                    claimed_at: row.get(6)?,
                    started_at: row.get(7)?,
                    retry_at: row.get(8)?,
                    reconciled_at: row.get(9)?,
                    run_attempt: row.get(10)?,
                    retry_count: row.get(11)?,
                    max_retries: row.get(12)?,
                    last_error: row.get(13)?,
                    reconciled_outcome: row.get(14)?,
                    reconciled_note: row.get(15)?,
                    created_at: row.get(16)?,
                })
            },
        )
        .map_err(|e| format!("load unit: {e}"))
    }
}

fn now_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{secs}")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conductor::{run_deterministic_code_worker, DeterministicWriteSpec};
    use tempfile::TempDir;

    fn tmp_store() -> (WorkStore, TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = WorkStore::open_or_create(dir.path()).expect("open store");
        (store, dir)
    }

    fn task_lifecycle_event(
        id: &str,
        event_type: TaskLifecycleEventType,
        actor: &str,
        causation: Option<&str>,
        occurred_at: &str,
        payload: serde_json::Value,
    ) -> TaskLifecycleEvent {
        TaskLifecycleEvent {
            id: id.into(),
            task_id: "task-123".into(),
            thread_id: "thread-abc".into(),
            r#type: event_type,
            actor: actor.into(),
            source: Some("coordinator".into()),
            correlation: Some("corr-789".into()),
            causation: causation.map(str::to_string),
            occurred_at: occurred_at.into(),
            payload,
        }
    }

    #[test]
    fn task_lifecycle_events_preserve_transition_ordering() {
        let events = vec![
            task_lifecycle_event(
                "evt-1",
                TaskLifecycleEventType::TaskCreated,
                "user",
                None,
                "unix:1",
                serde_json::json!({"title": "Coordinator first"}),
            ),
            task_lifecycle_event(
                "evt-2",
                TaskLifecycleEventType::PlanProposed,
                "planner",
                Some("evt-1"),
                "unix:2",
                serde_json::json!({"objectiveCount": 2}),
            ),
            task_lifecycle_event(
                "evt-3",
                TaskLifecycleEventType::PlanApproved,
                "user",
                Some("evt-2"),
                "unix:3",
                serde_json::json!({"approved": true}),
            ),
            task_lifecycle_event(
                "evt-4",
                TaskLifecycleEventType::WorkerStarted,
                "worker",
                Some("evt-3"),
                "unix:4",
                serde_json::json!({"sessionId": "sess-1", "objectiveId": "objective-1"}),
            ),
        ];

        let event_types: Vec<&str> = events.iter().map(|event| event.r#type.as_str()).collect();
        assert_eq!(
            event_types,
            vec![
                "task.created",
                "plan.proposed",
                "plan.approved",
                "worker.started"
            ]
        );
        assert_eq!(events[0].causation, None);
        assert_eq!(events[1].causation.as_deref(), Some("evt-1"));
        assert_eq!(events[2].causation.as_deref(), Some("evt-2"));
        assert_eq!(events[3].causation.as_deref(), Some("evt-3"));
        assert!(events
            .windows(2)
            .all(|pair| pair[0].occurred_at < pair[1].occurred_at));
    }

    #[test]
    fn task_lifecycle_event_envelope_serializes_required_fields() {
        let event = task_lifecycle_event(
            "evt-envelope",
            TaskLifecycleEventType::PlanProposed,
            "planner",
            Some("evt-previous"),
            "unix:22",
            serde_json::json!({"objectiveCount": 3}),
        );

        let json = serde_json::to_value(&event).expect("serialize lifecycle event");

        assert_eq!(json["id"], "evt-envelope");
        assert_eq!(json["taskId"], "task-123");
        assert_eq!(json["threadId"], "thread-abc");
        assert_eq!(json["type"], "plan.proposed");
        assert_eq!(json["actor"], "planner");
        assert_eq!(json["source"], "coordinator");
        assert_eq!(json["correlation"], "corr-789");
        assert_eq!(json["causation"], "evt-previous");
        assert_eq!(json["occurredAt"], "unix:22");
        assert_eq!(json["payload"]["objectiveCount"], 3);
    }

    #[test]
    fn verification_failed_lifecycle_event_shape_is_typed() {
        let event = task_lifecycle_event(
            "evt-verification-failed",
            TaskLifecycleEventType::VerificationFailed,
            "verifier",
            Some("evt-worker-started"),
            "unix:40",
            serde_json::json!({
                "objectiveId": "objective-1",
                "sessionId": "sess-1",
                "summary": "cargo test failed",
                "status": "failed",
                "details": {
                    "command": "cargo test",
                    "exitCode": 101
                }
            }),
        );

        let json = serde_json::to_value(&event).expect("serialize verification event");

        assert_eq!(event.r#type, TaskLifecycleEventType::VerificationFailed);
        assert_eq!(json["type"], "verification.failed");
        assert_eq!(json["actor"], "verifier");
        assert_eq!(json["source"], "coordinator");
        assert_eq!(json["correlation"], "corr-789");
        assert_eq!(json["causation"], "evt-worker-started");
        assert_eq!(json["payload"]["status"], "failed");
        assert_eq!(json["payload"]["summary"], "cargo test failed");
        assert_eq!(json["payload"]["details"]["command"], "cargo test");
        assert_eq!(json["payload"]["details"]["exitCode"], 101);
    }

    #[test]
    fn task_lifecycle_projection_orders_verification_failed_then_retry() {
        let run_events = vec![
            RunEventRecord {
                id: 1,
                task_id: "task-123".into(),
                unit_id: "objective-1".into(),
                session_id: "sess-1".into(),
                event_type: "started".into(),
                payload: serde_json::json!({"summary": "worker started"}),
                created_at: "unix:10".into(),
            },
            RunEventRecord {
                id: 2,
                task_id: "task-123".into(),
                unit_id: "objective-1".into(),
                session_id: "sess-1".into(),
                event_type: "retry_scheduled".into(),
                payload: serde_json::json!({"retryAt": "unix:30", "reason": "verification failed"}),
                created_at: "unix:30".into(),
            },
        ];
        let verifications = vec![VerificationRecord {
            id: 7,
            mission_id: "task-123".into(),
            objective_id: Some("objective-1".into()),
            session_id: Some("sess-1".into()),
            status: "failed".into(),
            summary: "cargo test failed".into(),
            details: serde_json::json!({
                "validation": {
                    "command": "cargo test",
                    "success": false,
                    "exitCode": 101
                },
                "retryScheduled": true,
                "retryAt": "unix:30"
            }),
            created_at: "unix:20".into(),
            updated_at: "unix:30".into(),
        }];

        let events = project_task_lifecycle_events_at(
            "task-123",
            "thread-abc",
            &[],
            &run_events,
            &[],
            &[],
            &verifications,
            40,
        );
        let event_types = events
            .iter()
            .map(|event| event.r#type.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            event_types,
            vec![
                "worker.started",
                "verification.failed",
                "task.retry_scheduled",
                "task.retry_scheduled"
            ]
        );
        assert_eq!(events[1].payload["summary"], "cargo test failed");
        assert_eq!(events[1].payload["details"]["validation"]["exitCode"], 101);
        assert_eq!(events[2].payload["reason"], "verification failed");
        assert_eq!(events[3].payload["verificationId"], "verification-7");
    }

    #[test]
    fn task_lifecycle_projection_marks_stale_session_path() {
        let sessions = vec![AgentSessionRuntimeRecord {
            id: "sess-stale".into(),
            mission_id: "task-123".into(),
            objective_id: Some("objective-1".into()),
            profile_id: "worker-profile".into(),
            assignment_kind: "execute_objective".into(),
            state: "running".into(),
            touched_paths: vec!["src/lib.rs".into()],
            workspace_path: "/tmp/worktree".into(),
            lease_expires_at: Some("unix:12".into()),
            last_heartbeat_at: Some("unix:10".into()),
            last_event_at: Some("unix:11".into()),
            last_error: None,
            created_at: "unix:9".into(),
            updated_at: "unix:11".into(),
        }];

        let events = project_task_lifecycle_events_at(
            "task-123",
            "thread-abc",
            &[],
            &[],
            &sessions,
            &[],
            &[],
            20,
        );
        let event_types = events
            .iter()
            .map(|event| event.r#type.as_str())
            .collect::<Vec<_>>();

        assert_eq!(event_types, vec!["worker.started", "session.stale"]);
        assert_eq!(events[1].payload["runtimeStatus"], "stuck");
        assert_eq!(events[1].payload["leaseExpired"], true);
        assert_eq!(events[1].payload["sessionId"], "sess-stale");
        assert_eq!(
            events[1].causation.as_deref(),
            Some("session-sess-stale-started")
        );
    }

    #[test]
    fn task_lifecycle_projection_attaches_review_decisions() {
        let decisions = vec![PatchDecisionRecord {
            mission_id: "task-123".into(),
            objective_id: "objective-1".into(),
            session_id: Some("sess-1".into()),
            status: "applied".into(),
            reason: None,
            patch_base_revision: Some("base-rev".into()),
            apply_target_path: Some("/repo".into()),
            apply_branch: Some("main".into()),
            applied_at: Some("unix:30".into()),
            apply_error: None,
            created_at: "unix:20".into(),
            updated_at: "unix:30".into(),
        }];

        let events = project_task_lifecycle_events_at(
            "task-123",
            "thread-abc",
            &[],
            &[],
            &[],
            &decisions,
            &[],
            40,
        );
        let event_types = events
            .iter()
            .map(|event| event.r#type.as_str())
            .collect::<Vec<_>>();

        assert_eq!(event_types, vec!["review.accepted", "patch.applied"]);
        assert_eq!(events[0].actor, "reviewer");
        assert_eq!(events[0].payload["objectiveId"], "objective-1");
        assert_eq!(events[0].payload["sessionId"], "sess-1");
        assert_eq!(events[1].actor, "system");
        assert_eq!(events[1].payload["branch"], "main");
        assert_eq!(
            events[1].causation.as_deref(),
            Some("patch-decision-task-123-objective-1-review-accepted")
        );
    }

    // ── Outer RED: full plan → claim → verdict lifecycle ─────────────────────

    /// Planning a task, claiming a unit, and submitting a verdict must
    /// advance statuses correctly end-to-end.
    #[test]
    fn plan_claim_verdict_lifecycle() {
        let (mut store, _dir) = tmp_store();

        store
            .plan_task(
                "task-1",
                "Refactor auth",
                "Make the auth module testable",
                &[
                    NewWorkUnit {
                        id: "u1".into(),
                        title: "Extract interface".into(),
                        prompt: "Extract AuthService interface".into(),
                    },
                    NewWorkUnit {
                        id: "u2".into(),
                        title: "Write tests".into(),
                        prompt: "Write unit tests for AuthService".into(),
                    },
                ],
            )
            .expect("plan_task");

        // Both start pending
        let units = store.list_work_units("task-1").expect("list");
        assert_eq!(units.len(), 2);
        assert!(units.iter().all(|u| u.status == WorkUnitStatus::Pending));

        // Claim the first one
        let claimed = store
            .claim_work_unit("task-1", "agent-42")
            .expect("claim")
            .expect("should have returned a unit");
        assert_eq!(claimed.id, "u1");
        assert_eq!(claimed.status, WorkUnitStatus::Claimed);
        assert_eq!(claimed.claimed_by.as_deref(), Some("agent-42"));

        // Second claim returns u2 (u1 is taken)
        let claimed2 = store
            .claim_work_unit("task-1", "agent-99")
            .expect("claim2")
            .expect("should have u2");
        assert_eq!(claimed2.id, "u2");

        // No more pending
        let none = store.claim_work_unit("task-1", "agent-0").expect("claim3");
        assert!(none.is_none(), "expected None when all units claimed");

        // Submit done verdict for u1
        store
            .submit_verdict(UnitVerdict {
                unit_id: "u1".into(),
                task_id: "task-1".into(),
                status: "done".into(),
                summary: "Interface extracted successfully".into(),
                submitted_at: "unix:0".into(),
                artifacts: vec![VerdictArtifact {
                    kind: "patch".into(),
                    content: "diff...".into(),
                }],
            })
            .expect("submit verdict");

        let units = store.list_work_units("task-1").expect("list after verdict");
        let u1 = units.iter().find(|u| u.id == "u1").unwrap();
        let u2 = units.iter().find(|u| u.id == "u2").unwrap();
        assert_eq!(u1.status, WorkUnitStatus::Done);
        assert_eq!(u2.status, WorkUnitStatus::Claimed);
    }

    // ── Outer RED: event ingest lifecycle ────────────────────────────────────

    /// Ingesting run events must store them in order and make them queryable
    /// by task, unit, and session.
    #[test]
    fn ingest_and_query_run_events() {
        let (mut store, _dir) = tmp_store();

        store
            .plan_task(
                "t",
                "T",
                "p",
                &[NewWorkUnit {
                    id: "u1".into(),
                    title: "U1".into(),
                    prompt: "".into(),
                }],
            )
            .expect("plan");
        store.claim_work_unit("t", "agent-1").expect("claim");

        store
            .persist_run_event(
                "t",
                "u1",
                "sess-99",
                "started",
                serde_json::json!({"at": "unix:1"}),
            )
            .expect("started");
        store
            .persist_run_event(
                "t",
                "u1",
                "sess-99",
                "progress",
                serde_json::json!({"message": "halfway"}),
            )
            .expect("progress");
        store
            .persist_run_event(
                "t",
                "u1",
                "sess-99",
                "completed",
                serde_json::json!({"summary": "done!"}),
            )
            .expect("completed");

        let events = store
            .get_run_events("t", Some("u1"), 10)
            .expect("get events");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, "started");
        assert_eq!(events[1].event_type, "progress");
        assert_eq!(events[2].event_type, "completed");

        // Filtering by session
        let by_sess = store
            .get_run_events_for_session("t", "sess-99", 10)
            .expect("by session");
        assert_eq!(by_sess.len(), 3);

        // Task-level query (no unit filter)
        let all = store.get_run_events("t", None, 10).expect("all events");
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn replanning_clears_old_units() {
        let (mut store, _dir) = tmp_store();

        store
            .plan_task(
                "t",
                "T",
                "p",
                &[NewWorkUnit {
                    id: "old".into(),
                    title: "Old".into(),
                    prompt: "".into(),
                }],
            )
            .expect("first plan");

        store
            .plan_task(
                "t",
                "T v2",
                "p2",
                &[
                    NewWorkUnit {
                        id: "new-a".into(),
                        title: "New A".into(),
                        prompt: "".into(),
                    },
                    NewWorkUnit {
                        id: "new-b".into(),
                        title: "New B".into(),
                        prompt: "".into(),
                    },
                ],
            )
            .expect("replan");

        let units = store.list_work_units("t").expect("list");
        assert_eq!(units.len(), 2);
        assert!(units.iter().all(|u| u.id.starts_with("new-")));
    }

    #[test]
    fn mission_state_roundtrip_persists_objectives_sessions_and_events() {
        let (mut store, _dir) = tmp_store();
        let mission = MissionRecord {
            id: "mission-1".into(),
            repo_path: "/repo".into(),
            worktree_path: "/repo".into(),
            graph_snapshot: "@".into(),
            title: "Mission".into(),
            brief: "Do thing".into(),
            validation_command: "npm run -s build".into(),
            planner_notes: None,
            planner_profile_id: None,
            planner_prompt_override: None,
            issue_key: Some("LIN-1".into()),
            issue_url: Some("https://linear.app/lin-1".into()),
            instruction_pack_hash: "abc123".into(),
            state: "planned".into(),
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };
        store.upsert_mission(&mission).expect("upsert mission");
        store
            .plan_mission(
                &mission,
                "mission prompt",
                &[NewObjective {
                    id: "outer-red".into(),
                    title: "Outer red".into(),
                    prompt: "write failing test".into(),
                    scope_paths: vec!["e2e/".into()],
                    expected_validation: vec!["npm run -s build".into()],
                    done_criteria: vec!["failing test exists".into()],
                    test_intent: Some("outer_red".into()),
                }],
            )
            .expect("plan mission");
        store
            .upsert_agent_session(&AgentSessionRecord {
                id: "sess-1".into(),
                mission_id: mission.id.clone(),
                objective_id: Some("outer-red".into()),
                profile_id: "fixture-worker".into(),
                assignment_kind: "execute_objective".into(),
                state: "running".into(),
                touched_paths: vec!["e2e/mission.spec.js".into()],
                created_at: "unix:2".into(),
                updated_at: "unix:2".into(),
            })
            .expect("upsert session");
        store
            .persist_run_event(
                &mission.id,
                "outer-red",
                "sess-1",
                "started",
                serde_json::json!({"message": "go"}),
            )
            .expect("event");

        let state = store
            .load_mission_state(Some(&mission.id), 20)
            .expect("load mission state");
        assert_eq!(
            state.mission.as_ref().map(|m| m.id.as_str()),
            Some("mission-1")
        );
        assert_eq!(state.objectives.len(), 1);
        assert_eq!(state.objectives[0].meta.scope_paths, vec!["e2e/"]);
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].id, "sess-1");
        assert_eq!(state.events.len(), 1);
        assert_eq!(state.events[0].event_type, "started");
        assert!(state.verifications.is_empty());
        assert_eq!(state.summary.total_objectives, 1);
    }

    #[test]
    fn verification_records_persist_and_load_with_mission_state() {
        let (mut store, _dir) = tmp_store();
        let mission = MissionRecord {
            id: "mission-ver".into(),
            repo_path: "/repo".into(),
            worktree_path: "/repo".into(),
            graph_snapshot: "@".into(),
            title: "Mission".into(),
            brief: "Do thing".into(),
            validation_command: "npm run -s build".into(),
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
        store.upsert_mission(&mission).expect("upsert mission");

        let verification_id = store
            .insert_verification(&NewVerificationRecord {
                mission_id: mission.id.clone(),
                objective_id: Some("outer-red".into()),
                session_id: Some("sess-ver".into()),
                status: "passed".into(),
                summary: "validation green".into(),
                details: serde_json::json!({
                    "checks": ["build", "tests"]
                }),
            })
            .expect("insert verification");
        assert!(verification_id > 0);

        let verifications = store.list_verifications(&mission.id).expect("list");
        assert_eq!(verifications.len(), 1);
        assert_eq!(verifications[0].mission_id, mission.id);
        assert_eq!(verifications[0].status, "passed");
        assert_eq!(verifications[0].details["checks"][0], "build");

        let state = store
            .load_mission_state(Some(&mission.id), 20)
            .expect("load mission state");
        assert_eq!(state.verifications.len(), 1);
        assert_eq!(state.verifications[0].summary, "validation green");
        assert_eq!(state.summary.total_objectives, 0);
        assert_eq!(state.summary.active_sessions, 0);
    }

    #[test]
    fn agent_session_runtime_survives_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().to_path_buf();

        {
            let mut store = WorkStore::open_or_create(&repo).expect("open store");
            let mission = MissionRecord {
                id: "mission-session".into(),
                repo_path: "/repo".into(),
                worktree_path: "/repo".into(),
                graph_snapshot: "@".into(),
                title: "Mission".into(),
                brief: "Do thing".into(),
                validation_command: "npm run -s build".into(),
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
            store.upsert_mission(&mission).expect("upsert mission");
            store
                .upsert_agent_session_runtime(&AgentSessionRuntimeRecord {
                    id: "sess-runtime".into(),
                    mission_id: mission.id.clone(),
                    objective_id: Some("outer-red".into()),
                    profile_id: "worker-1".into(),
                    assignment_kind: "execute_objective".into(),
                    state: "running".into(),
                    touched_paths: vec!["src/lib.rs".into()],
                    workspace_path: "/repo/worktrees/worker-1".into(),
                    lease_expires_at: Some("unix:10".into()),
                    last_heartbeat_at: Some("unix:3".into()),
                    last_event_at: Some("unix:4".into()),
                    last_error: Some("timeout".into()),
                    created_at: "unix:2".into(),
                    updated_at: "unix:5".into(),
                })
                .expect("upsert runtime session");
        }

        let store = WorkStore::open_or_create(&repo).expect("reopen store");
        let sessions = store
            .list_agent_sessions("mission-session")
            .expect("list sessions");
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.id, "sess-runtime");
        assert_eq!(session.workspace_path, "/repo/worktrees/worker-1");
        assert_eq!(session.lease_expires_at.as_deref(), Some("unix:10"));
        assert_eq!(session.last_heartbeat_at.as_deref(), Some("unix:3"));
        assert_eq!(session.last_event_at.as_deref(), Some("unix:4"));
        assert_eq!(session.last_error.as_deref(), Some("timeout"));
        assert_eq!(session.touched_paths, vec!["src/lib.rs"]);
    }

    fn runtime_session_fixture() -> AgentSessionRuntimeRecord {
        AgentSessionRuntimeRecord {
            id: "sess-runtime".into(),
            mission_id: "mission-runtime".into(),
            objective_id: Some("objective-1".into()),
            profile_id: "worker-1".into(),
            assignment_kind: "execute_objective".into(),
            state: "running".into(),
            touched_paths: vec!["src/lib.rs".into()],
            workspace_path: "/tmp/worktree".into(),
            lease_expires_at: Some("unix:220".into()),
            last_heartbeat_at: Some("unix:195".into()),
            last_event_at: Some("unix:195".into()),
            last_error: None,
            created_at: "unix:190".into(),
            updated_at: "unix:195".into(),
        }
    }

    #[test]
    fn derive_agent_session_runtime_slice_preserves_terminal_statuses() {
        for (state, expected) in [
            ("completed", "completed"),
            ("failed", "failed"),
            ("applied", "applied"),
            ("logged_out", "completed"),
        ] {
            let mut session = runtime_session_fixture();
            session.state = state.to_string();
            session.lease_expires_at = Some("unix:150".into());
            session.last_heartbeat_at = Some("unix:1".into());
            let slice = derive_agent_session_runtime_slice_at(&session, 200);
            assert_eq!(slice.runtime_status, expected, "state={state}");
        }
    }

    #[test]
    fn derive_agent_session_runtime_slice_marks_stuck_when_lease_expired() {
        let mut session = runtime_session_fixture();
        session.lease_expires_at =
            Some(format!("unix:{}", 200 - AGENT_SESSION_RUNNING_WINDOW_SECS));
        session.last_heartbeat_at = Some("unix:199".into());
        session.last_event_at = Some("unix:199".into());

        let slice = derive_agent_session_runtime_slice_at(&session, 200);
        assert_eq!(slice.runtime_status, "stuck");
        assert_eq!(slice.heartbeat_age_sec, Some(1));
        assert_eq!(slice.event_age_sec, Some(1));
        assert!(slice.lease_expired);
    }

    #[test]
    fn derive_agent_session_runtime_slice_marks_stuck_when_heartbeat_is_stale() {
        let mut session = runtime_session_fixture();
        session.last_heartbeat_at = Some(format!(
            "unix:{}",
            200 - AGENT_SESSION_STUCK_HEARTBEAT_TIMEOUT_SECS - 1
        ));
        session.last_event_at = Some("unix:199".into());
        session.lease_expires_at = Some("unix:260".into());

        let slice = derive_agent_session_runtime_slice_at(&session, 200);
        assert_eq!(slice.runtime_status, "stuck");
        assert_eq!(
            slice.heartbeat_age_sec,
            Some(AGENT_SESSION_STUCK_HEARTBEAT_TIMEOUT_SECS + 1)
        );
        assert_eq!(slice.event_age_sec, Some(1));
        assert!(!slice.lease_expired);
    }

    #[test]
    fn derive_agent_session_runtime_slice_marks_running_when_activity_is_recent() {
        let mut session = runtime_session_fixture();
        session.last_heartbeat_at =
            Some(format!("unix:{}", 200 - AGENT_SESSION_RUNNING_WINDOW_SECS));
        session.last_event_at = Some("unix:199".into());
        session.lease_expires_at = Some("unix:260".into());

        let slice = derive_agent_session_runtime_slice_at(&session, 200);
        assert_eq!(slice.runtime_status, "running");
        assert_eq!(
            slice.heartbeat_age_sec,
            Some(AGENT_SESSION_RUNNING_WINDOW_SECS)
        );
        assert_eq!(slice.event_age_sec, Some(1));
        assert!(!slice.lease_expired);
    }

    #[test]
    fn derive_agent_session_runtime_slice_marks_idle_between_recent_and_stuck() {
        let mut session = runtime_session_fixture();
        session.last_heartbeat_at = Some(format!(
            "unix:{}",
            200 - AGENT_SESSION_RUNNING_WINDOW_SECS - 1
        ));
        session.last_event_at = Some(format!(
            "unix:{}",
            200 - AGENT_SESSION_RUNNING_WINDOW_SECS - 1
        ));
        session.lease_expires_at = Some("unix:260".into());

        let slice = derive_agent_session_runtime_slice_at(&session, 200);
        assert_eq!(slice.runtime_status, "idle");
        assert_eq!(
            slice.heartbeat_age_sec,
            Some(AGENT_SESSION_RUNNING_WINDOW_SECS + 1)
        );
        assert_eq!(
            slice.event_age_sec,
            Some(AGENT_SESSION_RUNNING_WINDOW_SECS + 1)
        );
        assert!(!slice.lease_expired);
    }

    #[test]
    fn update_agent_session_status_refreshes_lease_for_running_session() {
        let repo = tempfile::tempdir().expect("tempdir");
        let mut store = WorkStore::open_or_create(repo.path()).expect("store");
        store
            .upsert_agent_session_runtime(&AgentSessionRuntimeRecord {
                id: "sess-refresh".into(),
                mission_id: "mission-runtime".into(),
                objective_id: Some("objective-1".into()),
                profile_id: "worker-1".into(),
                assignment_kind: "execute_objective".into(),
                state: "running".into(),
                touched_paths: vec![],
                workspace_path: "/tmp/worktree".into(),
                lease_expires_at: Some("unix:1".into()),
                last_heartbeat_at: Some("unix:1".into()),
                last_event_at: Some("unix:1".into()),
                last_error: None,
                created_at: "unix:1".into(),
                updated_at: "unix:1".into(),
            })
            .expect("upsert session");

        store
            .update_agent_session_status("sess-refresh", "running", "unix:200", None)
            .expect("refresh session");

        let session = store
            .get_agent_session_runtime("sess-refresh")
            .expect("get session")
            .expect("session exists");
        assert_eq!(session.lease_expires_at.as_deref(), Some("unix:320"));
        assert_eq!(session.last_heartbeat_at.as_deref(), Some("unix:200"));
        assert_eq!(session.last_event_at.as_deref(), Some("unix:200"));
    }

    #[test]
    fn code_worker_verification_details_survive_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().to_path_buf();
        std::fs::write(repo.join("app.js"), "export const answer = 1;\n").expect("seed file");
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo)
            .output()
            .expect("git init");
        std::process::Command::new("git")
            .args(["config", "user.email", "slopo@example.test"])
            .current_dir(&repo)
            .output()
            .expect("git email");
        std::process::Command::new("git")
            .args(["config", "user.name", "Sloposcope Test"])
            .current_dir(&repo)
            .output()
            .expect("git name");
        std::process::Command::new("git")
            .args(["add", "app.js"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new("git")
            .args(["commit", "-m", "seed"])
            .current_dir(&repo)
            .output()
            .expect("git commit");

        let mission = MissionRecord {
            id: "mission-code-worker".into(),
            repo_path: repo.display().to_string(),
            worktree_path: repo.display().to_string(),
            graph_snapshot: "@".into(),
            title: "Mission".into(),
            brief: "Do thing".into(),
            validation_command: "test \"$(cat app.js)\" = \"export const answer = 2;\"".into(),
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

        {
            let mut store = WorkStore::open_or_create(&repo).expect("open store");
            store.upsert_mission(&mission).expect("upsert mission");
            let result = run_deterministic_code_worker(
                &repo,
                &mission.id,
                "objective-code-worker",
                "session-code-worker",
                &DeterministicWriteSpec {
                    relative_path: "app.js".into(),
                    content: "export const answer = 2;\n".into(),
                    validation_command:
                        "test \"$(cat app.js)\" = \"export const answer = 2;\" && printf ok".into(),
                },
            )
            .expect("run deterministic worker");
            store
                .insert_verification(&result.to_verification_record())
                .expect("insert verification");
        }

        let store = WorkStore::open_or_create(&repo).expect("reopen store");
        let verifications = store
            .list_verifications(&mission.id)
            .expect("list verifications");
        assert_eq!(verifications.len(), 1);
        let details = &verifications[0].details;
        assert_eq!(details["relativePath"], "app.js");
        assert!(details["patch"]
            .as_str()
            .expect("patch string")
            .contains("+export const answer = 2;"));
        assert_eq!(details["validation"]["success"], true);
        assert_eq!(details["validation"]["stdout"], "ok");
    }

    #[test]
    fn agent_profiles_and_patch_records_persist_in_mission_state() {
        let (mut store, _dir) = tmp_store();
        let mission = MissionRecord {
            id: "mission-patch-records".into(),
            repo_path: "/repo".into(),
            worktree_path: "/repo".into(),
            graph_snapshot: "abc123".into(),
            title: "Mission".into(),
            brief: "Do thing".into(),
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
        store.upsert_mission(&mission).expect("upsert mission");
        store
            .plan_mission(
                &mission,
                "mission prompt",
                &[NewObjective {
                    id: "objective-1".into(),
                    title: "Objective 1".into(),
                    prompt: "implement".into(),
                    scope_paths: vec!["src/".into()],
                    expected_validation: vec!["cargo test".into()],
                    done_criteria: vec!["done".into()],
                    test_intent: Some("green".into()),
                }],
            )
            .expect("plan mission");
        store
            .upsert_agent_profile(&AgentProfile {
                id: "local-cli".into(),
                label: "Local CLI".into(),
                provider: "local_cli".into(),
                model: Some("test-model".into()),
                command: "/bin/sh".into(),
                args: vec!["-lc".into(), "printf ok".into()],
                env: serde_json::json!({"FOO": "bar"}),
                capabilities: vec!["can_edit".into()],
                transport_kind: "pty".into(),
                active: true,
                is_reviewer: false,
                created_at: "unix:2".into(),
                updated_at: "unix:2".into(),
            })
            .expect("upsert profile");
        store
            .upsert_patch_decision(&PatchDecisionRecord {
                mission_id: mission.id.clone(),
                objective_id: "objective-1".into(),
                session_id: Some("sess-1".into()),
                status: "accepted".into(),
                reason: None,
                patch_base_revision: Some("abc123".into()),
                apply_target_path: None,
                apply_branch: None,
                applied_at: None,
                apply_error: None,
                created_at: "unix:3".into(),
                updated_at: "unix:3".into(),
            })
            .expect("upsert decision");
        store
            .upsert_patch_gate(&PatchGateRecord {
                mission_id: mission.id.clone(),
                objective_id: "objective-1".into(),
                session_id: Some("sess-1".into()),
                status: "ready".into(),
                reasons: vec![],
                watchdog_status: Some("on_track".into()),
                created_at: "unix:4".into(),
                updated_at: "unix:4".into(),
            })
            .expect("upsert gate");

        let state = store
            .load_mission_state(Some(&mission.id), 20)
            .expect("load mission state");
        assert_eq!(state.profiles.len(), 1);
        assert_eq!(state.profiles[0].id, "local-cli");
        assert_eq!(state.patch_decisions.len(), 1);
        assert_eq!(state.patch_decisions[0].status, "accepted");
        assert_eq!(state.patch_gates.len(), 1);
        assert_eq!(state.patch_gates[0].status, "ready");
    }

    #[test]
    fn open_or_create_migrates_old_agent_profile_schema() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join(".sloposcope").join("conductor.sqlite3");
        std::fs::create_dir_all(db_path.parent().expect("db parent")).expect("create db dir");
        let conn = Connection::open(&db_path).expect("open raw sqlite");
        conn.execute_batch(
            "
            CREATE TABLE agent_profile (
                id TEXT PRIMARY KEY,
                label TEXT NOT NULL,
                provider TEXT NOT NULL
            );
            INSERT INTO agent_profile (id, label, provider)
            VALUES ('legacy', 'Legacy profile', 'local_cli');
            ",
        )
        .expect("seed old schema");
        drop(conn);

        let store = WorkStore::open_or_create(dir.path()).expect("migrate store");
        let profile = store
            .get_agent_profile("legacy")
            .expect("get profile")
            .expect("profile exists");
        assert_eq!(profile.id, "legacy");
        assert_eq!(profile.label, "Legacy profile");
        assert_eq!(profile.provider, "local_cli");
        assert!(!profile.is_reviewer);
        assert_eq!(profile.transport_kind, "internal");
    }
}

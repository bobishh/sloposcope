use crate::watchdog_types;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[allow(dead_code)]
#[path = "deterministic_worker.rs"]
mod deterministic_worker;

#[allow(unused_imports)]
pub use deterministic_worker::{
    ensure_isolated_git_worktree, run_deterministic_code_worker, DeterministicWriteSpec,
    LocalCodeWorkerResult, ValidationRunResult,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConductorTaskState {
    Claimed,
    Running,
    RetryScheduled,
    Reconciled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConductorReconciledOutcome {
    Succeeded,
    Failed,
    Cancelled,
    Exhausted,
}

fn default_workflow_max_retries() -> u32 {
    2
}

fn default_workflow_timeout_seconds() -> u64 {
    900
}

fn default_workflow_provider_strict_mode() -> bool {
    false
}

fn default_workflow_trace_retention() -> usize {
    30
}

fn default_runner_provider() -> ConductorRunnerProvider {
    ConductorRunnerProvider::CodingAgentAppServer
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConductorWorkflowFrontMatter {
    pub schema_version: u32,
    pub workflow_id: String,
    #[serde(default = "default_runner_provider")]
    pub provider: ConductorRunnerProvider,
    #[serde(default)]
    pub provider_model: Option<String>,
    #[serde(default = "default_workflow_provider_strict_mode")]
    pub provider_strict_mode: bool,
    #[serde(default = "default_workflow_trace_retention")]
    pub trace_retention: usize,
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default = "default_workflow_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_workflow_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub required_checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorWorkflowContract {
    pub front_matter: ConductorWorkflowFrontMatter,
    pub prompt_template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorWorkflowContractLoad {
    pub path: String,
    pub exists: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub contract: Option<ConductorWorkflowContract>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConductorWorkflowSettingsUpdate {
    pub provider: Option<ConductorRunnerProvider>,
    pub provider_model: Option<String>,
    pub provider_strict_mode: Option<bool>,
    pub timeout_seconds: Option<u64>,
    pub trace_retention: Option<usize>,
}

pub fn workflow_contract_path(repo: &Path) -> PathBuf {
    repo.join("WORKFLOW.md")
}

fn split_workflow_contract(content: &str) -> Result<(String, String), String> {
    let normalized = content.replace("\r\n", "\n");
    let normalized = normalized.trim_start_matches('\u{feff}');
    let mut lines = normalized.lines();

    if lines.next().map(str::trim) != Some("---") {
        return Err("WORKFLOW.md must start with YAML front matter fenced by ---".to_string());
    }

    let mut yaml_lines: Vec<String> = Vec::new();
    let mut found_closing_fence = false;
    for line in lines.by_ref() {
        if line.trim() == "---" {
            found_closing_fence = true;
            break;
        }
        yaml_lines.push(line.to_string());
    }

    if !found_closing_fence {
        return Err("WORKFLOW.md front matter closing --- is missing".to_string());
    }
    if yaml_lines.is_empty() {
        return Err("WORKFLOW.md front matter is empty".to_string());
    }

    let prompt_template = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    if prompt_template.is_empty() {
        return Err("WORKFLOW.md prompt template body is empty".to_string());
    }

    Ok((yaml_lines.join("\n"), prompt_template))
}

pub fn parse_workflow_contract(content: &str) -> Result<ConductorWorkflowContract, String> {
    let (front_matter_raw, prompt_template) = split_workflow_contract(content)?;
    let front_matter: ConductorWorkflowFrontMatter = serde_yaml::from_str(&front_matter_raw)
        .map_err(|err| format!("WORKFLOW.md front matter parse error: {err}"))?;

    Ok(ConductorWorkflowContract {
        front_matter,
        prompt_template,
    })
}

pub fn load_workflow_contract(repo: &Path) -> ConductorWorkflowContractLoad {
    let path = workflow_contract_path(repo);
    let path_label = path.display().to_string();
    if !path.is_file() {
        return ConductorWorkflowContractLoad {
            path: path_label,
            exists: false,
            loaded: false,
            error: None,
            contract: None,
        };
    }

    match fs::read_to_string(&path) {
        Ok(content) => match parse_workflow_contract(&content) {
            Ok(contract) => ConductorWorkflowContractLoad {
                path: path_label,
                exists: true,
                loaded: true,
                error: None,
                contract: Some(contract),
            },
            Err(error) => ConductorWorkflowContractLoad {
                path: path_label,
                exists: true,
                loaded: false,
                error: Some(error),
                contract: None,
            },
        },
        Err(err) => ConductorWorkflowContractLoad {
            path: path_label,
            exists: true,
            loaded: false,
            error: Some(format!("Failed to read WORKFLOW.md: {err}")),
            contract: None,
        },
    }
}

pub fn save_workflow_settings(
    repo: &Path,
    update: &ConductorWorkflowSettingsUpdate,
) -> Result<ConductorWorkflowContractLoad, String> {
    let path = workflow_contract_path(repo);
    if !path.is_file() {
        return Err(format!("WORKFLOW.md not found at {}", path.display()));
    }

    let content =
        fs::read_to_string(&path).map_err(|err| format!("Failed to read WORKFLOW.md: {err}"))?;
    let (_front_matter_raw, prompt_template) = split_workflow_contract(&content)?;
    let mut contract = parse_workflow_contract(&content)?;

    if let Some(provider) = &update.provider {
        contract.front_matter.provider = provider.clone();
    }
    if let Some(model) = &update.provider_model {
        let normalized = model.trim().to_string();
        contract.front_matter.provider_model = if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        };
    }
    if let Some(strict_mode) = update.provider_strict_mode {
        contract.front_matter.provider_strict_mode = strict_mode;
    }
    if let Some(timeout_seconds) = update.timeout_seconds {
        contract.front_matter.timeout_seconds = timeout_seconds.clamp(10, 3_600);
    }
    if let Some(trace_retention) = update.trace_retention {
        contract.front_matter.trace_retention = trace_retention.clamp(1, 200);
    }

    let mut yaml = serde_yaml::to_string(&contract.front_matter)
        .map_err(|err| format!("Failed to serialize workflow front matter: {err}"))?;
    if let Some(stripped) = yaml.strip_prefix("---\n") {
        yaml = stripped.to_string();
    }
    let yaml = yaml.trim_end();
    let body = prompt_template.trim();
    let next = format!("---\n{}\n---\n{}\n", yaml, body);
    fs::write(&path, next).map_err(|err| format!("Failed to write WORKFLOW.md: {err}"))?;

    Ok(load_workflow_contract(repo))
}

fn normalize_task_id(task_id: &str) -> Result<String, String> {
    let normalized = task_id.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("task_id is empty".to_string());
    }
    if normalized.len() > 64 {
        return Err(format!("task_id is too long ({} > 64)", normalized.len()));
    }
    if normalized.contains("..") {
        return Err("task_id must not contain '..'".to_string());
    }
    if !normalized
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("task_id contains unsupported characters; allowed: a-z 0-9 _ -".to_string());
    }
    Ok(normalized)
}

fn task_id_hash(task_id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    task_id.hash(&mut hasher);
    hasher.finish()
}

pub fn workspace_name_for_task(task_id: &str) -> Result<String, String> {
    let normalized = normalize_task_id(task_id)?;
    let prefix_len = normalized.len().min(32);
    let prefix = &normalized[..prefix_len];
    Ok(format!(
        "task-{}-{:08x}",
        prefix,
        task_id_hash(&normalized) as u32
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorWorkspaceSpec {
    pub task_id: String,
    pub workspace_name: String,
    pub workspace_path: String,
}

pub fn ensure_task_workspace(
    workspace_root: &Path,
    task_id: &str,
) -> Result<ConductorWorkspaceSpec, String> {
    if !workspace_root.exists() {
        fs::create_dir_all(workspace_root)
            .map_err(|err| format!("Failed to create workspace root: {err}"))?;
    }
    let root_canonical = workspace_root
        .canonicalize()
        .map_err(|err| format!("Failed to canonicalize workspace root: {err}"))?;
    let workspace_name = workspace_name_for_task(task_id)?;
    let workspace_path = root_canonical.join(&workspace_name);
    if !workspace_path.starts_with(&root_canonical) {
        return Err("Workspace path escaped root bounds".to_string());
    }

    fs::create_dir_all(&workspace_path)
        .map_err(|err| format!("Failed to create task workspace: {err}"))?;
    let workspace_canonical = workspace_path
        .canonicalize()
        .map_err(|err| format!("Failed to canonicalize task workspace: {err}"))?;
    if !workspace_canonical.starts_with(&root_canonical) {
        return Err("Task workspace canonical path escaped root bounds".to_string());
    }

    Ok(ConductorWorkspaceSpec {
        task_id: task_id.to_string(),
        workspace_name,
        workspace_path: workspace_canonical.display().to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConductorRunnerProvider {
    CodingAgentAppServer,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "nim")]
    Nim,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorRunnerLaunchRequest {
    #[serde(default = "default_runner_provider")]
    pub provider: ConductorRunnerProvider,
    pub task_id: String,
    pub run_attempt: u32,
    pub workflow_id: String,
    pub workspace_path: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorRunnerLaunchPayload {
    pub protocol_version: u32,
    pub provider: ConductorRunnerProvider,
    pub action: String,
    pub session_id: String,
    pub task_id: String,
    pub run_attempt: u32,
    pub workflow_id: String,
    pub workspace_path: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorOpenAiRunConfig {
    pub model: String,
    pub timeout_seconds: u64,
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorOpenAiRunResult {
    pub config: ConductorOpenAiRunConfig,
    pub output_text: String,
    pub verdict: watchdog_types::WatchdogVerdict,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAiChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAiChatCompletionsRequest {
    model: String,
    temperature: f32,
    messages: Vec<OpenAiChatMessage>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiChatCompletionsResponse {
    choices: Vec<OpenAiChatChoice>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiChatChoice {
    message: OpenAiChatChoiceMessage,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiChatChoiceMessage {
    content: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenAiVerdictIssue {
    kind: String,
    severity: String,
    title: String,
    why: String,
    #[serde(default)]
    evidence: Vec<String>,
    #[serde(alias = "recommended_next_step")]
    recommended_next_step: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenAiVerdictPayload {
    status: String,
    confidence: f32,
    summary: String,
    #[serde(default)]
    issues: Vec<OpenAiVerdictIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "eventType", rename_all = "snake_case")]
pub enum ConductorRunnerEvent {
    Started {
        session_id: String,
        started_at: String,
    },
    Progress {
        session_id: String,
        message: String,
    },
    Completed {
        session_id: String,
        completed_at: String,
        result_summary: Option<String>,
    },
    Failed {
        session_id: String,
        failed_at: String,
        error: String,
    },
}

pub fn app_server_session_id(task_id: &str, run_attempt: u32) -> Result<String, String> {
    let normalized = normalize_task_id(task_id)?;
    Ok(format!(
        "slopo-{}-a{}-{:08x}",
        normalized,
        run_attempt,
        task_id_hash(&format!("{normalized}:{run_attempt}")) as u32
    ))
}

pub fn build_app_server_launch_payload(
    request: &ConductorRunnerLaunchRequest,
) -> Result<ConductorRunnerLaunchPayload, String> {
    if request.provider != ConductorRunnerProvider::CodingAgentAppServer {
        return Err(
            "build_app_server_launch_payload requires provider=coding_agent_app_server".to_string(),
        );
    }
    if request.workflow_id.trim().is_empty() {
        return Err("workflow_id is empty".to_string());
    }
    if request.prompt.trim().is_empty() {
        return Err("prompt is empty".to_string());
    }
    let workspace = PathBuf::from(request.workspace_path.trim());
    if !workspace.is_absolute() {
        return Err("workspace_path must be absolute".to_string());
    }

    let session_id = app_server_session_id(&request.task_id, request.run_attempt)?;
    Ok(ConductorRunnerLaunchPayload {
        protocol_version: 1,
        provider: ConductorRunnerProvider::CodingAgentAppServer,
        action: "start_session".to_string(),
        session_id,
        task_id: request.task_id.clone(),
        run_attempt: request.run_attempt,
        workflow_id: request.workflow_id.clone(),
        workspace_path: request.workspace_path.clone(),
        prompt: request.prompt.clone(),
    })
}

pub fn parse_app_server_event(raw: &str) -> Result<ConductorRunnerEvent, String> {
    serde_json::from_str(raw)
        .map_err(|err| format!("Invalid app-server runner event payload: {err}"))
}

pub fn build_openai_prompt(prompt_template: &str, snapshot_json: &str) -> Result<String, String> {
    if prompt_template.trim().is_empty() {
        return Err("prompt_template is empty".to_string());
    }
    if snapshot_json.trim().is_empty() {
        return Err("snapshot_json is empty".to_string());
    }

    Ok(format!(
        "{prompt_template}\n\nSnapshot JSON:\n{snapshot_json}\n\nOpenAI output contract:\n- Return exactly one valid JSON object.\n- Do not wrap the answer in markdown or code fences.\n- Allowed top-level keys only: status, confidence, summary, issues.\n- status must be one of: on_track, drift, blocked, insufficient_context.\n- issues must be an array (empty array allowed)."
    ))
}

fn openai_default_model() -> String {
    std::env::var("SLOPOSCOPE_OPENAI_MODEL")
        .ok()
        .or_else(|| std::env::var("OPENAI_MODEL").ok())
        .unwrap_or_else(|| "gpt-4.1-mini".to_string())
}

fn nim_default_model() -> String {
    std::env::var("SLOPOSCOPE_NVIDIA_NIM_MODEL")
        .ok()
        .or_else(|| std::env::var("NVIDIA_NIM_MODEL").ok())
        .unwrap_or_else(|| "meta/llama3-8b-instruct".to_string())
}

fn openai_base_url() -> String {
    std::env::var("SLOPOSCOPE_OPENAI_BASE_URL")
        .ok()
        .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string())
}

fn nim_base_url() -> String {
    std::env::var("SLOPOSCOPE_NVIDIA_NIM_BASE_URL")
        .ok()
        .or_else(|| std::env::var("NVIDIA_NIM_BASE_URL").ok())
        .or_else(|| std::env::var("NVIDIA_BASE_URL").ok())
        .unwrap_or_else(|| "https://integrate.api.nvidia.com/v1".to_string())
}

fn openai_api_key() -> Result<String, String> {
    std::env::var("SLOPOSCOPE_OPENAI_API_KEY")
        .ok()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "OpenAI API key is missing. Set SLOPOSCOPE_OPENAI_API_KEY or OPENAI_API_KEY."
                .to_string()
        })
}

fn nim_api_key() -> Result<String, String> {
    std::env::var("SLOPOSCOPE_NVIDIA_NIM_API_KEY")
        .ok()
        .or_else(|| std::env::var("NVIDIA_NIM_API_KEY").ok())
        .or_else(|| std::env::var("NVIDIA_API_KEY").ok())
        .or_else(|| std::env::var("NGC_API_KEY").ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "NVIDIA NIM API key is missing. Set SLOPOSCOPE_NVIDIA_NIM_API_KEY, NVIDIA_NIM_API_KEY, NVIDIA_API_KEY, or NGC_API_KEY.".to_string()
        })
}

fn build_openai_chat_request(model: &str, prompt: &str) -> OpenAiChatCompletionsRequest {
    OpenAiChatCompletionsRequest {
        model: model.to_string(),
        temperature: 0.0,
        messages: vec![OpenAiChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }],
    }
}

fn normalize_openai_content(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    out.push(text.to_string());
                } else if let Some(text) = item
                    .get("content")
                    .and_then(|v| v.get("text"))
                    .and_then(|v| v.as_str())
                {
                    out.push(text.to_string());
                }
            }
            out.join("\n")
        }
        _ => content.to_string(),
    }
}

fn extract_first_json_object(raw: &str) -> Result<&str, String> {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix("```") {
        let mut parts = inner.splitn(2, '\n');
        let maybe_lang = parts.next().unwrap_or_default().trim();
        let rest = parts.next().unwrap_or_default();
        let candidate = if maybe_lang.starts_with('{') {
            inner
        } else {
            rest
        };
        let candidate = candidate.trim();
        if let Some(unfenced) = candidate.strip_suffix("```") {
            let body = unfenced.trim();
            if body.starts_with('{') && body.ends_with('}') {
                return Ok(body);
            }
        }
    }

    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Ok(trimmed);
    }

    let start = trimmed
        .find('{')
        .ok_or_else(|| "OpenAI response does not contain a JSON object start ('{').".to_string())?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| "OpenAI response does not contain a JSON object end ('}').".to_string())?;
    if end <= start {
        return Err("OpenAI response JSON bounds are invalid.".to_string());
    }
    Ok(&trimmed[start..=end])
}

fn parse_watchdog_status(value: &str) -> Result<watchdog_types::WatchdogStatus, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "on_track" => Ok(watchdog_types::WatchdogStatus::OnTrack),
        "drift" => Ok(watchdog_types::WatchdogStatus::Drift),
        "blocked" => Ok(watchdog_types::WatchdogStatus::Blocked),
        "insufficient_context" => Ok(watchdog_types::WatchdogStatus::InsufficientContext),
        _ => Err(format!(
            "Unsupported status '{}' (expected on_track|drift|blocked|insufficient_context).",
            value
        )),
    }
}

fn parse_watchdog_issue_kind(value: &str) -> Result<watchdog_types::WatchdogIssueKind, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "drift" => Ok(watchdog_types::WatchdogIssueKind::Drift),
        "instruction_violation" => Ok(watchdog_types::WatchdogIssueKind::InstructionViolation),
        "missing_validation" => Ok(watchdog_types::WatchdogIssueKind::MissingValidation),
        "cost_risk" => Ok(watchdog_types::WatchdogIssueKind::CostRisk),
        "unclear_mapping" => Ok(watchdog_types::WatchdogIssueKind::UnclearMapping),
        _ => Err(format!(
            "Unsupported issue kind '{}' (expected drift|instruction_violation|missing_validation|cost_risk|unclear_mapping).",
            value
        )),
    }
}

fn parse_watchdog_severity(value: &str) -> Result<watchdog_types::WatchdogSeverity, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "info" => Ok(watchdog_types::WatchdogSeverity::Info),
        "warning" | "warn" => Ok(watchdog_types::WatchdogSeverity::Warning),
        "critical" | "error" => Ok(watchdog_types::WatchdogSeverity::Critical),
        _ => Err(format!(
            "Unsupported severity '{}' (expected info|warning|critical).",
            value
        )),
    }
}

pub fn parse_openai_watchdog_verdict(raw: &str) -> Result<watchdog_types::WatchdogVerdict, String> {
    let json = extract_first_json_object(raw)?;
    let payload: OpenAiVerdictPayload = serde_json::from_str(json)
        .map_err(|err| format!("Failed to parse OpenAI verdict JSON: {err}"))?;

    let status = parse_watchdog_status(&payload.status)?;
    let confidence = payload.confidence.clamp(0.0, 1.0);
    let issues = payload
        .issues
        .into_iter()
        .map(|item| {
            Ok(watchdog_types::WatchdogVerdictIssue {
                kind: parse_watchdog_issue_kind(&item.kind)?,
                severity: parse_watchdog_severity(&item.severity)?,
                title: item.title.trim().to_string(),
                why: item.why.trim().to_string(),
                evidence: item
                    .evidence
                    .into_iter()
                    .map(|entry| entry.trim().to_string())
                    .filter(|entry| !entry.is_empty())
                    .collect(),
                recommended_next_step: item.recommended_next_step.trim().to_string(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    if payload.summary.trim().is_empty() {
        return Err("OpenAI verdict summary is empty.".to_string());
    }

    Ok(watchdog_types::WatchdogVerdict {
        status,
        confidence,
        summary: payload.summary.trim().to_string(),
        issues,
    })
}

pub fn run_openai_compatible_chat_completion(
    provider_name: &str,
    api_key: &str,
    base_url: &str,
    default_model: &str,
    prompt: &str,
    model_override: Option<&str>,
    timeout_seconds: u64,
) -> Result<(ConductorOpenAiRunConfig, String), String> {
    let model = model_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| default_model.to_string());
    let timeout = timeout_seconds.clamp(10, 120);
    let request = build_openai_chat_request(&model, prompt);

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .build()
        .map_err(|err| format!("Failed to build {provider_name} HTTP client: {err}"))?;
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let response = client
        .post(&endpoint)
        .bearer_auth(api_key)
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .map_err(|err| format!("{provider_name} request failed: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|err| format!("Failed to read {provider_name} response body: {err}"))?;

    if !status.is_success() {
        let compact_body = body.chars().take(1_200).collect::<String>();
        return Err(format!(
            "{provider_name} request failed with status {}: {}",
            status.as_u16(),
            compact_body
        ));
    }

    let parsed: OpenAiChatCompletionsResponse = serde_json::from_str(&body)
        .map_err(|err| format!("Failed to parse {provider_name} response JSON: {err}"))?;
    let content = parsed
        .choices
        .first()
        .map(|choice| normalize_openai_content(&choice.message.content))
        .ok_or_else(|| format!("{provider_name} response has no choices."))?;
    if content.trim().is_empty() {
        return Err(format!("{provider_name} response content is empty."));
    }

    Ok((
        ConductorOpenAiRunConfig {
            model,
            timeout_seconds: timeout,
            base_url: base_url.to_string(),
        },
        content,
    ))
}

pub fn run_openai_watchdog_check(
    prompt: &str,
    model_override: Option<&str>,
    timeout_seconds: u64,
) -> Result<ConductorOpenAiRunResult, String> {
    let api_key = openai_api_key()?;
    let base_url = openai_base_url();
    let (config, content) = run_openai_compatible_chat_completion(
        "OpenAI",
        &api_key,
        &base_url,
        &openai_default_model(),
        prompt,
        model_override,
        timeout_seconds,
    )?;
    let verdict = parse_openai_watchdog_verdict(&content)?;

    Ok(ConductorOpenAiRunResult {
        config,
        output_text: content,
        verdict,
    })
}

pub fn run_nim_watchdog_check(
    prompt: &str,
    model_override: Option<&str>,
    timeout_seconds: u64,
) -> Result<ConductorOpenAiRunResult, String> {
    let api_key = nim_api_key()?;
    let base_url = nim_base_url();
    let (config, content) = run_openai_compatible_chat_completion(
        "NVIDIA NIM",
        &api_key,
        &base_url,
        &nim_default_model(),
        prompt,
        model_override,
        timeout_seconds,
    )?;
    let verdict = parse_openai_watchdog_verdict(&content)?;

    Ok(ConductorOpenAiRunResult {
        config,
        output_text: content,
        verdict,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorSchedulerConfig {
    pub max_concurrent_runs: usize,
    pub retry_base_seconds: u64,
    pub retry_max_seconds: u64,
}

impl Default for ConductorSchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_runs: 2,
            retry_base_seconds: 15,
            retry_max_seconds: 300,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorRunnerUpdate {
    pub task_id: String,
    pub event: ConductorRunnerEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorReconciliationTickResult {
    pub started_task_ids: Vec<String>,
    pub reclaimed_retry_task_ids: Vec<String>,
    pub scheduled_retry_task_ids: Vec<String>,
    pub reconciled_task_ids: Vec<String>,
}

fn unix_label(seconds: u64) -> String {
    format!("unix:{seconds}")
}

fn parse_unix_label_seconds(label: &str) -> Option<u64> {
    label.strip_prefix("unix:")?.parse::<u64>().ok()
}

fn validate_scheduler_config(config: &ConductorSchedulerConfig) -> Result<(), String> {
    if config.max_concurrent_runs == 0 {
        return Err("max_concurrent_runs must be > 0".to_string());
    }
    if config.retry_base_seconds == 0 {
        return Err("retry_base_seconds must be > 0".to_string());
    }
    if config.retry_max_seconds < config.retry_base_seconds {
        return Err("retry_max_seconds must be >= retry_base_seconds".to_string());
    }
    Ok(())
}

fn retry_backoff_seconds(task: &ConductorTask, config: &ConductorSchedulerConfig) -> u64 {
    let shift = task.retry_count.min(16);
    let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
    let delay = config.retry_base_seconds.saturating_mul(factor);
    delay.clamp(config.retry_base_seconds, config.retry_max_seconds)
}

pub fn reconcile_conductor_tick(
    tasks: &mut [ConductorTask],
    updates: &[ConductorRunnerUpdate],
    now_unix_seconds: u64,
    config: &ConductorSchedulerConfig,
) -> Result<ConductorReconciliationTickResult, String> {
    validate_scheduler_config(config)?;
    let now_label = unix_label(now_unix_seconds);

    let mut task_index_by_id: HashMap<String, usize> = HashMap::new();
    for (idx, task) in tasks.iter().enumerate() {
        if task_index_by_id.insert(task.task_id.clone(), idx).is_some() {
            return Err(format!(
                "duplicate task_id in reconciliation tick: {}",
                task.task_id
            ));
        }
    }

    let mut result = ConductorReconciliationTickResult::default();

    for update in updates {
        let Some(&idx) = task_index_by_id.get(update.task_id.as_str()) else {
            continue;
        };
        let task = &mut tasks[idx];
        if task.is_terminal() {
            continue;
        }

        match &update.event {
            ConductorRunnerEvent::Started { started_at, .. } => {
                if task.state == ConductorTaskState::Claimed {
                    task.apply(ConductorTransition::StartRunning {
                        started_at: started_at.clone(),
                    })
                    .map_err(|err| {
                        format!(
                            "failed to apply Started event for task {}: {}",
                            err.task_id, err.why
                        )
                    })?;
                    result.started_task_ids.push(task.task_id.clone());
                }
            }
            ConductorRunnerEvent::Progress { .. } => {}
            ConductorRunnerEvent::Completed { result_summary, .. } => {
                task.apply(ConductorTransition::Reconcile {
                    reconciled_at: now_label.clone(),
                    outcome: ConductorReconciledOutcome::Succeeded,
                    note: result_summary.clone(),
                })
                .map_err(|err| {
                    format!(
                        "failed to reconcile completed task {}: {}",
                        err.task_id, err.why
                    )
                })?;
                result.reconciled_task_ids.push(task.task_id.clone());
            }
            ConductorRunnerEvent::Failed { error, .. } => {
                if task.state != ConductorTaskState::Running {
                    task.apply(ConductorTransition::Reconcile {
                        reconciled_at: now_label.clone(),
                        outcome: ConductorReconciledOutcome::Failed,
                        note: Some(error.clone()),
                    })
                    .map_err(|err| {
                        format!(
                            "failed to reconcile failed task {}: {}",
                            err.task_id, err.why
                        )
                    })?;
                    result.reconciled_task_ids.push(task.task_id.clone());
                    continue;
                }

                if task.retry_count >= task.max_retries {
                    task.apply(ConductorTransition::Reconcile {
                        reconciled_at: now_label.clone(),
                        outcome: ConductorReconciledOutcome::Exhausted,
                        note: Some(error.clone()),
                    })
                    .map_err(|err| {
                        format!(
                            "failed to reconcile exhausted task {}: {}",
                            err.task_id, err.why
                        )
                    })?;
                    result.reconciled_task_ids.push(task.task_id.clone());
                    continue;
                }

                let delay = retry_backoff_seconds(task, config);
                let retry_at = unix_label(now_unix_seconds.saturating_add(delay));
                task.apply(ConductorTransition::ScheduleRetry {
                    retry_at,
                    reason: error.clone(),
                })
                .map_err(|err| {
                    format!(
                        "failed to schedule retry for task {}: {}",
                        err.task_id, err.why
                    )
                })?;
                result.scheduled_retry_task_ids.push(task.task_id.clone());
            }
        }
    }

    let mut retry_ready_indices: Vec<usize> = tasks
        .iter()
        .enumerate()
        .filter_map(|(idx, task)| {
            if task.state != ConductorTaskState::RetryScheduled {
                return None;
            }
            let retry_at_secs = task
                .retry_at
                .as_deref()
                .and_then(parse_unix_label_seconds)
                .unwrap_or(u64::MAX);
            if retry_at_secs <= now_unix_seconds {
                Some(idx)
            } else {
                None
            }
        })
        .collect();
    retry_ready_indices.sort_by_key(|idx| tasks[*idx].task_id.clone());
    for idx in retry_ready_indices {
        let task = &mut tasks[idx];
        task.apply(ConductorTransition::ReclaimRetry {
            claimed_at: now_label.clone(),
        })
        .map_err(|err| {
            format!(
                "failed to reclaim retry for task {}: {}",
                err.task_id, err.why
            )
        })?;
        result.reclaimed_retry_task_ids.push(task.task_id.clone());
    }

    let running_count = tasks
        .iter()
        .filter(|task| task.state == ConductorTaskState::Running)
        .count();
    let start_budget = config.max_concurrent_runs.saturating_sub(running_count);
    if start_budget == 0 {
        return Ok(result);
    }

    let mut claimable_indices: Vec<usize> = tasks
        .iter()
        .enumerate()
        .filter_map(|(idx, task)| {
            if task.state == ConductorTaskState::Claimed {
                Some(idx)
            } else {
                None
            }
        })
        .collect();
    claimable_indices.sort_by_key(|idx| tasks[*idx].task_id.clone());
    for idx in claimable_indices.into_iter().take(start_budget) {
        let task = &mut tasks[idx];
        task.apply(ConductorTransition::StartRunning {
            started_at: now_label.clone(),
        })
        .map_err(|err| format!("failed to start claimed task {}: {}", err.task_id, err.why))?;
        result.started_task_ids.push(task.task_id.clone());
    }

    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorRunTraceEntry {
    pub at: String,
    pub event: String,
    pub task_id: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConductorProviderFailureStats {
    pub total: u64,
    pub openai: u64,
    pub nim: u64,
    pub coding_agent_app_server: u64,
    pub strict_mode_blocked: u64,
    pub fallback_used: u64,
    pub last_failure_at: Option<String>,
    pub last_failure_provider: Option<ConductorRunnerProvider>,
    pub last_failure_message: Option<String>,
}

fn trim_failure_message(message: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in message.chars().take(max_chars) {
        out.push(ch);
    }
    out
}

pub fn record_provider_failure(
    runtime: &mut ConductorRuntimeState,
    provider: ConductorRunnerProvider,
    strict_mode: bool,
    message: &str,
    now_unix_seconds: u64,
) {
    runtime.provider_failures.total = runtime.provider_failures.total.saturating_add(1);
    match provider {
        ConductorRunnerProvider::OpenAi => {
            runtime.provider_failures.openai = runtime.provider_failures.openai.saturating_add(1)
        }
        ConductorRunnerProvider::Nim => {
            runtime.provider_failures.nim = runtime.provider_failures.nim.saturating_add(1)
        }
        ConductorRunnerProvider::CodingAgentAppServer => {
            runtime.provider_failures.coding_agent_app_server = runtime
                .provider_failures
                .coding_agent_app_server
                .saturating_add(1)
        }
    }
    if strict_mode {
        runtime.provider_failures.strict_mode_blocked = runtime
            .provider_failures
            .strict_mode_blocked
            .saturating_add(1);
    } else {
        runtime.provider_failures.fallback_used =
            runtime.provider_failures.fallback_used.saturating_add(1);
    }
    runtime.provider_failures.last_failure_at = Some(unix_label(now_unix_seconds));
    runtime.provider_failures.last_failure_provider = Some(provider.clone());
    runtime.provider_failures.last_failure_message = Some(trim_failure_message(message, 600));

    push_trace(
        runtime,
        ConductorRunTraceEntry {
            at: unix_label(now_unix_seconds),
            event: "provider_failure".to_string(),
            task_id: None,
            detail: Some(format!(
                "provider={:?} strict_mode={} message={}",
                provider,
                strict_mode,
                trim_failure_message(message, 160)
            )),
        },
    );
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConductorRuntimeState {
    pub config: ConductorSchedulerConfig,
    #[serde(default)]
    pub tasks: Vec<ConductorTask>,
    #[serde(default)]
    pub issue_runs: Vec<IssueRun>,
    #[serde(default)]
    pub traces: Vec<ConductorRunTraceEntry>,
    #[serde(default)]
    pub provider_failures: ConductorProviderFailureStats,
    pub last_tick_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorRefreshResponse {
    pub tick: ConductorReconciliationTickResult,
    pub status: ConductorStatusView,
}

/// Lightweight view of an IssueRun for the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueRunView {
    pub task_id: String,
    pub state: ConductorTaskState,
    pub run_attempt: u32,
    pub active_session_id: Option<String>,
    pub workflow_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorStatusView {
    pub generated_at: String,
    pub config: ConductorSchedulerConfig,
    pub tasks_total: usize,
    pub tasks_claimed: usize,
    pub tasks_running: usize,
    pub tasks_retry_scheduled: usize,
    pub tasks_reconciled: usize,
    pub traces_total: usize,
    pub provider_failures: ConductorProviderFailureStats,
    pub last_tick_at: Option<String>,
    pub recent_traces: Vec<ConductorRunTraceEntry>,
    pub issue_runs: Vec<IssueRunView>,
}

const CONDUCTOR_TRACE_LIMIT: usize = 500;

fn push_trace(runtime: &mut ConductorRuntimeState, entry: ConductorRunTraceEntry) {
    runtime.traces.push(entry);
    if runtime.traces.len() > CONDUCTOR_TRACE_LIMIT {
        let trim = runtime.traces.len() - CONDUCTOR_TRACE_LIMIT;
        runtime.traces.drain(0..trim);
    }
}

pub fn enqueue_task(
    runtime: &mut ConductorRuntimeState,
    task_id: &str,
    now_unix_seconds: u64,
    max_retries: Option<u32>,
) -> Result<ConductorTask, String> {
    let normalized = normalize_task_id(task_id)?;
    if runtime.tasks.iter().any(|task| task.task_id == normalized) {
        return Err(format!("task already exists: {normalized}"));
    }
    let task = ConductorTask::new_claimed(
        normalized.clone(),
        unix_label(now_unix_seconds),
        max_retries.unwrap_or(2),
    );
    runtime.tasks.push(task.clone());
    runtime.tasks.sort_by_key(|item| item.task_id.clone());
    push_trace(
        runtime,
        ConductorRunTraceEntry {
            at: unix_label(now_unix_seconds),
            event: "task_enqueued".to_string(),
            task_id: Some(normalized),
            detail: None,
        },
    );
    Ok(task)
}

pub fn build_status_view(
    runtime: &ConductorRuntimeState,
    now_unix_seconds: u64,
    trace_limit: usize,
) -> ConductorStatusView {
    let mut tasks_claimed = 0usize;
    let mut tasks_running = 0usize;
    let mut tasks_retry_scheduled = 0usize;
    let mut tasks_reconciled = 0usize;
    for task in &runtime.tasks {
        match task.state {
            ConductorTaskState::Claimed => tasks_claimed += 1,
            ConductorTaskState::Running => tasks_running += 1,
            ConductorTaskState::RetryScheduled => tasks_retry_scheduled += 1,
            ConductorTaskState::Reconciled => tasks_reconciled += 1,
        }
    }

    let take = trace_limit.clamp(1, 200);
    let recent_traces = runtime
        .traces
        .iter()
        .rev()
        .take(take)
        .cloned()
        .collect::<Vec<_>>();

    let issue_runs = runtime
        .issue_runs
        .iter()
        .map(|ir| IssueRunView {
            task_id: ir.task.task_id.clone(),
            state: ir.task.state.clone(),
            run_attempt: ir.task.run_attempt,
            active_session_id: ir.active_session_id.clone(),
            workflow_id: ir.context.workflow_id.clone(),
        })
        .collect::<Vec<_>>();

    ConductorStatusView {
        generated_at: unix_label(now_unix_seconds),
        config: runtime.config.clone(),
        tasks_total: runtime.tasks.len(),
        tasks_claimed,
        tasks_running,
        tasks_retry_scheduled,
        tasks_reconciled,
        traces_total: runtime.traces.len(),
        provider_failures: runtime.provider_failures.clone(),
        last_tick_at: runtime.last_tick_at.clone(),
        recent_traces,
        issue_runs,
    }
}

pub fn refresh_runtime(
    runtime: &mut ConductorRuntimeState,
    updates: &[ConductorRunnerUpdate],
    now_unix_seconds: u64,
) -> Result<ConductorRefreshResponse, String> {
    let now_label = unix_label(now_unix_seconds);
    push_trace(
        runtime,
        ConductorRunTraceEntry {
            at: now_label.clone(),
            event: "tick_start".to_string(),
            task_id: None,
            detail: Some(format!("updates={}", updates.len())),
        },
    );

    for update in updates {
        push_trace(
            runtime,
            ConductorRunTraceEntry {
                at: now_label.clone(),
                event: "runner_update".to_string(),
                task_id: Some(update.task_id.clone()),
                detail: Some(format!("{:?}", update.event)),
            },
        );
    }

    let tick = reconcile_conductor_tick(
        &mut runtime.tasks,
        updates,
        now_unix_seconds,
        &runtime.config,
    )?;
    runtime.last_tick_at = Some(now_label.clone());

    for task_id in &tick.scheduled_retry_task_ids {
        push_trace(
            runtime,
            ConductorRunTraceEntry {
                at: now_label.clone(),
                event: "retry_scheduled".to_string(),
                task_id: Some(task_id.clone()),
                detail: None,
            },
        );
    }
    for task_id in &tick.reclaimed_retry_task_ids {
        push_trace(
            runtime,
            ConductorRunTraceEntry {
                at: now_label.clone(),
                event: "retry_reclaimed".to_string(),
                task_id: Some(task_id.clone()),
                detail: None,
            },
        );
    }
    for task_id in &tick.started_task_ids {
        push_trace(
            runtime,
            ConductorRunTraceEntry {
                at: now_label.clone(),
                event: "task_started".to_string(),
                task_id: Some(task_id.clone()),
                detail: None,
            },
        );
    }
    for task_id in &tick.reconciled_task_ids {
        push_trace(
            runtime,
            ConductorRunTraceEntry {
                at: now_label.clone(),
                event: "task_reconciled".to_string(),
                task_id: Some(task_id.clone()),
                detail: None,
            },
        );
    }

    push_trace(
        runtime,
        ConductorRunTraceEntry {
            at: now_label.clone(),
            event: "tick_done".to_string(),
            task_id: None,
            detail: Some(format!(
                "started={} reclaimed={} retries={} reconciled={}",
                tick.started_task_ids.len(),
                tick.reclaimed_retry_task_ids.len(),
                tick.scheduled_retry_task_ids.len(),
                tick.reconciled_task_ids.len()
            )),
        },
    );

    Ok(ConductorRefreshResponse {
        tick,
        status: build_status_view(runtime, now_unix_seconds, 30),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConductorTask {
    pub task_id: String,
    pub state: ConductorTaskState,
    pub claimed_at: Option<String>,
    pub started_at: Option<String>,
    pub retry_at: Option<String>,
    pub reconciled_at: Option<String>,
    pub run_attempt: u32,
    pub retry_count: u32,
    pub max_retries: u32,
    pub last_error: Option<String>,
    pub reconciled_outcome: Option<ConductorReconciledOutcome>,
    pub reconciled_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConductorTransition {
    StartRunning {
        started_at: String,
    },
    ScheduleRetry {
        retry_at: String,
        reason: String,
    },
    ReclaimRetry {
        claimed_at: String,
    },
    Reconcile {
        reconciled_at: String,
        outcome: ConductorReconciledOutcome,
        note: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConductorTransitionError {
    pub task_id: String,
    pub from: ConductorTaskState,
    pub transition: ConductorTransition,
    pub why: String,
}

impl ConductorTask {
    pub fn new_claimed(
        task_id: impl Into<String>,
        claimed_at: impl Into<String>,
        max_retries: u32,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            state: ConductorTaskState::Claimed,
            claimed_at: Some(claimed_at.into()),
            started_at: None,
            retry_at: None,
            reconciled_at: None,
            run_attempt: 0,
            retry_count: 0,
            max_retries,
            last_error: None,
            reconciled_outcome: None,
            reconciled_note: None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state, ConductorTaskState::Reconciled)
    }

    pub fn apply(
        &mut self,
        transition: ConductorTransition,
    ) -> Result<(), ConductorTransitionError> {
        let from = self.state.clone();
        match (&self.state, &transition) {
            (ConductorTaskState::Claimed, ConductorTransition::StartRunning { started_at }) => {
                self.state = ConductorTaskState::Running;
                self.started_at = Some(started_at.clone());
                self.retry_at = None;
                self.reconciled_at = None;
                self.reconciled_outcome = None;
                self.reconciled_note = None;
                self.run_attempt = self.run_attempt.saturating_add(1);
                Ok(())
            }
            (
                ConductorTaskState::Running,
                ConductorTransition::ScheduleRetry { retry_at, reason },
            ) => {
                if self.retry_count >= self.max_retries {
                    return Err(ConductorTransitionError {
                        task_id: self.task_id.clone(),
                        from,
                        transition,
                        why: format!(
                            "retry_count={} reached max_retries={}",
                            self.retry_count, self.max_retries
                        ),
                    });
                }
                self.state = ConductorTaskState::RetryScheduled;
                self.retry_at = Some(retry_at.clone());
                self.started_at = None;
                self.last_error = Some(reason.clone());
                self.retry_count = self.retry_count.saturating_add(1);
                Ok(())
            }
            (
                ConductorTaskState::RetryScheduled,
                ConductorTransition::ReclaimRetry { claimed_at },
            ) => {
                self.state = ConductorTaskState::Claimed;
                self.claimed_at = Some(claimed_at.clone());
                self.retry_at = None;
                Ok(())
            }
            (
                ConductorTaskState::Claimed,
                ConductorTransition::Reconcile {
                    reconciled_at,
                    outcome,
                    note,
                },
            )
            | (
                ConductorTaskState::Running,
                ConductorTransition::Reconcile {
                    reconciled_at,
                    outcome,
                    note,
                },
            )
            | (
                ConductorTaskState::RetryScheduled,
                ConductorTransition::Reconcile {
                    reconciled_at,
                    outcome,
                    note,
                },
            ) => {
                self.state = ConductorTaskState::Reconciled;
                self.reconciled_at = Some(reconciled_at.clone());
                self.reconciled_outcome = Some(outcome.clone());
                self.reconciled_note = note.clone();
                self.retry_at = None;
                self.started_at = None;
                Ok(())
            }
            _ => Err(ConductorTransitionError {
                task_id: self.task_id.clone(),
                from,
                transition,
                why: "invalid transition for current task state".to_string(),
            }),
        }
    }
}

// ── Runtime model ────────────────────────────────────────────────────────────

/// Issue + workflow context pinned to a specific task execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskContext {
    pub task_id: String,
    pub workflow_id: String,
    pub prompt: String,
    pub max_retries: u32,
    pub timeout_seconds: u64,
    pub issue_id: Option<String>,
    pub issue_title: Option<String>,
}

impl TaskContext {
    pub fn from_contract(
        task_id: &str,
        contract: &ConductorWorkflowContract,
        prompt: String,
    ) -> Self {
        Self {
            task_id: task_id.to_string(),
            workflow_id: contract.front_matter.workflow_id.clone(),
            prompt,
            max_retries: contract.front_matter.max_retries,
            timeout_seconds: contract.front_matter.timeout_seconds,
            issue_id: None,
            issue_title: None,
        }
    }

    pub fn with_issue(mut self, id: impl Into<String>, title: impl Into<String>) -> Self {
        self.issue_id = Some(id.into());
        self.issue_title = Some(title.into());
        self
    }
}

/// A single worker invocation — one run attempt for a TaskContext.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRun {
    pub session_id: String,
    pub task_id: String,
    pub run_attempt: u32,
    pub workspace_path: String,
    pub launched_at: String,
}

/// Normalised event from any runner backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    Started { at: String },
    Progress { message: String },
    Completed { summary: Option<String> },
    Failed { error: String },
}

/// An artifact produced by a worker run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArtifact {
    pub kind: String,
    pub content: String,
    pub created_at: String,
}

/// A task paired with its execution context and active session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueRun {
    pub task: ConductorTask,
    pub context: TaskContext,
    pub active_session_id: Option<String>,
    pub artifacts: Vec<RunArtifact>,
}

impl IssueRun {
    pub fn new(task: ConductorTask, context: TaskContext) -> Self {
        Self {
            task,
            context,
            active_session_id: None,
            artifacts: vec![],
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.task.is_terminal()
    }

    pub fn attach_session(&mut self, session_id: impl Into<String>) {
        self.active_session_id = Some(session_id.into());
    }

    pub fn push_artifact(&mut self, artifact: RunArtifact) {
        self.artifacts.push(artifact);
    }
}

/// Abstraction over different runner backends (app-server, OpenAI, future).
pub trait RunnerAdapter: Send + Sync {
    /// Launch a new run. Returns the session id on success.
    fn launch(&self, run: &WorkerRun, prompt: &str) -> Result<String, String>;

    /// Convert a raw runner event payload to the normalised RunEvent.
    fn parse_event(&self, raw: &str) -> Result<RunEvent, String>;
}

fn app_server_base_url() -> String {
    std::env::var("SLOPOSCOPE_APP_SERVER_URL")
        .unwrap_or_else(|_| "http://localhost:52340".to_string())
}

/// HTTP adapter that launches runs via the coding-agent app-server protocol.
pub struct HttpRunnerAdapter {
    pub base_url: String,
    pub timeout_seconds: u64,
    pub workflow_id: String,
}

impl HttpRunnerAdapter {
    pub fn from_contract(contract: &ConductorWorkflowContract) -> Self {
        Self {
            base_url: app_server_base_url(),
            timeout_seconds: contract.front_matter.timeout_seconds,
            workflow_id: contract.front_matter.workflow_id.clone(),
        }
    }
}

impl RunnerAdapter for HttpRunnerAdapter {
    fn launch(&self, run: &WorkerRun, prompt: &str) -> Result<String, String> {
        let payload = ConductorRunnerLaunchPayload {
            protocol_version: 1,
            provider: ConductorRunnerProvider::CodingAgentAppServer,
            action: "start_session".to_string(),
            session_id: run.session_id.clone(),
            task_id: run.task_id.clone(),
            run_attempt: run.run_attempt,
            workflow_id: self.workflow_id.clone(),
            workspace_path: run.workspace_path.clone(),
            prompt: prompt.to_string(),
        };
        let timeout = self.timeout_seconds.clamp(10, 3_600);
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(timeout))
            .build()
            .map_err(|err| format!("Failed to build HTTP client: {err}"))?;
        let url = format!("{}/run", self.base_url.trim_end_matches('/'));
        let response = client
            .post(&url)
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .map_err(|err| format!("App-server launch request failed: {err}"))?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            let compact = body.chars().take(800).collect::<String>();
            return Err(format!(
                "App-server launch failed with status {status}: {compact}"
            ));
        }
        Ok(run.session_id.clone())
    }

    fn parse_event(&self, raw: &str) -> Result<RunEvent, String> {
        let event = parse_app_server_event(raw)?;
        Ok(match event {
            ConductorRunnerEvent::Started { started_at, .. } => {
                RunEvent::Started { at: started_at }
            }
            ConductorRunnerEvent::Progress { message, .. } => RunEvent::Progress { message },
            ConductorRunnerEvent::Completed { result_summary, .. } => RunEvent::Completed {
                summary: result_summary,
            },
            ConductorRunnerEvent::Failed { error, .. } => RunEvent::Failed { error },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    /// No-op adapter used as a test stub.
    struct NullRunnerAdapter;

    impl RunnerAdapter for NullRunnerAdapter {
        fn launch(&self, run: &WorkerRun, _prompt: &str) -> Result<String, String> {
            Ok(run.session_id.clone())
        }

        fn parse_event(&self, _raw: &str) -> Result<RunEvent, String> {
            Ok(RunEvent::Completed { summary: None })
        }
    }

    #[test]
    fn transitions_claimed_to_running_to_reconciled() {
        let mut task = ConductorTask::new_claimed("task-1", "unix:1", 2);
        task.apply(ConductorTransition::StartRunning {
            started_at: "unix:2".to_string(),
        })
        .expect("claimed -> running");
        assert_eq!(task.state, ConductorTaskState::Running);
        assert_eq!(task.run_attempt, 1);

        task.apply(ConductorTransition::Reconcile {
            reconciled_at: "unix:3".to_string(),
            outcome: ConductorReconciledOutcome::Succeeded,
            note: Some("done".to_string()),
        })
        .expect("running -> reconciled");
        assert_eq!(task.state, ConductorTaskState::Reconciled);
        assert!(task.is_terminal());
        assert_eq!(
            task.reconciled_outcome,
            Some(ConductorReconciledOutcome::Succeeded)
        );
    }

    #[test]
    fn transitions_through_retry_cycle() {
        let mut task = ConductorTask::new_claimed("task-2", "unix:1", 2);
        task.apply(ConductorTransition::StartRunning {
            started_at: "unix:2".to_string(),
        })
        .expect("claimed -> running");

        task.apply(ConductorTransition::ScheduleRetry {
            retry_at: "unix:10".to_string(),
            reason: "runner timeout".to_string(),
        })
        .expect("running -> retry");
        assert_eq!(task.state, ConductorTaskState::RetryScheduled);
        assert_eq!(task.retry_count, 1);
        assert_eq!(task.last_error.as_deref(), Some("runner timeout"));

        task.apply(ConductorTransition::ReclaimRetry {
            claimed_at: "unix:11".to_string(),
        })
        .expect("retry -> claimed");
        assert_eq!(task.state, ConductorTaskState::Claimed);

        task.apply(ConductorTransition::StartRunning {
            started_at: "unix:12".to_string(),
        })
        .expect("claimed -> running again");
        assert_eq!(task.run_attempt, 2);
    }

    #[test]
    fn blocks_invalid_transition_for_state() {
        let mut task = ConductorTask::new_claimed("task-3", "unix:1", 1);
        let err = task
            .apply(ConductorTransition::ScheduleRetry {
                retry_at: "unix:2".to_string(),
                reason: "invalid".to_string(),
            })
            .expect_err("claimed cannot go to retry directly");
        assert_eq!(err.from, ConductorTaskState::Claimed);
    }

    #[test]
    fn blocks_retry_when_max_retries_reached() {
        let mut task = ConductorTask::new_claimed("task-4", "unix:1", 0);
        task.apply(ConductorTransition::StartRunning {
            started_at: "unix:2".to_string(),
        })
        .expect("claimed -> running");

        let err = task
            .apply(ConductorTransition::ScheduleRetry {
                retry_at: "unix:5".to_string(),
                reason: "still failing".to_string(),
            })
            .expect_err("retry should be blocked");
        assert!(err.why.contains("max_retries"));
        assert_eq!(task.state, ConductorTaskState::Running);
    }

    #[test]
    fn blocks_transitions_after_reconciled() {
        let mut task = ConductorTask::new_claimed("task-5", "unix:1", 2);
        task.apply(ConductorTransition::Reconcile {
            reconciled_at: "unix:2".to_string(),
            outcome: ConductorReconciledOutcome::Cancelled,
            note: None,
        })
        .expect("claimed -> reconciled");

        let err = task
            .apply(ConductorTransition::StartRunning {
                started_at: "unix:3".to_string(),
            })
            .expect_err("reconciled is terminal");
        assert_eq!(err.from, ConductorTaskState::Reconciled);
    }

    #[test]
    fn loads_workflow_contract_with_front_matter_and_prompt_template() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(
            dir.path().join("WORKFLOW.md"),
            r#"---
schemaVersion: 1
workflowId: watchdog_focus
provider: openai
title: Watchdog focus review
summary: Validate active issue alignment and validation evidence
maxRetries: 3
timeoutSeconds: 600
requiredChecks:
  - build
  - tests
---
You are the watchdog.
Use the active issue context first.
Return a strict JSON verdict.
"#,
        )
        .expect("write workflow");

        let loaded = load_workflow_contract(dir.path());
        assert!(loaded.exists);
        assert!(loaded.loaded);
        let contract = loaded.contract.expect("contract");
        assert_eq!(contract.front_matter.workflow_id, "watchdog_focus");
        assert_eq!(
            contract.front_matter.provider,
            ConductorRunnerProvider::OpenAi
        );
        assert_eq!(contract.front_matter.max_retries, 3);
        assert_eq!(contract.front_matter.timeout_seconds, 600);
        assert_eq!(
            contract.front_matter.required_checks,
            vec!["build", "tests"]
        );
        assert!(contract
            .prompt_template
            .contains("Use the active issue context first."));
    }

    #[test]
    fn workflow_contract_defaults_provider_when_not_set() {
        let parse = parse_workflow_contract(
            r#"---
schemaVersion: 1
workflowId: watchdog_focus
title: Watchdog focus review
---
Prompt body
"#,
        )
        .expect("workflow should parse");
        assert_eq!(
            parse.front_matter.provider,
            ConductorRunnerProvider::CodingAgentAppServer
        );
    }

    #[test]
    fn workflow_contract_parse_fails_without_front_matter_fence() {
        let parse = parse_workflow_contract("No front matter here");
        let err = parse.expect_err("should fail");
        assert!(err.contains("must start with YAML front matter"));
    }

    #[test]
    fn workflow_contract_parse_fails_on_unknown_front_matter_fields() {
        let parse = parse_workflow_contract(
            r#"---
schemaVersion: 1
workflowId: watchdog_focus
title: Watchdog focus review
unexpectedKey: nope
---
Prompt body
"#,
        );
        let err = parse.expect_err("should fail");
        assert!(err.contains("front matter parse error"));
    }

    #[test]
    fn workflow_contract_parse_fails_when_prompt_template_is_empty() {
        let parse = parse_workflow_contract(
            r#"---
schemaVersion: 1
workflowId: watchdog_focus
title: Watchdog focus review
---
"#,
        );
        let err = parse.expect_err("should fail");
        assert!(err.contains("prompt template body is empty"));
    }

    #[test]
    fn save_workflow_settings_updates_front_matter_and_preserves_prompt() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(
            dir.path().join("WORKFLOW.md"),
            r#"---
schemaVersion: 1
workflowId: watchdog_focus
provider: coding_agent_app_server
title: Watchdog focus review
maxRetries: 3
timeoutSeconds: 120
requiredChecks:
  - tests
---
Prompt line one.
Prompt line two.
"#,
        )
        .expect("write workflow");

        let update = ConductorWorkflowSettingsUpdate {
            provider: Some(ConductorRunnerProvider::OpenAi),
            provider_model: Some(" gpt-4.1-mini ".to_string()),
            provider_strict_mode: Some(true),
            timeout_seconds: Some(8_000),
            trace_retention: Some(500),
        };
        let loaded = save_workflow_settings(dir.path(), &update).expect("save workflow settings");
        assert!(loaded.loaded);
        let contract = loaded.contract.expect("contract");
        assert_eq!(
            contract.front_matter.provider,
            ConductorRunnerProvider::OpenAi
        );
        assert_eq!(
            contract.front_matter.provider_model.as_deref(),
            Some("gpt-4.1-mini")
        );
        assert!(contract.front_matter.provider_strict_mode);
        assert_eq!(contract.front_matter.timeout_seconds, 3_600);
        assert_eq!(contract.front_matter.trace_retention, 200);
        assert_eq!(
            contract.prompt_template,
            "Prompt line one.\nPrompt line two.".to_string()
        );
    }

    #[test]
    fn save_workflow_settings_clears_provider_model_when_empty_string() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(
            dir.path().join("WORKFLOW.md"),
            r#"---
schemaVersion: 1
workflowId: watchdog_focus
provider: openai
providerModel: gpt-4.1
providerStrictMode: false
traceRetention: 25
title: Watchdog focus review
maxRetries: 2
timeoutSeconds: 300
requiredChecks: []
---
Prompt.
"#,
        )
        .expect("write workflow");

        let update = ConductorWorkflowSettingsUpdate {
            provider: None,
            provider_model: Some("   ".to_string()),
            provider_strict_mode: Some(false),
            timeout_seconds: Some(7),
            trace_retention: Some(0),
        };
        let loaded = save_workflow_settings(dir.path(), &update).expect("save workflow settings");
        let contract = loaded.contract.expect("contract");
        assert_eq!(
            contract.front_matter.provider,
            ConductorRunnerProvider::OpenAi
        );
        assert_eq!(contract.front_matter.provider_model, None);
        assert!(!contract.front_matter.provider_strict_mode);
        assert_eq!(contract.front_matter.timeout_seconds, 10);
        assert_eq!(contract.front_matter.trace_retention, 1);
    }

    #[test]
    fn save_workflow_settings_fails_when_contract_file_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = save_workflow_settings(
            dir.path(),
            &ConductorWorkflowSettingsUpdate {
                provider: Some(ConductorRunnerProvider::OpenAi),
                ..ConductorWorkflowSettingsUpdate::default()
            },
        )
        .expect_err("missing workflow file must fail");
        assert!(err.contains("WORKFLOW.md not found"));
    }

    #[test]
    fn workspace_name_for_task_is_deterministic() {
        let first = workspace_name_for_task("Issue_123").expect("workspace name");
        let second = workspace_name_for_task("issue_123").expect("workspace name");
        assert_eq!(first, second);
        assert!(first.starts_with("task-issue_123-"));
    }

    #[test]
    fn workspace_name_for_task_rejects_invalid_ids() {
        let err = workspace_name_for_task("../escape").expect_err("must reject traversal");
        assert!(err.contains("must not contain"));

        let err = workspace_name_for_task("has/slash").expect_err("must reject slash");
        assert!(err.contains("unsupported characters"));
    }

    #[test]
    fn ensure_task_workspace_creates_path_under_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("workspaces");
        let spec = ensure_task_workspace(&root, "task_77").expect("workspace");

        let root_canonical = root.canonicalize().expect("root canonical");
        let workspace_path = PathBuf::from(&spec.workspace_path);
        assert!(workspace_path.exists());
        assert!(workspace_path.starts_with(&root_canonical));
        assert_eq!(
            spec.workspace_name,
            workspace_name_for_task("task_77").unwrap()
        );
    }

    #[test]
    fn app_server_session_id_is_deterministic_per_task_and_attempt() {
        let first = app_server_session_id("Issue_42", 1).expect("session id");
        let second = app_server_session_id("issue_42", 1).expect("session id");
        let next_attempt = app_server_session_id("issue_42", 2).expect("session id");

        assert_eq!(first, second);
        assert_ne!(first, next_attempt);
        assert!(first.starts_with("slopo-issue_42-a1-"));
        assert!(next_attempt.starts_with("slopo-issue_42-a2-"));
    }

    #[test]
    fn build_app_server_launch_payload_validates_required_fields() {
        let relative_workspace = ConductorRunnerLaunchRequest {
            provider: ConductorRunnerProvider::CodingAgentAppServer,
            task_id: "issue_10".to_string(),
            run_attempt: 0,
            workflow_id: "watchdog_focus".to_string(),
            workspace_path: "relative/path".to_string(),
            prompt: "check the task".to_string(),
        };
        let err = build_app_server_launch_payload(&relative_workspace).expect_err("must fail");
        assert!(err.contains("workspace_path must be absolute"));

        let empty_workflow = ConductorRunnerLaunchRequest {
            provider: ConductorRunnerProvider::CodingAgentAppServer,
            task_id: "issue_10".to_string(),
            run_attempt: 0,
            workflow_id: "   ".to_string(),
            workspace_path: "/tmp/workspace".to_string(),
            prompt: "check the task".to_string(),
        };
        let err = build_app_server_launch_payload(&empty_workflow).expect_err("must fail");
        assert!(err.contains("workflow_id is empty"));

        let empty_prompt = ConductorRunnerLaunchRequest {
            provider: ConductorRunnerProvider::CodingAgentAppServer,
            task_id: "issue_10".to_string(),
            run_attempt: 0,
            workflow_id: "watchdog_focus".to_string(),
            workspace_path: "/tmp/workspace".to_string(),
            prompt: "  ".to_string(),
        };
        let err = build_app_server_launch_payload(&empty_prompt).expect_err("must fail");
        assert!(err.contains("prompt is empty"));

        let wrong_provider = ConductorRunnerLaunchRequest {
            provider: ConductorRunnerProvider::OpenAi,
            task_id: "issue_10".to_string(),
            run_attempt: 0,
            workflow_id: "watchdog_focus".to_string(),
            workspace_path: "/tmp/workspace".to_string(),
            prompt: "check the task".to_string(),
        };
        let err = build_app_server_launch_payload(&wrong_provider).expect_err("must fail");
        assert!(err.contains("requires provider=coding_agent_app_server"));
    }

    #[test]
    fn build_app_server_launch_payload_produces_protocol_message() {
        let dir = tempfile::tempdir().expect("tempdir");
        let request = ConductorRunnerLaunchRequest {
            provider: ConductorRunnerProvider::CodingAgentAppServer,
            task_id: "task_11".to_string(),
            run_attempt: 3,
            workflow_id: "watchdog_focus".to_string(),
            workspace_path: dir.path().display().to_string(),
            prompt: "Run watchdog checks".to_string(),
        };
        let payload = build_app_server_launch_payload(&request).expect("payload");

        assert_eq!(payload.protocol_version, 1);
        assert_eq!(
            payload.provider,
            ConductorRunnerProvider::CodingAgentAppServer
        );
        assert_eq!(payload.action, "start_session");
        assert_eq!(payload.task_id, "task_11");
        assert_eq!(payload.run_attempt, 3);
        assert_eq!(payload.workflow_id, "watchdog_focus");
        assert_eq!(payload.workspace_path, request.workspace_path);
        assert_eq!(payload.prompt, "Run watchdog checks");
        assert!(payload.session_id.starts_with("slopo-task_11-a3-"));
    }

    #[test]
    fn build_openai_prompt_enforces_json_output_contract() {
        let prompt = build_openai_prompt(
            "You are a watchdog evaluator.",
            r#"{"branch":"main","revision":"abc123"}"#,
        )
        .expect("prompt");
        assert!(prompt.contains("Snapshot JSON:"));
        assert!(prompt.contains("OpenAI output contract"));
        assert!(prompt.contains("Return exactly one valid JSON object"));

        let err = build_openai_prompt("", "{}").expect_err("empty template");
        assert!(err.contains("prompt_template is empty"));

        let err = build_openai_prompt("template", "   ").expect_err("empty snapshot");
        assert!(err.contains("snapshot_json is empty"));
    }

    #[test]
    fn parse_openai_watchdog_verdict_accepts_plain_and_fenced_json() {
        let plain = r#"{
  "status":"on_track",
  "confidence":0.91,
  "summary":"Aligned with issue scope.",
  "issues":[]
}"#;
        let verdict = parse_openai_watchdog_verdict(plain).expect("plain json");
        assert_eq!(verdict.status, watchdog_types::WatchdogStatus::OnTrack);
        assert!((verdict.confidence - 0.91).abs() < 0.001);
        assert_eq!(verdict.issues.len(), 0);

        let fenced = r#"```json
{
  "status":"drift",
  "confidence":0.4,
  "summary":"Scope drift.",
  "issues":[
    {
      "kind":"drift",
      "severity":"warning",
      "title":"Potential drift",
      "why":"Files are unrelated",
      "evidence":["lib/a.rs","lib/b.rs"],
      "recommended_next_step":"Narrow scope"
    }
  ]
}
```"#;
        let verdict = parse_openai_watchdog_verdict(fenced).expect("fenced json");
        assert_eq!(verdict.status, watchdog_types::WatchdogStatus::Drift);
        assert_eq!(verdict.issues.len(), 1);
        assert_eq!(
            verdict.issues[0].kind,
            watchdog_types::WatchdogIssueKind::Drift
        );
        assert_eq!(
            verdict.issues[0].severity,
            watchdog_types::WatchdogSeverity::Warning
        );
    }

    #[test]
    fn parse_openai_watchdog_verdict_rejects_invalid_payload() {
        let err = parse_openai_watchdog_verdict("not-json").expect_err("must fail");
        assert!(err.contains("does not contain a JSON object"));

        let err = parse_openai_watchdog_verdict(
            r#"{"status":"unknown","confidence":0.5,"summary":"x","issues":[]}"#,
        )
        .expect_err("invalid status must fail");
        assert!(err.contains("Unsupported status"));
    }

    #[test]
    fn parse_app_server_event_parses_started_and_rejects_invalid_payloads() {
        let started = parse_app_server_event(
            r#"{"eventType":"started","session_id":"s1","started_at":"2026-03-10T10:00:00Z"}"#,
        )
        .expect("started event");
        assert_eq!(
            started,
            ConductorRunnerEvent::Started {
                session_id: "s1".to_string(),
                started_at: "2026-03-10T10:00:00Z".to_string(),
            }
        );

        let err = parse_app_server_event("not-json").expect_err("must fail");
        assert!(err.contains("Invalid app-server runner event payload"));
    }

    #[test]
    fn reconcile_tick_schedules_retry_then_reclaims_and_restarts() {
        let config = ConductorSchedulerConfig {
            max_concurrent_runs: 2,
            retry_base_seconds: 10,
            retry_max_seconds: 120,
        };
        let mut tasks = vec![ConductorTask::new_claimed("task-a", "unix:1", 2)];

        let first_tick = reconcile_conductor_tick(&mut tasks, &[], 100, &config).expect("tick");
        assert_eq!(first_tick.started_task_ids, vec!["task-a"]);
        assert_eq!(tasks[0].state, ConductorTaskState::Running);
        assert_eq!(tasks[0].run_attempt, 1);

        let failure_update = ConductorRunnerUpdate {
            task_id: "task-a".to_string(),
            event: ConductorRunnerEvent::Failed {
                session_id: "s-a".to_string(),
                failed_at: "2026-03-10T10:00:05Z".to_string(),
                error: "timeout".to_string(),
            },
        };
        let second_tick =
            reconcile_conductor_tick(&mut tasks, &[failure_update], 105, &config).expect("tick");
        assert_eq!(second_tick.scheduled_retry_task_ids, vec!["task-a"]);
        assert_eq!(tasks[0].state, ConductorTaskState::RetryScheduled);
        assert_eq!(tasks[0].retry_count, 1);
        assert_eq!(tasks[0].retry_at.as_deref(), Some("unix:115"));

        let third_tick = reconcile_conductor_tick(&mut tasks, &[], 116, &config).expect("tick");
        assert_eq!(third_tick.reclaimed_retry_task_ids, vec!["task-a"]);
        assert_eq!(third_tick.started_task_ids, vec!["task-a"]);
        assert_eq!(tasks[0].state, ConductorTaskState::Running);
        assert_eq!(tasks[0].run_attempt, 2);
    }

    #[test]
    fn reconcile_tick_marks_task_exhausted_when_retries_are_spent() {
        let config = ConductorSchedulerConfig {
            max_concurrent_runs: 1,
            retry_base_seconds: 10,
            retry_max_seconds: 120,
        };
        let mut task = ConductorTask::new_claimed("task-b", "unix:1", 0);
        task.apply(ConductorTransition::StartRunning {
            started_at: "unix:2".to_string(),
        })
        .expect("start");
        let mut tasks = vec![task];
        let updates = vec![ConductorRunnerUpdate {
            task_id: "task-b".to_string(),
            event: ConductorRunnerEvent::Failed {
                session_id: "s-b".to_string(),
                failed_at: "2026-03-10T10:01:00Z".to_string(),
                error: "compile failed".to_string(),
            },
        }];

        let tick = reconcile_conductor_tick(&mut tasks, &updates, 200, &config).expect("tick");
        assert_eq!(tick.reconciled_task_ids, vec!["task-b"]);
        assert_eq!(tasks[0].state, ConductorTaskState::Reconciled);
        assert_eq!(
            tasks[0].reconciled_outcome,
            Some(ConductorReconciledOutcome::Exhausted)
        );
    }

    #[test]
    fn reconcile_tick_respects_bounded_concurrency() {
        let config = ConductorSchedulerConfig {
            max_concurrent_runs: 1,
            retry_base_seconds: 10,
            retry_max_seconds: 120,
        };
        let mut tasks = vec![
            ConductorTask::new_claimed("task-c", "unix:1", 1),
            ConductorTask::new_claimed("task-a", "unix:1", 1),
            ConductorTask::new_claimed("task-b", "unix:1", 1),
        ];

        let tick = reconcile_conductor_tick(&mut tasks, &[], 42, &config).expect("tick");
        assert_eq!(tick.started_task_ids, vec!["task-a"]);
        assert_eq!(tasks[0].state, ConductorTaskState::Claimed);
        assert_eq!(tasks[1].state, ConductorTaskState::Running);
        assert_eq!(tasks[2].state, ConductorTaskState::Claimed);
    }

    #[test]
    fn enqueue_task_normalizes_id_and_rejects_duplicates() {
        let mut runtime = ConductorRuntimeState::default();
        let task = enqueue_task(&mut runtime, "Issue_90", 100, Some(3)).expect("enqueue");
        assert_eq!(task.task_id, "issue_90");
        assert_eq!(task.max_retries, 3);
        assert_eq!(runtime.tasks.len(), 1);

        let err = enqueue_task(&mut runtime, "issue_90", 100, None).expect_err("duplicate");
        assert!(err.contains("task already exists"));
    }

    #[test]
    fn refresh_runtime_returns_status_and_traces() {
        let mut runtime = ConductorRuntimeState {
            config: ConductorSchedulerConfig {
                max_concurrent_runs: 1,
                retry_base_seconds: 10,
                retry_max_seconds: 120,
            },
            ..ConductorRuntimeState::default()
        };
        enqueue_task(&mut runtime, "task-z", 10, Some(1)).expect("enqueue");

        let first = refresh_runtime(&mut runtime, &[], 20).expect("refresh");
        assert_eq!(first.tick.started_task_ids, vec!["task-z"]);
        assert_eq!(first.status.tasks_running, 1);
        assert_eq!(first.status.tasks_total, 1);
        assert!(first.status.traces_total >= 3);

        let updates = vec![ConductorRunnerUpdate {
            task_id: "task-z".to_string(),
            event: ConductorRunnerEvent::Completed {
                session_id: "s-z".to_string(),
                completed_at: "2026-03-10T11:00:00Z".to_string(),
                result_summary: Some("ok".to_string()),
            },
        }];
        let second = refresh_runtime(&mut runtime, &updates, 25).expect("refresh");
        assert_eq!(second.tick.reconciled_task_ids, vec!["task-z"]);
        assert_eq!(second.status.tasks_reconciled, 1);
        assert_eq!(second.status.tasks_running, 0);
        assert_eq!(second.status.last_tick_at.as_deref(), Some("unix:25"));
    }

    #[test]
    fn build_status_view_applies_trace_limit() {
        let mut runtime = ConductorRuntimeState::default();
        for idx in 0..40 {
            runtime.traces.push(ConductorRunTraceEntry {
                at: format!("unix:{idx}"),
                event: "noop".to_string(),
                task_id: None,
                detail: None,
            });
        }
        let status = build_status_view(&runtime, 200, 5);
        assert_eq!(status.traces_total, 40);
        assert_eq!(status.recent_traces.len(), 5);
        assert_eq!(status.recent_traces[0].at, "unix:39");
    }

    #[test]
    fn record_provider_failure_updates_counters_and_status_view() {
        let mut runtime = ConductorRuntimeState::default();
        record_provider_failure(
            &mut runtime,
            ConductorRunnerProvider::OpenAi,
            true,
            "openai request timeout",
            321,
        );
        record_provider_failure(
            &mut runtime,
            ConductorRunnerProvider::OpenAi,
            false,
            "openai malformed response",
            400,
        );

        assert_eq!(runtime.provider_failures.total, 2);
        assert_eq!(runtime.provider_failures.openai, 2);
        assert_eq!(runtime.provider_failures.coding_agent_app_server, 0);
        assert_eq!(runtime.provider_failures.strict_mode_blocked, 1);
        assert_eq!(runtime.provider_failures.fallback_used, 1);
        assert_eq!(
            runtime.provider_failures.last_failure_provider,
            Some(ConductorRunnerProvider::OpenAi)
        );
        assert_eq!(
            runtime.provider_failures.last_failure_at.as_deref(),
            Some("unix:400")
        );
        assert!(runtime
            .provider_failures
            .last_failure_message
            .as_deref()
            .is_some_and(|msg| msg.contains("malformed")));

        let view = build_status_view(&runtime, 401, 20);
        assert_eq!(view.provider_failures.total, 2);
        assert_eq!(view.provider_failures.strict_mode_blocked, 1);
        assert_eq!(view.provider_failures.fallback_used, 1);
        assert!(view
            .recent_traces
            .iter()
            .any(|entry| entry.event == "provider_failure"));
    }

    #[test]
    fn task_context_from_contract_captures_workflow_fields() {
        let contract = parse_workflow_contract(
            "---\nschemaVersion: 1\nworkflowId: my-flow\ntitle: T\nmaxRetries: 3\ntimeoutSeconds: 120\n---\nPrompt",
        )
        .expect("parse");
        let ctx = TaskContext::from_contract("task-abc", &contract, "my prompt".to_string());
        assert_eq!(ctx.task_id, "task-abc");
        assert_eq!(ctx.workflow_id, "my-flow");
        assert_eq!(ctx.max_retries, 3);
        assert_eq!(ctx.timeout_seconds, 120);
        assert_eq!(ctx.prompt, "my prompt");
        assert!(ctx.issue_id.is_none());
    }

    #[test]
    fn task_context_with_issue_attaches_issue_fields() {
        let contract =
            parse_workflow_contract("---\nschemaVersion: 1\nworkflowId: flow\ntitle: T\n---\nP")
                .expect("parse");
        let ctx = TaskContext::from_contract("t", &contract, "p".to_string())
            .with_issue("issue-7", "Fix parser");
        assert_eq!(ctx.issue_id.as_deref(), Some("issue-7"));
        assert_eq!(ctx.issue_title.as_deref(), Some("Fix parser"));
    }

    #[test]
    fn issue_run_attaches_session_and_pushes_artifacts() {
        let task = ConductorTask::new_claimed("task-1", "unix:1", 1);
        let contract =
            parse_workflow_contract("---\nschemaVersion: 1\nworkflowId: flow\ntitle: T\n---\nP")
                .expect("parse");
        let ctx = TaskContext::from_contract("task-1", &contract, "prompt".to_string());
        let mut run = IssueRun::new(task, ctx);

        assert!(run.active_session_id.is_none());
        assert!(!run.is_terminal());

        run.attach_session("sess-42");
        assert_eq!(run.active_session_id.as_deref(), Some("sess-42"));

        run.push_artifact(RunArtifact {
            kind: "verdict".to_string(),
            content: "{}".to_string(),
            created_at: "unix:2".to_string(),
        });
        assert_eq!(run.artifacts.len(), 1);
        assert_eq!(run.artifacts[0].kind, "verdict");
    }

    #[test]
    fn null_adapter_launch_returns_session_id() {
        let adapter = NullRunnerAdapter;
        let run = WorkerRun {
            session_id: "sess-abc".to_string(),
            task_id: "task-1".to_string(),
            run_attempt: 1,
            workspace_path: "/tmp/ws".to_string(),
            launched_at: "unix:10".to_string(),
        };
        let session = adapter.launch(&run, "prompt").expect("launch");
        assert_eq!(session, "sess-abc");

        let event = adapter.parse_event("anything").expect("parse event");
        assert_eq!(event, RunEvent::Completed { summary: None });
    }

    #[test]
    fn deterministic_worker_creates_isolated_worktree_writes_file_and_captures_patch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        fs::write(repo.join("app.js"), "export const answer = 1;\n").expect("seed file");
        Command::new("git")
            .args(["init"])
            .current_dir(repo)
            .output()
            .expect("git init");
        Command::new("git")
            .args(["config", "user.email", "slopo@example.test"])
            .current_dir(repo)
            .output()
            .expect("git email");
        Command::new("git")
            .args(["config", "user.name", "Sloposcope Test"])
            .current_dir(repo)
            .output()
            .expect("git name");
        Command::new("git")
            .args(["add", "app.js"])
            .current_dir(repo)
            .output()
            .expect("git add");
        Command::new("git")
            .args(["commit", "-m", "seed"])
            .current_dir(repo)
            .output()
            .expect("git commit");

        let result = run_deterministic_code_worker(
            repo,
            "mission-1",
            "objective-1",
            "session-1",
            &DeterministicWriteSpec {
                relative_path: "app.js".into(),
                content: "export const answer = 2;\n".into(),
                validation_command: "test \"$(cat app.js)\" = \"export const answer = 2;\" && printf 'validation-ok'".into(),
            },
        )
        .expect("run deterministic worker");

        assert!(result
            .workspace
            .workspace_path
            .contains("/.sloposcope/worktrees/"));
        assert_eq!(
            fs::read_to_string(&result.absolute_path).expect("read rewritten file"),
            "export const answer = 2;\n"
        );
        assert!(result.patch.contains("-export const answer = 1;"));
        assert!(result.patch.contains("+export const answer = 2;"));
        assert!(result.validation.success);
        assert_eq!(result.validation.exit_code, Some(0));
        assert!(result.validation.stdout.contains("validation-ok"));
    }
}

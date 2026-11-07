use crate::watchdog;
use crate::work_store::{AgentProfile, MissionRecord, ObjectiveRecord};
use serde::Serialize;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

const HARNESS_SOURCE: &str = include_str!("../scripts/agent_harness.py");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHarnessBundle {
    pub harness_script_path: String,
    pub bundle_dir: String,
    pub instruction_pack_path: String,
    pub mission_facts_path: String,
    pub objective_contract_path: String,
    pub session_context_path: String,
    pub context_snapshot_path: String,
    pub prompt_path: String,
    pub nudge_path: String,
    pub mcp_config_path: String,
    pub opencode_config_path: String,
    pub pi_config_dir: String,
    pub pi_config_path: String,
    pub pi_models_path: String,
    pub diagnostics_path: String,
    pub mcp_command: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MissionFacts<'a> {
    mission_id: &'a str,
    title: &'a str,
    brief: &'a str,
    validation_command: &'a str,
    planner_notes: Option<&'a str>,
    issue_key: Option<&'a str>,
    issue_url: Option<&'a str>,
    repo_path: &'a str,
    worktree_path: &'a str,
    graph_snapshot: &'a str,
    instruction_pack_hash: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ObjectiveContract<'a> {
    objective_id: &'a str,
    title: &'a str,
    prompt: &'a str,
    scope_paths: &'a [String],
    expected_validation: &'a [String],
    done_criteria: &'a [String],
    test_intent: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionContext<'a> {
    session_id: &'a str,
    profile_id: &'a str,
    profile_label: &'a str,
    provider: &'a str,
    model: Option<&'a str>,
    transport_kind: &'a str,
    capabilities: &'a [String],
    workspace_path: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActiveMcpConfig<'a> {
    command: &'a [String],
    repo_path: &'a str,
    mission_id: &'a str,
    objective_id: &'a str,
    session_id: &'a str,
    profile_id: &'a str,
    tools: [&'a str; 7],
}

pub fn is_opencode_cli_provider(provider: &str) -> bool {
    let normalized = provider.trim().to_ascii_lowercase();
    normalized == "opencode_cli" || normalized == "opencode_nim"
}

pub fn is_pi_cli_provider(provider: &str) -> bool {
    provider.trim().eq_ignore_ascii_case("pi_cli")
}

pub fn prompt_to_stdin_for_provider(provider: &str) -> bool {
    matches!(provider.trim(), "gemini_cli" | "codex_cli" | "claude_cli")
}

fn provider_prompt_hint(profile: &AgentProfile) -> Option<&'static str> {
    match profile.provider.trim() {
        "gemini_cli" => Some(
            "Gemini CLI profile. Prompt goes to stdin. First pull facts with agent_request_context, then work from files in bundle.",
        ),
        "codex_cli" => Some(
            "Codex CLI profile. Prompt goes to stdin. First pull facts with agent_request_context, then work from files in bundle.",
        ),
        "claude_cli" => Some(
            "Claude CLI profile. Prompt goes to stdin. First pull facts with agent_request_context, then work from files in bundle.",
        ),
        "local_cli" => Some(
            "Local CLI profile. No chat loop assumed. Read bundle files directly. Use active MCP only for structured state and extra facts.",
        ),
        "opencode_cli" | "opencode_nim" => Some(
            "OpenCode CLI profile. Harness injects project-local opencode config with NIM provider and sloposcope MCP. Use MCP tools for authoritative mission state.",
        ),
        "pi_cli" => Some(
            "Pi coding agent profile. Harness injects project-local PI_CODING_AGENT_DIR with NIM-backed config/models and sloposcope MCP. Use MCP tools for authoritative mission state.",
        ),
        _ => None,
    }
}

fn write_if_changed(path: &Path, content: &str) -> Result<(), String> {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == content {
            return Ok(());
        }
    }
    fs::write(path, content).map_err(|err| format!("write {}: {err}", path.display()))
}

fn ensure_runtime_dir(repo: &Path) -> Result<PathBuf, String> {
    let dir = repo.join(".sloposcope").join("runtime");
    fs::create_dir_all(&dir)
        .map_err(|err| format!("create runtime dir {}: {err}", dir.display()))?;
    Ok(dir)
}

fn ensure_bundle_dir(workspace_path: &str, session_id: &str) -> Result<PathBuf, String> {
    let dir = Path::new(workspace_path)
        .join(".sloposcope")
        .join("agent-sessions")
        .join(session_id);
    fs::create_dir_all(&dir)
        .map_err(|err| format!("create bundle dir {}: {err}", dir.display()))?;
    Ok(dir)
}

pub fn session_nudge_path(workspace_path: &str, session_id: &str) -> PathBuf {
    Path::new(workspace_path)
        .join(".sloposcope")
        .join("agent-sessions")
        .join(session_id)
        .join("nudges.ndjson")
}

pub fn append_session_nudge(
    workspace_path: &str,
    session_id: &str,
    message: &str,
) -> Result<PathBuf, String> {
    let path = session_nudge_path(workspace_path, session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create nudge dir {}: {err}", parent.display()))?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| format!("open nudge file {}: {err}", path.display()))?;
    let payload = format!("{}\n", message.trim());
    file.write_all(payload.as_bytes())
        .map_err(|err| format!("append nudge file {}: {err}", path.display()))?;
    Ok(path)
}

fn opencode_config_path(workspace_path: &str) -> PathBuf {
    Path::new(workspace_path)
        .join(".sloposcope")
        .join("opencode.json")
}

fn pi_config_dir(workspace_path: &str) -> PathBuf {
    Path::new(workspace_path)
        .join(".sloposcope")
        .join("pi-agent")
}

fn pi_config_path(workspace_path: &str) -> PathBuf {
    pi_config_dir(workspace_path).join("config.yml")
}

fn pi_models_path(workspace_path: &str) -> PathBuf {
    pi_config_dir(workspace_path).join("models.yml")
}

fn ensure_harness_script(repo: &Path) -> Result<PathBuf, String> {
    let runtime_dir = ensure_runtime_dir(repo)?;
    let path = runtime_dir.join("agent_harness.py");
    write_if_changed(&path, HARNESS_SOURCE)?;
    Ok(path)
}

fn default_mcp_command() -> Vec<String> {
    if let Ok(raw) = std::env::var("SLOPOSCOPE_MCP_COMMAND_JSON") {
        if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&raw) {
            let parsed: Vec<String> = parsed
                .into_iter()
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect();
            if !parsed.is_empty() {
                return parsed;
            }
        }
    }

    if let Ok(current_exe) = std::env::current_exe() {
        let sibling = current_exe.with_file_name("sloposcope-mcp");
        if sibling.is_file() {
            return vec![sibling.display().to_string()];
        }
    }

    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    vec![
        std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()),
        "run".to_string(),
        "--quiet".to_string(),
        "--manifest-path".to_string(),
        manifest_path.display().to_string(),
        "--bin".to_string(),
        "sloposcope-mcp".to_string(),
    ]
}

fn profile_env_lookup(profile: &AgentProfile, keys: &[&str]) -> Option<String> {
    let from_profile = profile.env.as_object().and_then(|map| {
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

fn opencode_nim_model(profile: &AgentProfile) -> String {
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

fn opencode_nim_base_url(profile: &AgentProfile) -> String {
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

fn opencode_nim_api_key(profile: &AgentProfile) -> Result<String, String> {
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
            "OpenCode profile '{}' missing NVIDIA API key. Set NVIDIA_API_KEY, NGC_API_KEY, or NVIDIA_NIM_API_KEY in Env JSON or shell env.",
            profile.id
        )
    })
}

fn write_opencode_config(
    profile: &AgentProfile,
    bundle: &AgentHarnessBundle,
) -> Result<(), String> {
    let config_path = Path::new(&bundle.opencode_config_path);
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create opencode config dir {}: {err}", parent.display()))?;
    }
    let nim_model = opencode_nim_model(profile);
    let nim_api_key = opencode_nim_api_key(profile)?;
    let nim_base_url = opencode_nim_base_url(profile);
    let config = serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "autoupdate": false,
        "enabled_providers": ["sloposcope_nim"],
        "model": format!("sloposcope_nim/{nim_model}"),
        "provider": {
            "sloposcope_nim": {
                "npm": "@ai-sdk/openai-compatible",
                "options": {
                    "baseURL": nim_base_url,
                    "apiKey": nim_api_key
                },
                "models": {
                    nim_model: {
                        "name": "Sloposcope NIM"
                    }
                }
            }
        },
        "instructions": [
            bundle.instruction_pack_path.clone()
        ],
        "mcp": {
            "sloposcope": {
                "type": "local",
                "enabled": true,
                "command": bundle.mcp_command.clone(),
            }
        }
    });
    fs::write(
        config_path,
        serde_json::to_vec_pretty(&config)
            .map_err(|err| format!("serialize opencode config: {err}"))?,
    )
    .map_err(|err| format!("write {}: {err}", config_path.display()))
}

fn write_pi_config(profile: &AgentProfile, workspace_path: &str) -> Result<(), String> {
    let config_dir = pi_config_dir(workspace_path);
    fs::create_dir_all(&config_dir)
        .map_err(|err| format!("create pi config dir {}: {err}", config_dir.display()))?;

    let nim_model = opencode_nim_model(profile);
    let nim_base_url = opencode_nim_base_url(profile);
    let model_ref = format!("sloposcope_nim/{nim_model}");
    let config = serde_yaml::to_string(&serde_yaml::Value::Mapping(
        serde_yaml::Mapping::from_iter([
            (
                serde_yaml::Value::String("defaultThinkingLevel".to_string()),
                serde_yaml::Value::String("medium".to_string()),
            ),
            (
                serde_yaml::Value::String("enabledModels".to_string()),
                serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                    "sloposcope_nim/*".to_string(),
                )]),
            ),
            (
                serde_yaml::Value::String("modelProviderOrder".to_string()),
                serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                    "sloposcope_nim".to_string(),
                )]),
            ),
            (
                serde_yaml::Value::String("modelRoles".to_string()),
                serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter([
                    (
                        serde_yaml::Value::String("default".to_string()),
                        serde_yaml::Value::String(model_ref.clone()),
                    ),
                    (
                        serde_yaml::Value::String("smol".to_string()),
                        serde_yaml::Value::String(model_ref.clone()),
                    ),
                    (
                        serde_yaml::Value::String("slow".to_string()),
                        serde_yaml::Value::String(model_ref.clone()),
                    ),
                    (
                        serde_yaml::Value::String("plan".to_string()),
                        serde_yaml::Value::String(model_ref.clone()),
                    ),
                    (
                        serde_yaml::Value::String("commit".to_string()),
                        serde_yaml::Value::String(model_ref),
                    ),
                ])),
            ),
        ]),
    ))
    .map_err(|err| format!("serialize pi config: {err}"))?;
    write_if_changed(&pi_config_path(workspace_path), &config)?;

    let models = serde_yaml::to_string(&serde_yaml::Value::Mapping(
        serde_yaml::Mapping::from_iter([(
            serde_yaml::Value::String("providers".to_string()),
            serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter([(
                serde_yaml::Value::String("sloposcope_nim".to_string()),
                serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter([
                    (
                        serde_yaml::Value::String("baseUrl".to_string()),
                        serde_yaml::Value::String(nim_base_url),
                    ),
                    (
                        serde_yaml::Value::String("apiKey".to_string()),
                        serde_yaml::Value::String("NVIDIA_API_KEY".to_string()),
                    ),
                    (
                        serde_yaml::Value::String("api".to_string()),
                        serde_yaml::Value::String("openai-completions".to_string()),
                    ),
                    (
                        serde_yaml::Value::String("authHeader".to_string()),
                        serde_yaml::Value::Bool(true),
                    ),
                    (
                        serde_yaml::Value::String("models".to_string()),
                        serde_yaml::Value::Sequence(vec![serde_yaml::Value::Mapping(
                            serde_yaml::Mapping::from_iter([
                                (
                                    serde_yaml::Value::String("id".to_string()),
                                    serde_yaml::Value::String(nim_model),
                                ),
                                (
                                    serde_yaml::Value::String("name".to_string()),
                                    serde_yaml::Value::String("Sloposcope NIM".to_string()),
                                ),
                                (
                                    serde_yaml::Value::String("reasoning".to_string()),
                                    serde_yaml::Value::Bool(true),
                                ),
                                (
                                    serde_yaml::Value::String("input".to_string()),
                                    serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(
                                        "text".to_string(),
                                    )]),
                                ),
                                (
                                    serde_yaml::Value::String("contextWindow".to_string()),
                                    serde_yaml::Value::Number(128000.into()),
                                ),
                                (
                                    serde_yaml::Value::String("maxTokens".to_string()),
                                    serde_yaml::Value::Number(32000.into()),
                                ),
                                (
                                    serde_yaml::Value::String("cost".to_string()),
                                    serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter([
                                        (
                                            serde_yaml::Value::String("input".to_string()),
                                            serde_yaml::Value::Number(0.into()),
                                        ),
                                        (
                                            serde_yaml::Value::String("output".to_string()),
                                            serde_yaml::Value::Number(0.into()),
                                        ),
                                        (
                                            serde_yaml::Value::String("cacheRead".to_string()),
                                            serde_yaml::Value::Number(0.into()),
                                        ),
                                        (
                                            serde_yaml::Value::String("cacheWrite".to_string()),
                                            serde_yaml::Value::Number(0.into()),
                                        ),
                                    ])),
                                ),
                                (
                                    serde_yaml::Value::String("compat".to_string()),
                                    serde_yaml::Value::Mapping(serde_yaml::Mapping::from_iter([(
                                        serde_yaml::Value::String(
                                            "supportsDeveloperRole".to_string(),
                                        ),
                                        serde_yaml::Value::Bool(false),
                                    )])),
                                ),
                            ]),
                        )]),
                    ),
                ])),
            )])),
        )]),
    ))
    .map_err(|err| format!("serialize pi models: {err}"))?;
    write_if_changed(&pi_models_path(workspace_path), &models)?;

    Ok(())
}

fn render_instruction_pack(repo: &Path) -> String {
    let context = watchdog::load_agents_context(repo);
    let mut lines = vec!["Base instruction pack".to_string(), "".to_string()];
    if let Some(path) = context.load.path {
        lines.push(format!("Source: {path}"));
    }
    if context.rules.is_empty() {
        lines.push("Rules: none loaded from AGENTS.md".to_string());
    } else {
        lines.push("Rules:".to_string());
        for rule in context.rules.into_iter().take(64) {
            lines.push(format!("- {rule}"));
        }
    }
    lines.join("\n") + "\n"
}

fn render_prompt(
    mission: &MissionRecord,
    objective: &ObjectiveRecord,
    profile: &AgentProfile,
    bundle: &AgentHarnessBundle,
) -> String {
    let validation = if objective.meta.expected_validation.is_empty() {
        mission.validation_command.clone()
    } else {
        objective.meta.expected_validation.join(" && ")
    };
    let mut lines = vec![
        format!("Mission: {}", mission.title),
        format!("Brief: {}", mission.brief),
        format!("Objective: {}", objective.title),
        "".to_string(),
        format!("Primary task: {}", objective.prompt),
        format!("Validation: {}", validation),
        format!(
            "Scope paths: {}",
            if objective.meta.scope_paths.is_empty() {
                "(none)".to_string()
            } else {
                objective.meta.scope_paths.join(", ")
            }
        ),
        "".to_string(),
        "Read bundle first. Use active MCP for structured updates.".to_string(),
        format!("- Base instructions: {}", bundle.instruction_pack_path),
        format!("- Mission facts: {}", bundle.mission_facts_path),
        format!("- Objective contract: {}", bundle.objective_contract_path),
        format!("- Session context: {}", bundle.session_context_path),
        format!(
            "- Runtime context snapshot: {}",
            bundle.context_snapshot_path
        ),
        format!("- Active MCP config: {}", bundle.mcp_config_path),
        "".to_string(),
        "Required sequence:".to_string(),
        "1. agent_login".to_string(),
        "2. agent_request_context before reading code broadly".to_string(),
        "3. agent_activity_set running".to_string(),
        "4. agent_report_event progress as needed".to_string(),
        "5. edit only inside isolated worktree and mission scope".to_string(),
        "6. agent_report_touched_files".to_string(),
        "7. run validation command".to_string(),
        "8. agent_logout when done".to_string(),
    ];
    if let Some(provider_hint) = provider_prompt_hint(profile) {
        lines.push("".to_string());
        lines.push(format!("Provider hint: {provider_hint}"));
    }
    if let Some(planner_notes) = mission
        .planner_notes
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push("".to_string());
        lines.push(format!("Planner notes: {planner_notes}"));
    }
    if let Some(test_intent) = &objective.meta.test_intent {
        lines.push("".to_string());
        lines.push(format!("Test intent: {test_intent}"));
    }
    lines.join("\n") + "\n"
}

pub fn prepare_agent_harness_bundle(
    repo: &Path,
    mission: &MissionRecord,
    objective: &ObjectiveRecord,
    session_id: &str,
    profile: &AgentProfile,
    workspace_path: &str,
) -> Result<AgentHarnessBundle, String> {
    let bundle_dir = ensure_bundle_dir(workspace_path, session_id)?;
    let harness_script_path = ensure_harness_script(repo)?;
    let instruction_pack_path = bundle_dir.join("base_instruction_pack.md");
    let mission_facts_path = bundle_dir.join("mission_facts.json");
    let objective_contract_path = bundle_dir.join("objective_contract.json");
    let session_context_path = bundle_dir.join("session_context.json");
    let context_snapshot_path = bundle_dir.join("runtime_context.json");
    let prompt_path = bundle_dir.join("prompt.md");
    let nudge_path = bundle_dir.join("nudges.ndjson");
    let mcp_config_path = bundle_dir.join("active_mcp.json");
    let diagnostics_path = bundle_dir.join("harness_diagnostics.json");
    let opencode_config_path = opencode_config_path(workspace_path);
    let pi_config_dir = pi_config_dir(workspace_path);
    let pi_config_path = pi_config_path(workspace_path);
    let pi_models_path = pi_models_path(workspace_path);
    let mcp_command = default_mcp_command();

    let bundle = AgentHarnessBundle {
        harness_script_path: harness_script_path.display().to_string(),
        bundle_dir: bundle_dir.display().to_string(),
        instruction_pack_path: instruction_pack_path.display().to_string(),
        mission_facts_path: mission_facts_path.display().to_string(),
        objective_contract_path: objective_contract_path.display().to_string(),
        session_context_path: session_context_path.display().to_string(),
        context_snapshot_path: context_snapshot_path.display().to_string(),
        prompt_path: prompt_path.display().to_string(),
        nudge_path: nudge_path.display().to_string(),
        mcp_config_path: mcp_config_path.display().to_string(),
        opencode_config_path: opencode_config_path.display().to_string(),
        pi_config_dir: pi_config_dir.display().to_string(),
        pi_config_path: pi_config_path.display().to_string(),
        pi_models_path: pi_models_path.display().to_string(),
        diagnostics_path: diagnostics_path.display().to_string(),
        mcp_command: mcp_command.clone(),
    };

    write_if_changed(&instruction_pack_path, &render_instruction_pack(repo))?;
    fs::write(
        &mission_facts_path,
        serde_json::to_vec_pretty(&MissionFacts {
            mission_id: &mission.id,
            title: &mission.title,
            brief: &mission.brief,
            validation_command: &mission.validation_command,
            planner_notes: mission.planner_notes.as_deref(),
            issue_key: mission.issue_key.as_deref(),
            issue_url: mission.issue_url.as_deref(),
            repo_path: &mission.repo_path,
            worktree_path: &mission.worktree_path,
            graph_snapshot: &mission.graph_snapshot,
            instruction_pack_hash: &mission.instruction_pack_hash,
        })
        .map_err(|err| format!("serialize mission facts: {err}"))?,
    )
    .map_err(|err| format!("write {}: {err}", mission_facts_path.display()))?;
    fs::write(
        &objective_contract_path,
        serde_json::to_vec_pretty(&ObjectiveContract {
            objective_id: &objective.id,
            title: &objective.title,
            prompt: &objective.prompt,
            scope_paths: &objective.meta.scope_paths,
            expected_validation: &objective.meta.expected_validation,
            done_criteria: &objective.meta.done_criteria,
            test_intent: objective.meta.test_intent.as_deref(),
        })
        .map_err(|err| format!("serialize objective contract: {err}"))?,
    )
    .map_err(|err| format!("write {}: {err}", objective_contract_path.display()))?;
    fs::write(
        &session_context_path,
        serde_json::to_vec_pretty(&SessionContext {
            session_id,
            profile_id: &profile.id,
            profile_label: &profile.label,
            provider: &profile.provider,
            model: profile.model.as_deref(),
            transport_kind: &profile.transport_kind,
            capabilities: &profile.capabilities,
            workspace_path,
        })
        .map_err(|err| format!("serialize session context: {err}"))?,
    )
    .map_err(|err| format!("write {}: {err}", session_context_path.display()))?;
    write_if_changed(&context_snapshot_path, "{}\n")?;
    write_if_changed(&nudge_path, "")?;
    fs::write(
        &mcp_config_path,
        serde_json::to_vec_pretty(&ActiveMcpConfig {
            command: &mcp_command,
            repo_path: &mission.repo_path,
            mission_id: &mission.id,
            objective_id: &objective.id,
            session_id,
            profile_id: &profile.id,
            tools: [
                "agent_login",
                "agent_request_context",
                "agent_activity_set",
                "agent_report_event",
                "agent_report_touched_files",
                "agent_submit_verdict",
                "agent_logout",
            ],
        })
        .map_err(|err| format!("serialize active MCP config: {err}"))?,
    )
    .map_err(|err| format!("write {}: {err}", mcp_config_path.display()))?;
    write_if_changed(
        &prompt_path,
        &render_prompt(mission, objective, profile, &bundle),
    )?;
    if is_opencode_cli_provider(&profile.provider) {
        write_opencode_config(profile, &bundle)?;
    }
    if is_pi_cli_provider(&profile.provider) {
        write_pi_config(profile, workspace_path)?;
    }

    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_store::{MissionRecord, ObjectiveMeta, ObjectiveRecord, WorkUnitStatus};

    #[test]
    fn prepare_agent_harness_bundle_writes_prompt_and_context_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let worktree = dir.path().join("worktree");
        fs::create_dir_all(&worktree).expect("worktree");
        let mission = MissionRecord {
            id: "mission-ui".into(),
            repo_path: dir.path().display().to_string(),
            worktree_path: dir.path().display().to_string(),
            graph_snapshot: "rev-1".into(),
            title: "Mission".into(),
            brief: "Fix harness".into(),
            validation_command: "npm run -s build".into(),
            planner_notes: Some("Stay viewer-first.".into()),
            planner_profile_id: None,
            planner_prompt_override: None,
            issue_key: Some("LIN-1".into()),
            issue_url: None,
            instruction_pack_hash: "hash".into(),
            state: "running".into(),
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };
        let objective = ObjectiveRecord {
            id: "objective-1".into(),
            mission_id: mission.id.clone(),
            title: "Implement".into(),
            prompt: "Do thing".into(),
            status: WorkUnitStatus::Pending,
            claimed_by: None,
            claimed_at: None,
            created_at: "unix:1".into(),
            meta: ObjectiveMeta {
                scope_paths: vec!["src/".into()],
                expected_validation: vec!["npm run -s build".into()],
                done_criteria: vec!["done".into()],
                test_intent: Some("outer_red".into()),
            },
        };
        let profile = AgentProfile {
            id: "cli".into(),
            label: "CLI".into(),
            provider: "local_cli".into(),
            model: Some("shell".into()),
            command: "/bin/sh".into(),
            args: vec!["-lc".into(), "echo ok".into()],
            env: serde_json::json!({}),
            capabilities: vec!["can_edit".into()],
            transport_kind: "pty".into(),
            active: true,
            is_reviewer: false,
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };

        let bundle = prepare_agent_harness_bundle(
            dir.path(),
            &mission,
            &objective,
            "session-1",
            &profile,
            &worktree.display().to_string(),
        )
        .expect("bundle");

        let prompt = fs::read_to_string(&bundle.prompt_path).expect("prompt");
        assert!(prompt.contains("agent_login"));
        assert!(prompt.contains("agent_request_context"));
        assert!(prompt.contains("Fix harness"));
        assert!(prompt.contains("Planner notes: Stay viewer-first."));
        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(&bundle.mcp_config_path).expect("mcp config"))
                .expect("json");
        assert_eq!(
            config.get("missionId").and_then(serde_json::Value::as_str),
            Some("mission-ui")
        );
        assert!(config["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .any(|value| value.as_str() == Some("agent_request_context")));
        assert!(Path::new(&bundle.harness_script_path).is_file());
    }

    #[test]
    fn prompt_to_stdin_behavior_matches_provider() {
        assert!(!prompt_to_stdin_for_provider("local_cli"));
        assert!(!prompt_to_stdin_for_provider("opencode_cli"));
        assert!(!prompt_to_stdin_for_provider("pi_cli"));
        assert!(prompt_to_stdin_for_provider("gemini_cli"));
        assert!(prompt_to_stdin_for_provider("codex_cli"));
        assert!(prompt_to_stdin_for_provider("claude_cli"));
    }

    #[test]
    fn prepare_agent_harness_bundle_writes_project_local_opencode_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let worktree = dir.path().join("worktree");
        fs::create_dir_all(&worktree).expect("worktree");
        let mission = MissionRecord {
            id: "mission-opencode".into(),
            repo_path: dir.path().display().to_string(),
            worktree_path: dir.path().display().to_string(),
            graph_snapshot: "rev-1".into(),
            title: "Mission".into(),
            brief: "Use opencode".into(),
            validation_command: "cargo test".into(),
            planner_notes: None,
            planner_profile_id: None,
            planner_prompt_override: None,
            issue_key: None,
            issue_url: None,
            instruction_pack_hash: "hash".into(),
            state: "running".into(),
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };
        let objective = ObjectiveRecord {
            id: "objective-opencode".into(),
            mission_id: mission.id.clone(),
            title: "Implement".into(),
            prompt: "Do opencode thing".into(),
            status: WorkUnitStatus::Pending,
            claimed_by: None,
            claimed_at: None,
            created_at: "unix:1".into(),
            meta: ObjectiveMeta {
                scope_paths: vec!["src-tauri/src/lib.rs".into()],
                expected_validation: vec!["cargo test".into()],
                done_criteria: vec!["done".into()],
                test_intent: Some("outer_red".into()),
            },
        };
        let profile = AgentProfile {
            id: "opencode".into(),
            label: "OpenCode".into(),
            provider: "opencode_cli".into(),
            model: Some("meta/test-model".into()),
            command: "opencode".into(),
            args: vec![],
            env: serde_json::json!({
                "NVIDIA_API_KEY": "nim-key",
                "NVIDIA_NIM_BASE_URL": "https://nim.example.test/v1"
            }),
            capabilities: vec!["can_edit".into()],
            transport_kind: "pipe".into(),
            active: true,
            is_reviewer: false,
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };

        let bundle = prepare_agent_harness_bundle(
            dir.path(),
            &mission,
            &objective,
            "session-opencode",
            &profile,
            &worktree.display().to_string(),
        )
        .expect("bundle");

        let config_path = worktree.join(".sloposcope").join("opencode.json");
        assert_eq!(
            bundle.opencode_config_path,
            config_path.display().to_string()
        );
        assert!(config_path.is_file());
        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(&config_path).expect("opencode config"))
                .expect("json");
        assert_eq!(
            config.get("model").and_then(serde_json::Value::as_str),
            Some("sloposcope_nim/meta/test-model")
        );
        assert_eq!(
            config
                .get("provider")
                .and_then(|value| value.get("sloposcope_nim"))
                .and_then(|value| value.get("npm"))
                .and_then(serde_json::Value::as_str),
            Some("@ai-sdk/openai-compatible")
        );
        assert_eq!(
            config
                .get("provider")
                .and_then(|value| value.get("sloposcope_nim"))
                .and_then(|value| value.get("options"))
                .and_then(|value| value.get("baseURL"))
                .and_then(serde_json::Value::as_str),
            Some("https://nim.example.test/v1")
        );
        assert_eq!(
            config
                .get("mcp")
                .and_then(|value| value.get("sloposcope"))
                .and_then(|value| value.get("type"))
                .and_then(serde_json::Value::as_str),
            Some("local")
        );
    }

    #[test]
    fn prepare_agent_harness_bundle_writes_project_local_pi_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let worktree = dir.path().join("worktree");
        fs::create_dir_all(&worktree).expect("worktree");
        let mission = MissionRecord {
            id: "mission-pi".into(),
            repo_path: dir.path().display().to_string(),
            worktree_path: dir.path().display().to_string(),
            graph_snapshot: "rev-1".into(),
            title: "Mission".into(),
            brief: "Use pi".into(),
            validation_command: "cargo test".into(),
            planner_notes: None,
            planner_profile_id: None,
            planner_prompt_override: None,
            issue_key: None,
            issue_url: None,
            instruction_pack_hash: "hash".into(),
            state: "running".into(),
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };
        let objective = ObjectiveRecord {
            id: "objective-pi".into(),
            mission_id: mission.id.clone(),
            title: "Implement".into(),
            prompt: "Do pi thing".into(),
            status: WorkUnitStatus::Pending,
            claimed_by: None,
            claimed_at: None,
            created_at: "unix:1".into(),
            meta: ObjectiveMeta {
                scope_paths: vec!["src-tauri/src/lib.rs".into()],
                expected_validation: vec!["cargo test".into()],
                done_criteria: vec!["done".into()],
                test_intent: Some("outer_red".into()),
            },
        };
        let profile = AgentProfile {
            id: "pi".into(),
            label: "PI".into(),
            provider: "pi_cli".into(),
            model: Some("openai/gpt-oss-20b".into()),
            command: "pi".into(),
            args: vec![],
            env: serde_json::json!({
                "NVIDIA_API_KEY": "nim-key",
                "NVIDIA_NIM_BASE_URL": "https://nim.example.test/v1"
            }),
            capabilities: vec!["can_edit".into()],
            transport_kind: "pipe".into(),
            active: true,
            is_reviewer: false,
            created_at: "unix:1".into(),
            updated_at: "unix:1".into(),
        };

        let bundle = prepare_agent_harness_bundle(
            dir.path(),
            &mission,
            &objective,
            "session-pi",
            &profile,
            &worktree.display().to_string(),
        )
        .expect("bundle");

        let config_dir = worktree.join(".sloposcope").join("pi-agent");
        assert_eq!(bundle.pi_config_dir, config_dir.display().to_string());
        let config: serde_yaml::Value =
            serde_yaml::from_slice(&fs::read(config_dir.join("config.yml")).expect("config"))
                .expect("yaml");
        let models: serde_yaml::Value =
            serde_yaml::from_slice(&fs::read(config_dir.join("models.yml")).expect("models"))
                .expect("yaml");
        assert_eq!(
            bundle.pi_config_path,
            config_dir.join("config.yml").display().to_string()
        );
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
                .and_then(|value| value.get(serde_yaml::Value::String("baseUrl".to_string())))
                .and_then(serde_yaml::Value::as_str),
            Some("https://nim.example.test/v1")
        );
    }
}

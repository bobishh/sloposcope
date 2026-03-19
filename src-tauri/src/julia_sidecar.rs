use crate::graph_store::{CanonicalClusterMember, CanonicalGraphSnapshot};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Serialize)]
struct JuliaClusterInput<'a> {
    entities: Vec<JuliaEntityInput<'a>>,
    edges: Vec<JuliaEdgeInput<'a>>,
}

#[derive(Debug, Serialize)]
struct JuliaEntityInput<'a> {
    entity_id: &'a str,
    kind: &'a str,
    file_path: &'a str,
    language: &'a str,
    display_name: &'a str,
}

#[derive(Debug, Serialize)]
struct JuliaEdgeInput<'a> {
    src_id: &'a str,
    dst_id: &'a str,
    kind: &'a str,
    confidence: f64,
}

#[derive(Debug, Deserialize)]
struct JuliaClusterOutput {
    entity_id: String,
    cluster_id: String,
    #[serde(default)]
    label: Option<String>,
}

pub fn cluster_snapshot(
    repo: &Path,
    snapshot: &CanonicalGraphSnapshot,
    julia_bin: Option<&str>,
) -> Result<Vec<CanonicalClusterMember>, String> {
    let script_path = cluster_script_path();
    if !script_path.exists() {
        return Err(format!(
            "Julia clustering script missing at {}",
            script_path.display()
        ));
    }

    let project_path = cluster_project_path();
    let payload = JuliaClusterInput {
        entities: snapshot
            .entities
            .iter()
            .map(|entity| JuliaEntityInput {
                entity_id: &entity.id,
                kind: crate::graph_store::canonical_entity_kind_label(&entity.kind),
                file_path: &entity.file_path,
                language: &entity.language,
                display_name: &entity.display_name,
            })
            .collect(),
        edges: snapshot
            .edges
            .iter()
            .map(|edge| JuliaEdgeInput {
                src_id: &edge.src_id,
                dst_id: &edge.dst_id,
                kind: &edge.kind,
                confidence: edge.confidence,
            })
            .collect(),
    };

    let input = serde_json::to_vec(&payload)
        .map_err(|err| format!("Failed to serialize clustering payload: {err}"))?;

    let julia = julia_bin
        .map(str::to_string)
        .or_else(|| std::env::var("SLOPOSCOPE_JULIA_BIN").ok())
        .or_else(|| std::env::var("EYELOSS_JULIA_BIN").ok())
        .unwrap_or_else(|| "julia".to_string());

    let mut child = Command::new(&julia)
        .arg(format!("--project={}", project_path.display()))
        .arg(&script_path)
        .arg(repo.display().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Failed to spawn Julia clustering sidecar `{julia}`: {err}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&input)
            .and_then(|_| stdin.flush())
            .map_err(|err| format!("Failed to write clustering input to Julia sidecar: {err}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|err| format!("Failed waiting for Julia clustering sidecar: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!(
            "Julia clustering sidecar failed with status {}{}",
            output.status,
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("Julia clustering output is not valid UTF-8: {err}"))?;
    let parsed: Vec<JuliaClusterOutput> = serde_json::from_str(stdout.trim())
        .map_err(|err| format!("Failed to parse Julia clustering output: {err}"))?;

    Ok(parsed
        .into_iter()
        .map(|member| CanonicalClusterMember {
            cluster_id: member.cluster_id,
            entity_id: member.entity_id,
            label: member.label,
        })
        .collect())
}

fn cluster_script_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("scripts")
        .join("cluster.jl")
}

fn cluster_project_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("scripts")
}

use crate::graph::{Func, Graph, Node};
use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const GRAPH_SCHEMA_VERSION: i32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalEntityKind {
    File,
    Module,
    Class,
    Function,
    Method,
    Constant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalEntity {
    pub id: String,
    pub kind: CanonicalEntityKind,
    pub name: String,
    pub display_name: String,
    pub file_path: String,
    pub language: String,
    pub parent_id: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub line_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalEdge {
    pub src_id: String,
    pub dst_id: String,
    pub kind: String,
    pub confidence: f64,
    pub source_file: String,
}

#[derive(Debug, Clone, Default)]
pub struct CanonicalGraphSnapshot {
    pub entities: Vec<CanonicalEntity>,
    pub edges: Vec<CanonicalEdge>,
    pub repo_meta: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalClusterMember {
    pub cluster_id: String,
    pub entity_id: String,
    pub label: Option<String>,
}

pub trait GraphStore {
    fn replace_all(&self, snapshot: &CanonicalGraphSnapshot) -> Result<(), String>;
    fn remove_file(&self, file_path: &str) -> Result<(), String>;
    fn upsert_file(
        &self,
        entities: &[CanonicalEntity],
        edges: &[CanonicalEdge],
        repo_meta: Option<&HashMap<String, String>>,
    ) -> Result<(), String>;
    fn load_snapshot(&self) -> Result<CanonicalGraphSnapshot, String>;
    fn replace_clusters(
        &self,
        members: &[CanonicalClusterMember],
        mode: &str,
    ) -> Result<(), String>;
    fn load_clusters(&self) -> Result<Vec<CanonicalClusterMember>, String>;
}

#[derive(Debug, Clone)]
pub struct SqliteGraphStore {
    db_path: PathBuf,
}

impl SqliteGraphStore {
    pub fn db_path(repo: &Path) -> PathBuf {
        repo.join(".sloposcope").join("graph.sqlite3")
    }

    pub fn open_or_create(repo: &Path) -> Result<Self, String> {
        let dir = repo.join(".sloposcope");
        fs::create_dir_all(&dir).map_err(|err| {
            format!(
                "Failed to create .sloposcope dir at {}: {err}",
                dir.display()
            )
        })?;
        let store = Self {
            db_path: Self::db_path(repo),
        };
        let mut conn = store.open_connection()?;
        store.ensure_schema(&mut conn)?;
        Ok(store)
    }

    fn open_connection(&self) -> Result<Connection, String> {
        let conn = Connection::open(&self.db_path).map_err(|err| {
            format!(
                "Failed to open graph store at {}: {err}",
                self.db_path.display()
            )
        })?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|err| format!("Failed to enable foreign_keys: {err}"))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|err| format!("Failed to enable WAL mode: {err}"))?;
        Ok(conn)
    }

    fn ensure_schema(&self, conn: &mut Connection) -> Result<(), String> {
        let current_version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|err| format!("Failed to read graph store schema version: {err}"))?;

        if current_version > GRAPH_SCHEMA_VERSION {
            return Err(format!(
                "Graph store schema version {} is newer than supported version {}",
                current_version, GRAPH_SCHEMA_VERSION
            ));
        }

        let tx = conn
            .transaction()
            .map_err(|err| format!("Failed to start schema transaction: {err}"))?;
        tx.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS entity (
              id TEXT PRIMARY KEY,
              kind TEXT NOT NULL,
              name TEXT NOT NULL,
              display_name TEXT NOT NULL,
              file_path TEXT NOT NULL,
              language TEXT NOT NULL,
              parent_id TEXT NULL,
              start_line INTEGER NULL,
              end_line INTEGER NULL,
              line_count INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS edge (
              src_id TEXT NOT NULL,
              dst_id TEXT NOT NULL,
              kind TEXT NOT NULL,
              confidence REAL NOT NULL,
              source_file TEXT NOT NULL,
              PRIMARY KEY (src_id, dst_id, kind, source_file)
            );
            CREATE TABLE IF NOT EXISTS repo_meta (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS cluster_member (
              cluster_id TEXT NOT NULL,
              entity_id TEXT NOT NULL,
              label TEXT NULL,
              PRIMARY KEY (cluster_id, entity_id)
            );
            CREATE INDEX IF NOT EXISTS idx_entity_file_path ON entity(file_path);
            CREATE INDEX IF NOT EXISTS idx_entity_parent_id ON entity(parent_id);
            CREATE INDEX IF NOT EXISTS idx_edge_src_id ON edge(src_id);
            CREATE INDEX IF NOT EXISTS idx_edge_dst_id ON edge(dst_id);
            CREATE INDEX IF NOT EXISTS idx_edge_source_file ON edge(source_file);
            ",
        )
        .map_err(|err| format!("Failed to initialize graph store schema: {err}"))?;
        tx.pragma_update(None, "user_version", GRAPH_SCHEMA_VERSION)
            .map_err(|err| format!("Failed to update graph store schema version: {err}"))?;
        tx.commit()
            .map_err(|err| format!("Failed to commit graph store schema: {err}"))
    }

    fn write_snapshot(
        &self,
        tx: &Transaction<'_>,
        entities: &[CanonicalEntity],
        edges: &[CanonicalEdge],
        repo_meta: &HashMap<String, String>,
    ) -> Result<(), String> {
        {
            let mut stmt = tx
                .prepare(
                    "
                    INSERT INTO entity (
                      id, kind, name, display_name, file_path, language, parent_id, start_line, end_line, line_count
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                    ON CONFLICT(id) DO UPDATE SET
                      kind = excluded.kind,
                      name = excluded.name,
                      display_name = excluded.display_name,
                      file_path = excluded.file_path,
                      language = excluded.language,
                      parent_id = excluded.parent_id,
                      start_line = excluded.start_line,
                      end_line = excluded.end_line,
                      line_count = excluded.line_count
                    ",
                )
                .map_err(|err| format!("Failed to prepare entity upsert: {err}"))?;
            for entity in entities {
                stmt.execute(params![
                    entity.id,
                    canonical_entity_kind_label(&entity.kind),
                    entity.name,
                    entity.display_name,
                    entity.file_path,
                    entity.language,
                    entity.parent_id,
                    entity.start_line.map(i64::from),
                    entity.end_line.map(i64::from),
                    i64::from(entity.line_count),
                ])
                .map_err(|err| format!("Failed to upsert entity {}: {err}", entity.id))?;
            }
        }

        {
            let mut stmt = tx
                .prepare(
                    "
                    INSERT INTO edge (src_id, dst_id, kind, confidence, source_file)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    ON CONFLICT(src_id, dst_id, kind, source_file) DO UPDATE SET
                      confidence = excluded.confidence
                    ",
                )
                .map_err(|err| format!("Failed to prepare edge upsert: {err}"))?;
            for edge in edges {
                stmt.execute(params![
                    edge.src_id,
                    edge.dst_id,
                    edge.kind,
                    edge.confidence,
                    edge.source_file,
                ])
                .map_err(|err| {
                    format!(
                        "Failed to upsert edge {} -> {} ({}): {err}",
                        edge.src_id, edge.dst_id, edge.kind
                    )
                })?;
            }
        }

        {
            let mut stmt = tx
                .prepare(
                    "
                    INSERT INTO repo_meta (key, value)
                    VALUES (?1, ?2)
                    ON CONFLICT(key) DO UPDATE SET value = excluded.value
                    ",
                )
                .map_err(|err| format!("Failed to prepare repo_meta upsert: {err}"))?;
            for (key, value) in repo_meta {
                stmt.execute(params![key, value])
                    .map_err(|err| format!("Failed to upsert repo_meta {}: {err}", key))?;
            }
        }

        Ok(())
    }

    fn remove_file_tx(tx: &Transaction<'_>, file_path: &str) -> Result<(), String> {
        let mut entity_ids = Vec::new();
        {
            let mut stmt = tx
                .prepare("SELECT id FROM entity WHERE file_path = ?1")
                .map_err(|err| format!("Failed to prepare entity id lookup: {err}"))?;
            let rows = stmt
                .query_map(params![file_path], |row| row.get::<_, String>(0))
                .map_err(|err| format!("Failed to query entity ids for {file_path}: {err}"))?;
            for row in rows {
                entity_ids
                    .push(row.map_err(|err| format!("Failed to decode entity id row: {err}"))?);
            }
        }

        tx.execute(
            "DELETE FROM edge WHERE source_file = ?1",
            params![file_path],
        )
        .map_err(|err| format!("Failed to delete edges for {file_path}: {err}"))?;

        for entity_id in &entity_ids {
            tx.execute(
                "DELETE FROM edge WHERE src_id = ?1 OR dst_id = ?1",
                params![entity_id],
            )
            .map_err(|err| format!("Failed to delete edges for entity {entity_id}: {err}"))?;
        }

        tx.execute(
            "DELETE FROM entity WHERE file_path = ?1",
            params![file_path],
        )
        .map_err(|err| format!("Failed to delete entities for {file_path}: {err}"))?;
        Ok(())
    }
}

impl GraphStore for SqliteGraphStore {
    fn replace_all(&self, snapshot: &CanonicalGraphSnapshot) -> Result<(), String> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction()
            .map_err(|err| format!("Failed to start replace_all transaction: {err}"))?;
        tx.execute_batch(
            "
            DELETE FROM edge;
            DELETE FROM entity;
            DELETE FROM repo_meta;
            DELETE FROM cluster_member;
            ",
        )
        .map_err(|err| format!("Failed to clear graph store: {err}"))?;
        self.write_snapshot(
            &tx,
            &snapshot.entities,
            &snapshot.edges,
            &snapshot.repo_meta,
        )?;
        tx.commit()
            .map_err(|err| format!("Failed to commit replace_all transaction: {err}"))
    }

    fn remove_file(&self, file_path: &str) -> Result<(), String> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction()
            .map_err(|err| format!("Failed to start remove_file transaction: {err}"))?;
        Self::remove_file_tx(&tx, file_path)?;
        tx.commit()
            .map_err(|err| format!("Failed to commit remove_file transaction: {err}"))
    }

    fn upsert_file(
        &self,
        entities: &[CanonicalEntity],
        edges: &[CanonicalEdge],
        repo_meta: Option<&HashMap<String, String>>,
    ) -> Result<(), String> {
        let file_path = entities
            .first()
            .map(|entity| entity.file_path.as_str())
            .or_else(|| edges.first().map(|edge| edge.source_file.as_str()))
            .ok_or_else(|| "upsert_file called without entities or edges".to_string())?;
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction()
            .map_err(|err| format!("Failed to start upsert_file transaction: {err}"))?;
        Self::remove_file_tx(&tx, file_path)?;
        self.write_snapshot(
            &tx,
            entities,
            edges,
            &repo_meta.cloned().unwrap_or_default(),
        )?;
        tx.commit()
            .map_err(|err| format!("Failed to commit upsert_file transaction: {err}"))
    }

    fn load_snapshot(&self) -> Result<CanonicalGraphSnapshot, String> {
        let conn = self.open_connection()?;
        let mut snapshot = CanonicalGraphSnapshot::default();

        {
            let mut stmt = conn
                .prepare(
                    "
                    SELECT id, kind, name, display_name, file_path, language, parent_id, start_line, end_line, line_count
                    FROM entity
                    ORDER BY file_path, id
                    ",
                )
                .map_err(|err| format!("Failed to prepare entity load: {err}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(CanonicalEntity {
                        id: row.get(0)?,
                        kind: parse_canonical_entity_kind(&row.get::<_, String>(1)?),
                        name: row.get(2)?,
                        display_name: row.get(3)?,
                        file_path: row.get(4)?,
                        language: row.get(5)?,
                        parent_id: row.get(6)?,
                        start_line: row.get::<_, Option<i64>>(7)?.map(|value| value as u32),
                        end_line: row.get::<_, Option<i64>>(8)?.map(|value| value as u32),
                        line_count: row.get::<_, i64>(9)? as u32,
                    })
                })
                .map_err(|err| format!("Failed to query entities: {err}"))?;
            for row in rows {
                snapshot
                    .entities
                    .push(row.map_err(|err| format!("Failed to decode entity row: {err}"))?);
            }
        }

        {
            let mut stmt = conn
                .prepare(
                    "
                    SELECT src_id, dst_id, kind, confidence, source_file
                    FROM edge
                    ORDER BY source_file, src_id, dst_id, kind
                    ",
                )
                .map_err(|err| format!("Failed to prepare edge load: {err}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(CanonicalEdge {
                        src_id: row.get(0)?,
                        dst_id: row.get(1)?,
                        kind: row.get(2)?,
                        confidence: row.get(3)?,
                        source_file: row.get(4)?,
                    })
                })
                .map_err(|err| format!("Failed to query edges: {err}"))?;
            for row in rows {
                snapshot
                    .edges
                    .push(row.map_err(|err| format!("Failed to decode edge row: {err}"))?);
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT key, value FROM repo_meta ORDER BY key")
                .map_err(|err| format!("Failed to prepare repo_meta load: {err}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|err| format!("Failed to query repo_meta: {err}"))?;
            for row in rows {
                let (key, value) =
                    row.map_err(|err| format!("Failed to decode repo_meta row: {err}"))?;
                snapshot.repo_meta.insert(key, value);
            }
        }

        Ok(snapshot)
    }

    fn replace_clusters(
        &self,
        members: &[CanonicalClusterMember],
        mode: &str,
    ) -> Result<(), String> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction()
            .map_err(|err| format!("Failed to start replace_clusters transaction: {err}"))?;
        tx.execute("DELETE FROM cluster_member", [])
            .map_err(|err| format!("Failed to clear cluster members: {err}"))?;

        {
            let mut stmt = tx
                .prepare(
                    "
                    INSERT INTO cluster_member (cluster_id, entity_id, label)
                    VALUES (?1, ?2, ?3)
                    ON CONFLICT(cluster_id, entity_id) DO UPDATE SET
                      label = excluded.label
                    ",
                )
                .map_err(|err| format!("Failed to prepare cluster member upsert: {err}"))?;
            for member in members {
                stmt.execute(params![member.cluster_id, member.entity_id, member.label])
                    .map_err(|err| {
                        format!(
                            "Failed to upsert cluster member {} -> {}: {err}",
                            member.cluster_id, member.entity_id
                        )
                    })?;
            }
        }

        {
            let mut stmt = tx
                .prepare(
                    "
                    INSERT INTO repo_meta (key, value)
                    VALUES (?1, ?2)
                    ON CONFLICT(key) DO UPDATE SET value = excluded.value
                    ",
                )
                .map_err(|err| format!("Failed to prepare clustering repo_meta upsert: {err}"))?;
            stmt.execute(params!["clustering_mode", mode])
                .map_err(|err| format!("Failed to upsert clustering_mode: {err}"))?;
            stmt.execute(params!["clustering_updated_at", unix_now_label()])
                .map_err(|err| format!("Failed to upsert clustering_updated_at: {err}"))?;
        }

        tx.commit()
            .map_err(|err| format!("Failed to commit replace_clusters transaction: {err}"))
    }

    fn load_clusters(&self) -> Result<Vec<CanonicalClusterMember>, String> {
        let conn = self.open_connection()?;
        let mut stmt = conn
            .prepare(
                "
                SELECT cluster_id, entity_id, label
                FROM cluster_member
                ORDER BY cluster_id, entity_id
                ",
            )
            .map_err(|err| format!("Failed to prepare cluster member load: {err}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CanonicalClusterMember {
                    cluster_id: row.get(0)?,
                    entity_id: row.get(1)?,
                    label: row.get(2)?,
                })
            })
            .map_err(|err| format!("Failed to query cluster members: {err}"))?;
        let mut members = Vec::new();
        for row in rows {
            members.push(row.map_err(|err| format!("Failed to decode cluster member row: {err}"))?);
        }
        Ok(members)
    }
}

pub fn snapshot_from_graph(
    repo: &Path,
    graph: &Graph,
    checkout_revision: &str,
    build_fingerprint: u64,
) -> CanonicalGraphSnapshot {
    let mut entities = Vec::new();
    let mut edges = Vec::new();
    let mut seen_function_ids = HashSet::new();

    for node in &graph.nodes {
        entities.push(CanonicalEntity {
            id: node.id.clone(),
            kind: normalize_node_kind(&node.kind),
            name: node.id.clone(),
            display_name: node.label.clone(),
            file_path: node.file.clone(),
            language: language_from_path(&node.file),
            parent_id: None,
            start_line: (node.line_count > 0).then_some(1),
            end_line: (node.line_count > 0).then_some(node.line_count as u32),
            line_count: node.line_count as u32,
        });

        for (index, func) in node.functions.iter().enumerate() {
            let func_id = function_entity_id(node, func, index);
            if !seen_function_ids.insert(func_id.clone()) {
                continue;
            }
            entities.push(CanonicalEntity {
                id: func_id.clone(),
                kind: normalize_function_kind(&func.kind),
                name: func.name.clone(),
                display_name: func.name.clone(),
                file_path: node.file.clone(),
                language: language_from_path(&node.file),
                parent_id: Some(node.id.clone()),
                start_line: None,
                end_line: None,
                line_count: 0,
            });
            edges.push(CanonicalEdge {
                src_id: node.id.clone(),
                dst_id: func_id,
                kind: "contains".to_string(),
                confidence: 1.0,
                source_file: node.file.clone(),
            });
        }
    }

    for edge in &graph.edges {
        let source_file = graph
            .nodes
            .iter()
            .find(|node| node.id == edge.source)
            .map(|node| node.file.clone())
            .unwrap_or_default();
        edges.push(CanonicalEdge {
            src_id: edge.source.clone(),
            dst_id: edge.target.clone(),
            kind: normalize_edge_kind(&edge.kind),
            confidence: edge_confidence(&edge.kind),
            source_file,
        });
    }

    let mut repo_meta = HashMap::new();
    repo_meta.insert("repo_path".to_string(), repo.display().to_string());
    repo_meta.insert(
        "checkout_revision".to_string(),
        checkout_revision.to_string(),
    );
    repo_meta.insert(
        "build_fingerprint".to_string(),
        build_fingerprint.to_string(),
    );
    repo_meta.insert("built_at".to_string(), unix_now_label());

    CanonicalGraphSnapshot {
        entities,
        edges,
        repo_meta,
    }
}

pub fn function_entity_id(node: &Node, func: &Func, index: usize) -> String {
    format!("{}#{}:{}:{}", node.id, func.kind, func.name, index)
}

fn normalize_node_kind(kind: &str) -> CanonicalEntityKind {
    match kind {
        "class" => CanonicalEntityKind::Class,
        "constant" => CanonicalEntityKind::Constant,
        "file" => CanonicalEntityKind::File,
        _ => CanonicalEntityKind::Module,
    }
}

fn normalize_function_kind(kind: &str) -> CanonicalEntityKind {
    if kind.eq_ignore_ascii_case("method") {
        CanonicalEntityKind::Method
    } else {
        CanonicalEntityKind::Function
    }
}

fn normalize_edge_kind(kind: &str) -> String {
    match kind {
        "alias" => "reference".to_string(),
        "require" => "use".to_string(),
        other => other.to_string(),
    }
}

fn edge_confidence(kind: &str) -> f64 {
    match kind {
        "contains" => 1.0,
        "extends" => 0.95,
        "call" | "calls" => 0.9,
        "inherits" | "implements" => 0.85,
        "import" => 0.8,
        "use" | "using" | "include" => 0.75,
        "reference" | "alias" => 0.6,
        "inferred_call" => 0.5,
        _ => 0.5,
    }
}

pub fn canonical_entity_kind_label(kind: &CanonicalEntityKind) -> &'static str {
    match kind {
        CanonicalEntityKind::File => "file",
        CanonicalEntityKind::Module => "module",
        CanonicalEntityKind::Class => "class",
        CanonicalEntityKind::Function => "function",
        CanonicalEntityKind::Method => "method",
        CanonicalEntityKind::Constant => "constant",
    }
}

pub fn parse_canonical_entity_kind(value: &str) -> CanonicalEntityKind {
    match value {
        "file" => CanonicalEntityKind::File,
        "class" => CanonicalEntityKind::Class,
        "function" => CanonicalEntityKind::Function,
        "method" => CanonicalEntityKind::Method,
        "constant" => CanonicalEntityKind::Constant,
        _ => CanonicalEntityKind::Module,
    }
}

fn language_from_path(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "ex" | "exs" => "elixir",
        "rb" => "ruby",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "py" => "python",
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "h" => "cpp",
        "php" => "php",
        "cs" => "csharp",
        "clj" | "cljs" | "cljc" => "clojure",
        "kt" | "kts" => "kotlin",
        "json" => "json",
        "md" => "markdown",
        "svelte" => "svelte",
        "" => "unknown",
        _ => ext.as_str(),
    }
    .to_string()
}

fn unix_now_label() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, Graph, Node};

    #[test]
    fn snapshot_from_graph_creates_entities_and_contains_edges() {
        let graph = Graph {
            nodes: vec![Node {
                id: "src/lib.rs".to_string(),
                label: "lib.rs".to_string(),
                kind: "file".to_string(),
                file: "src/lib.rs".to_string(),
                line_count: 10,
                change_status: "modified".to_string(),
                functions: vec![Func {
                    name: "run".to_string(),
                    arity: 0,
                    kind: "function".to_string(),
                }],
            }],
            edges: vec![Edge {
                source: "src/lib.rs".to_string(),
                target: "src/main.rs".to_string(),
                kind: "call".to_string(),
            }],
        };
        let snapshot = snapshot_from_graph(Path::new("/tmp/repo"), &graph, "abc123", 42);
        assert_eq!(snapshot.entities.len(), 2);
        assert_eq!(snapshot.edges.len(), 2);
        assert!(snapshot
            .edges
            .iter()
            .any(|edge| edge.kind == "contains" && edge.src_id == "src/lib.rs"));
        assert_eq!(
            snapshot
                .repo_meta
                .get("checkout_revision")
                .map(String::as_str),
            Some("abc123")
        );
    }

    #[test]
    fn sqlite_store_roundtrips_and_replaces_file_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteGraphStore::open_or_create(dir.path()).expect("open store");
        let snapshot = CanonicalGraphSnapshot {
            entities: vec![
                CanonicalEntity {
                    id: "src/App.svelte".to_string(),
                    kind: CanonicalEntityKind::File,
                    name: "src/App.svelte".to_string(),
                    display_name: "App.svelte".to_string(),
                    file_path: "src/App.svelte".to_string(),
                    language: "svelte".to_string(),
                    parent_id: None,
                    start_line: Some(1),
                    end_line: Some(20),
                    line_count: 20,
                },
                CanonicalEntity {
                    id: "src/lib/backendBridge.js".to_string(),
                    kind: CanonicalEntityKind::File,
                    name: "src/lib/backendBridge.js".to_string(),
                    display_name: "backendBridge.js".to_string(),
                    file_path: "src/lib/backendBridge.js".to_string(),
                    language: "javascript".to_string(),
                    parent_id: None,
                    start_line: Some(1),
                    end_line: Some(10),
                    line_count: 10,
                },
            ],
            edges: vec![CanonicalEdge {
                src_id: "src/App.svelte".to_string(),
                dst_id: "src/lib/backendBridge.js".to_string(),
                kind: "import".to_string(),
                confidence: 0.8,
                source_file: "src/App.svelte".to_string(),
            }],
            repo_meta: HashMap::from([
                ("repo_path".to_string(), dir.path().display().to_string()),
                ("checkout_revision".to_string(), "headless".to_string()),
            ]),
        };
        store.replace_all(&snapshot).expect("replace all");

        let replacement_meta = HashMap::from([
            ("repo_path".to_string(), dir.path().display().to_string()),
            ("checkout_revision".to_string(), "updated".to_string()),
        ]);
        store
            .upsert_file(
                &[CanonicalEntity {
                    id: "src/App.svelte".to_string(),
                    kind: CanonicalEntityKind::File,
                    name: "src/App.svelte".to_string(),
                    display_name: "AppRenamed.svelte".to_string(),
                    file_path: "src/App.svelte".to_string(),
                    language: "svelte".to_string(),
                    parent_id: None,
                    start_line: Some(1),
                    end_line: Some(30),
                    line_count: 30,
                }],
                &[],
                Some(&replacement_meta),
            )
            .expect("upsert file");

        let loaded = store.load_snapshot().expect("load snapshot");
        assert_eq!(loaded.entities.len(), 2);
        assert_eq!(
            loaded
                .entities
                .iter()
                .find(|entity| entity.file_path == "src/App.svelte")
                .map(|entity| entity.display_name.as_str()),
            Some("AppRenamed.svelte")
        );
        assert_eq!(loaded.edges.len(), 0);
        assert_eq!(
            loaded
                .repo_meta
                .get("checkout_revision")
                .map(String::as_str),
            Some("updated")
        );
    }

    #[test]
    fn sqlite_store_roundtrips_cluster_assignments() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteGraphStore::open_or_create(dir.path()).expect("open store");
        let snapshot = CanonicalGraphSnapshot {
            entities: vec![CanonicalEntity {
                id: "src/App.svelte".to_string(),
                kind: CanonicalEntityKind::File,
                name: "src/App.svelte".to_string(),
                display_name: "App.svelte".to_string(),
                file_path: "src/App.svelte".to_string(),
                language: "svelte".to_string(),
                parent_id: None,
                start_line: Some(1),
                end_line: Some(20),
                line_count: 20,
            }],
            edges: vec![],
            repo_meta: HashMap::new(),
        };
        store.replace_all(&snapshot).expect("replace");
        store
            .replace_clusters(
                &[CanonicalClusterMember {
                    cluster_id: "cluster-1".to_string(),
                    entity_id: "src/App.svelte".to_string(),
                    label: Some("ui".to_string()),
                }],
                "persisted",
            )
            .expect("replace clusters");

        let clusters = store.load_clusters().expect("load clusters");
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].cluster_id, "cluster-1");
        assert_eq!(clusters[0].entity_id, "src/App.svelte");
        assert_eq!(clusters[0].label.as_deref(), Some("ui"));

        let loaded = store.load_snapshot().expect("load snapshot");
        assert_eq!(
            loaded.repo_meta.get("clustering_mode").map(String::as_str),
            Some("persisted")
        );
    }
}

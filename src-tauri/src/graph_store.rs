use crate::graph::{Func, Graph, Node};
use crate::program_model::{
    filesystem_file_id, program_model_from_graph, syntax_entity_kind_label, EvidenceSource,
    FilesystemEntry, FilesystemEntryKind, FilesystemHierarchy, ProgramModelSnapshot,
    StaticRelation, SyntaxEntity, SyntaxEntityKind, SyntaxHierarchy,
};
use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const GRAPH_SCHEMA_VERSION: i32 = 2;

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
            CREATE TABLE IF NOT EXISTS filesystem_entry (
              id TEXT PRIMARY KEY,
              kind TEXT NOT NULL,
              path TEXT NOT NULL,
              name TEXT NOT NULL,
              display_name TEXT NOT NULL,
              parent_id TEXT NULL,
              line_count INTEGER NOT NULL,
              language TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS syntax_entity (
              id TEXT PRIMARY KEY,
              kind TEXT NOT NULL,
              name TEXT NOT NULL,
              display_name TEXT NOT NULL,
              file_id TEXT NOT NULL,
              file_path TEXT NOT NULL,
              parent_id TEXT NULL,
              start_line INTEGER NULL,
              end_line INTEGER NULL,
              line_count INTEGER NOT NULL,
              parameter_count INTEGER NULL
            );
            CREATE TABLE IF NOT EXISTS static_relation (
              src_id TEXT NOT NULL,
              dst_id TEXT NOT NULL,
              kind TEXT NOT NULL,
              confidence REAL NOT NULL,
              source_file TEXT NOT NULL,
              evidence TEXT NOT NULL,
              PRIMARY KEY (src_id, dst_id, kind, source_file, evidence)
            );
            CREATE INDEX IF NOT EXISTS idx_entity_file_path ON entity(file_path);
            CREATE INDEX IF NOT EXISTS idx_entity_parent_id ON entity(parent_id);
            CREATE INDEX IF NOT EXISTS idx_edge_src_id ON edge(src_id);
            CREATE INDEX IF NOT EXISTS idx_edge_dst_id ON edge(dst_id);
            CREATE INDEX IF NOT EXISTS idx_edge_source_file ON edge(source_file);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_filesystem_entry_path_kind ON filesystem_entry(path, kind);
            CREATE INDEX IF NOT EXISTS idx_filesystem_entry_parent_id ON filesystem_entry(parent_id);
            CREATE INDEX IF NOT EXISTS idx_syntax_entity_file_path ON syntax_entity(file_path);
            CREATE INDEX IF NOT EXISTS idx_syntax_entity_parent_id ON syntax_entity(parent_id);
            CREATE INDEX IF NOT EXISTS idx_static_relation_src_id ON static_relation(src_id);
            CREATE INDEX IF NOT EXISTS idx_static_relation_dst_id ON static_relation(dst_id);
            CREATE INDEX IF NOT EXISTS idx_static_relation_source_file ON static_relation(source_file);
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

    fn write_program_model(
        &self,
        tx: &Transaction<'_>,
        model: &ProgramModelSnapshot,
    ) -> Result<(), String> {
        {
            let mut stmt = tx
                .prepare(
                    "
                    INSERT INTO filesystem_entry (
                      id, kind, path, name, display_name, parent_id, line_count, language
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                    ON CONFLICT(id) DO UPDATE SET
                      kind = excluded.kind,
                      path = excluded.path,
                      name = excluded.name,
                      display_name = excluded.display_name,
                      parent_id = excluded.parent_id,
                      line_count = excluded.line_count,
                      language = excluded.language
                    ",
                )
                .map_err(|err| format!("Failed to prepare filesystem entry upsert: {err}"))?;
            for entry in &model.filesystem.entries {
                stmt.execute(params![
                    entry.id,
                    filesystem_entry_kind_label(&entry.kind),
                    entry.path,
                    entry.name,
                    entry.display_name,
                    entry.parent_id,
                    i64::from(entry.line_count),
                    entry.language,
                ])
                .map_err(|err| format!("Failed to upsert filesystem entry {}: {err}", entry.id))?;
            }
        }

        {
            let mut stmt = tx
                .prepare(
                    "
                    INSERT INTO syntax_entity (
                      id, kind, name, display_name, file_id, file_path, parent_id,
                      start_line, end_line, line_count, parameter_count
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                    ON CONFLICT(id) DO UPDATE SET
                      kind = excluded.kind,
                      name = excluded.name,
                      display_name = excluded.display_name,
                      file_id = excluded.file_id,
                      file_path = excluded.file_path,
                      parent_id = excluded.parent_id,
                      start_line = excluded.start_line,
                      end_line = excluded.end_line,
                      line_count = excluded.line_count,
                      parameter_count = excluded.parameter_count
                    ",
                )
                .map_err(|err| format!("Failed to prepare syntax entity upsert: {err}"))?;
            for entity in &model.syntax.entities {
                stmt.execute(params![
                    entity.id,
                    syntax_entity_kind_label(&entity.kind),
                    entity.name,
                    entity.display_name,
                    entity.file_id,
                    entity.file_path,
                    entity.parent_id,
                    entity.start_line.map(i64::from),
                    entity.end_line.map(i64::from),
                    i64::from(entity.line_count),
                    entity.parameter_count.map(|value| value as i64),
                ])
                .map_err(|err| format!("Failed to upsert syntax entity {}: {err}", entity.id))?;
            }
        }

        {
            let mut stmt = tx
                .prepare(
                    "
                    INSERT INTO static_relation (
                      src_id, dst_id, kind, confidence, source_file, evidence
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                    ON CONFLICT(src_id, dst_id, kind, source_file, evidence) DO UPDATE SET
                      confidence = excluded.confidence
                    ",
                )
                .map_err(|err| format!("Failed to prepare static relation upsert: {err}"))?;
            for relation in &model.static_relations {
                stmt.execute(params![
                    relation.src_id,
                    relation.dst_id,
                    relation.kind,
                    relation.confidence,
                    relation.source_file,
                    evidence_source_label(&relation.evidence),
                ])
                .map_err(|err| {
                    format!(
                        "Failed to upsert static relation {} -> {}: {err}",
                        relation.src_id, relation.dst_id
                    )
                })?;
            }
        }

        Ok(())
    }

    pub fn replace_program_model(&self, model: &ProgramModelSnapshot) -> Result<(), String> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction()
            .map_err(|err| format!("Failed to start replace_program_model transaction: {err}"))?;
        tx.execute_batch(
            "
            DELETE FROM static_relation;
            DELETE FROM syntax_entity;
            DELETE FROM filesystem_entry;
            DELETE FROM edge;
            DELETE FROM entity;
            DELETE FROM repo_meta;
            DELETE FROM cluster_member;
            ",
        )
        .map_err(|err| format!("Failed to clear program model store: {err}"))?;
        self.write_program_model(&tx, model)?;
        let legacy = canonical_snapshot_from_program_model(model);
        self.write_snapshot(&tx, &legacy.entities, &legacy.edges, &model.repo_meta)?;
        tx.commit()
            .map_err(|err| format!("Failed to commit replace_program_model transaction: {err}"))
    }

    pub fn load_program_model(&self) -> Result<ProgramModelSnapshot, String> {
        let conn = self.open_connection()?;
        let mut filesystem_entries = Vec::new();
        let mut syntax_entities = Vec::new();
        let mut static_relations = Vec::new();
        let mut repo_meta = HashMap::new();

        {
            let mut stmt = conn
                .prepare(
                    "
                    SELECT id, kind, path, name, display_name, parent_id, line_count, language
                    FROM filesystem_entry
                    ORDER BY id
                    ",
                )
                .map_err(|err| format!("Failed to prepare filesystem entry load: {err}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(FilesystemEntry {
                        id: row.get(0)?,
                        kind: parse_filesystem_entry_kind(&row.get::<_, String>(1)?),
                        path: row.get(2)?,
                        name: row.get(3)?,
                        display_name: row.get(4)?,
                        parent_id: row.get(5)?,
                        line_count: row.get::<_, i64>(6)? as u32,
                        language: row.get(7)?,
                    })
                })
                .map_err(|err| format!("Failed to query filesystem entries: {err}"))?;
            for row in rows {
                filesystem_entries
                    .push(row.map_err(|err| format!("Failed to decode filesystem entry: {err}"))?);
            }
        }

        {
            let mut stmt = conn
                .prepare(
                    "
                    SELECT id, kind, name, display_name, file_id, file_path, parent_id,
                           start_line, end_line, line_count, parameter_count
                    FROM syntax_entity
                    ORDER BY id
                    ",
                )
                .map_err(|err| format!("Failed to prepare syntax entity load: {err}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(SyntaxEntity {
                        id: row.get(0)?,
                        kind: parse_syntax_entity_kind(&row.get::<_, String>(1)?),
                        name: row.get(2)?,
                        display_name: row.get(3)?,
                        file_id: row.get(4)?,
                        file_path: row.get(5)?,
                        parent_id: row.get(6)?,
                        start_line: row.get::<_, Option<i64>>(7)?.map(|value| value as u32),
                        end_line: row.get::<_, Option<i64>>(8)?.map(|value| value as u32),
                        line_count: row.get::<_, i64>(9)? as u32,
                        parameter_count: row.get::<_, Option<i64>>(10)?.map(|value| value as usize),
                    })
                })
                .map_err(|err| format!("Failed to query syntax entities: {err}"))?;
            for row in rows {
                syntax_entities
                    .push(row.map_err(|err| format!("Failed to decode syntax entity: {err}"))?);
            }
        }

        {
            let mut stmt = conn
                .prepare(
                    "
                    SELECT src_id, dst_id, kind, confidence, source_file, evidence
                    FROM static_relation
                    ORDER BY src_id, dst_id, kind, source_file, evidence
                    ",
                )
                .map_err(|err| format!("Failed to prepare static relation load: {err}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(StaticRelation {
                        src_id: row.get(0)?,
                        dst_id: row.get(1)?,
                        kind: row.get(2)?,
                        confidence: row.get(3)?,
                        source_file: row.get(4)?,
                        evidence: parse_evidence_source(&row.get::<_, String>(5)?),
                    })
                })
                .map_err(|err| format!("Failed to query static relations: {err}"))?;
            for row in rows {
                static_relations
                    .push(row.map_err(|err| format!("Failed to decode static relation: {err}"))?);
            }
        }

        {
            let mut stmt = conn
                .prepare("SELECT key, value FROM repo_meta ORDER BY key")
                .map_err(|err| format!("Failed to prepare program repo_meta load: {err}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|err| format!("Failed to query program repo_meta: {err}"))?;
            for row in rows {
                let (key, value) =
                    row.map_err(|err| format!("Failed to decode program repo_meta: {err}"))?;
                repo_meta.insert(key, value);
            }
        }

        Ok(ProgramModelSnapshot {
            filesystem: FilesystemHierarchy {
                entries: filesystem_entries,
            },
            syntax: SyntaxHierarchy {
                entities: syntax_entities,
            },
            static_relations,
            repo_meta,
        })
    }

    pub fn upsert_program_file(
        &self,
        file_path: &str,
        fragment: &ProgramModelSnapshot,
    ) -> Result<(), String> {
        let scoped = ProgramModelSnapshot {
            filesystem: FilesystemHierarchy {
                entries: fragment
                    .filesystem
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.kind == FilesystemEntryKind::Directory || entry.path == file_path
                    })
                    .cloned()
                    .collect(),
            },
            syntax: SyntaxHierarchy {
                entities: fragment
                    .syntax
                    .entities
                    .iter()
                    .filter(|entity| entity.file_path == file_path)
                    .cloned()
                    .collect(),
            },
            static_relations: fragment
                .static_relations
                .iter()
                .filter(|relation| relation.source_file == file_path)
                .cloned()
                .collect(),
            repo_meta: fragment.repo_meta.clone(),
        };
        if !scoped
            .filesystem
            .entries
            .iter()
            .any(|entry| entry.kind == FilesystemEntryKind::File && entry.path == file_path)
        {
            return Err(format!(
                "upsert_program_file fragment has no filesystem row for {file_path}"
            ));
        }

        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction()
            .map_err(|err| format!("Failed to start upsert_program_file transaction: {err}"))?;
        Self::remove_program_file_rows_tx(&tx, file_path)?;
        Self::remove_file_tx(&tx, file_path)?;
        self.write_program_model(&tx, &scoped)?;

        let mut legacy = canonical_snapshot_from_program_model(fragment);
        legacy
            .entities
            .retain(|entity| entity.file_path == file_path);
        legacy.edges.retain(|edge| edge.source_file == file_path);
        self.write_snapshot(&tx, &legacy.entities, &legacy.edges, &fragment.repo_meta)?;
        Self::prune_empty_directories_tx(&tx)?;
        tx.commit()
            .map_err(|err| format!("Failed to commit upsert_program_file transaction: {err}"))
    }

    pub fn remove_program_file(&self, file_path: &str) -> Result<(), String> {
        let mut conn = self.open_connection()?;
        let tx = conn
            .transaction()
            .map_err(|err| format!("Failed to start remove_program_file transaction: {err}"))?;
        Self::remove_program_file_rows_tx(&tx, file_path)?;
        let file_id = filesystem_file_id(file_path);
        tx.execute(
            "DELETE FROM static_relation WHERE src_id = ?1 OR dst_id = ?1",
            params![file_id],
        )
        .map_err(|err| {
            format!("Failed to delete static relations pointing to {file_path}: {err}")
        })?;
        Self::remove_file_tx(&tx, file_path)?;
        Self::prune_empty_directories_tx(&tx)?;
        tx.commit()
            .map_err(|err| format!("Failed to commit remove_program_file transaction: {err}"))
    }

    fn remove_program_file_rows_tx(tx: &Transaction<'_>, file_path: &str) -> Result<(), String> {
        tx.execute(
            "DELETE FROM static_relation WHERE source_file = ?1",
            params![file_path],
        )
        .map_err(|err| format!("Failed to delete static relations for {file_path}: {err}"))?;
        tx.execute(
            "DELETE FROM syntax_entity WHERE file_path = ?1",
            params![file_path],
        )
        .map_err(|err| format!("Failed to delete syntax entities for {file_path}: {err}"))?;
        tx.execute(
            "DELETE FROM filesystem_entry WHERE kind = 'file' AND path = ?1",
            params![file_path],
        )
        .map_err(|err| format!("Failed to delete filesystem file {file_path}: {err}"))?;
        Ok(())
    }

    fn prune_empty_directories_tx(tx: &Transaction<'_>) -> Result<(), String> {
        loop {
            let removed = tx
                .execute(
                    "
                    DELETE FROM filesystem_entry AS directory
                    WHERE directory.kind = 'directory'
                      AND directory.id != 'fs-dir:.'
                      AND NOT EXISTS (
                        SELECT 1 FROM filesystem_entry AS child
                        WHERE child.parent_id = directory.id
                      )
                    ",
                    [],
                )
                .map_err(|err| format!("Failed to prune empty filesystem directories: {err}"))?;
            if removed == 0 {
                return Ok(());
            }
        }
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
    let model = program_model_from_graph(repo, graph, checkout_revision, build_fingerprint);
    canonical_snapshot_from_program_model(&model)
}

pub fn canonical_snapshot_from_program_model(
    model: &ProgramModelSnapshot,
) -> CanonicalGraphSnapshot {
    let roots_by_file = model
        .syntax
        .entities
        .iter()
        .filter(|entity| {
            entity.parent_id.is_none()
                && matches!(
                    entity.kind,
                    SyntaxEntityKind::Module | SyntaxEntityKind::Class | SyntaxEntityKind::Constant
                )
        })
        .map(|entity| (entity.file_id.as_str(), entity))
        .collect::<HashMap<_, _>>();
    let mut canonical_id_by_file_id = HashMap::new();
    let mut language_by_file_id = HashMap::new();
    let mut entities = Vec::new();
    let mut edges = Vec::new();

    for entry in model
        .filesystem
        .entries
        .iter()
        .filter(|entry| entry.kind == FilesystemEntryKind::File)
    {
        let root = roots_by_file.get(entry.id.as_str()).copied();
        let canonical_id = root
            .map(|entity| entity.name.clone())
            .unwrap_or_else(|| entry.path.clone());
        canonical_id_by_file_id.insert(entry.id.as_str(), canonical_id.clone());
        language_by_file_id.insert(entry.id.as_str(), entry.language.as_str());
        entities.push(CanonicalEntity {
            id: canonical_id.clone(),
            kind: root
                .map(|entity| canonical_kind_from_syntax(&entity.kind))
                .unwrap_or(CanonicalEntityKind::File),
            name: canonical_id,
            display_name: root
                .map(|entity| entity.display_name.clone())
                .unwrap_or_else(|| entry.display_name.clone()),
            file_path: entry.path.clone(),
            language: entry.language.clone(),
            parent_id: None,
            start_line: root
                .and_then(|entity| entity.start_line)
                .or_else(|| (entry.line_count > 0).then_some(1)),
            end_line: root
                .and_then(|entity| entity.end_line)
                .or_else(|| (entry.line_count > 0).then_some(entry.line_count)),
            line_count: entry.line_count,
        });
    }

    for entity in model.syntax.entities.iter().filter(|entity| {
        matches!(
            entity.kind,
            SyntaxEntityKind::Function | SyntaxEntityKind::Method
        )
    }) {
        let Some(parent_id) = canonical_id_by_file_id.get(entity.file_id.as_str()) else {
            continue;
        };
        let syntax_file_prefix = format!("syntax:{}", entity.file_path);
        let suffix = entity
            .id
            .strip_prefix(&syntax_file_prefix)
            .unwrap_or(entity.id.as_str());
        let id = format!("{parent_id}{suffix}");
        entities.push(CanonicalEntity {
            id: id.clone(),
            kind: canonical_kind_from_syntax(&entity.kind),
            name: entity.name.clone(),
            display_name: entity.display_name.clone(),
            file_path: entity.file_path.clone(),
            language: language_by_file_id
                .get(entity.file_id.as_str())
                .copied()
                .unwrap_or("unknown")
                .to_string(),
            parent_id: Some(parent_id.clone()),
            start_line: entity.start_line,
            end_line: entity.end_line,
            line_count: entity.line_count,
        });
        edges.push(CanonicalEdge {
            src_id: parent_id.clone(),
            dst_id: id,
            kind: "contains".to_string(),
            confidence: 1.0,
            source_file: entity.file_path.clone(),
        });
    }

    for relation in &model.static_relations {
        let Some(src_id) = canonical_id_by_file_id.get(relation.src_id.as_str()) else {
            continue;
        };
        let Some(dst_id) = canonical_id_by_file_id.get(relation.dst_id.as_str()) else {
            continue;
        };
        edges.push(CanonicalEdge {
            src_id: src_id.clone(),
            dst_id: dst_id.clone(),
            kind: relation.kind.clone(),
            confidence: relation.confidence,
            source_file: relation.source_file.clone(),
        });
    }

    CanonicalGraphSnapshot {
        entities,
        edges,
        repo_meta: model.repo_meta.clone(),
    }
}

pub fn function_entity_id(node: &Node, func: &Func, index: usize) -> String {
    format!("{}#{}:{}:{}", node.id, func.kind, func.name, index)
}

fn canonical_kind_from_syntax(kind: &SyntaxEntityKind) -> CanonicalEntityKind {
    match kind {
        SyntaxEntityKind::Module => CanonicalEntityKind::Module,
        SyntaxEntityKind::Class => CanonicalEntityKind::Class,
        SyntaxEntityKind::Function => CanonicalEntityKind::Function,
        SyntaxEntityKind::Method => CanonicalEntityKind::Method,
        SyntaxEntityKind::Constant => CanonicalEntityKind::Constant,
    }
}

fn filesystem_entry_kind_label(kind: &FilesystemEntryKind) -> &'static str {
    match kind {
        FilesystemEntryKind::Directory => "directory",
        FilesystemEntryKind::File => "file",
    }
}

fn parse_filesystem_entry_kind(value: &str) -> FilesystemEntryKind {
    if value == "file" {
        FilesystemEntryKind::File
    } else {
        FilesystemEntryKind::Directory
    }
}

fn parse_syntax_entity_kind(value: &str) -> SyntaxEntityKind {
    match value {
        "class" => SyntaxEntityKind::Class,
        "function" => SyntaxEntityKind::Function,
        "method" => SyntaxEntityKind::Method,
        "constant" => SyntaxEntityKind::Constant,
        _ => SyntaxEntityKind::Module,
    }
}

fn evidence_source_label(source: &EvidenceSource) -> &'static str {
    match source {
        EvidenceSource::TreeSitter => "tree_sitter",
    }
}

fn parse_evidence_source(_value: &str) -> EvidenceSource {
    EvidenceSource::TreeSitter
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
    use crate::program_model::program_model_from_graph;

    #[test]
    fn compatibility_projection_preserves_legacy_ids_and_contains_edges() {
        let graph = Graph {
            nodes: vec![Node {
                id: "lib/parser.ex".to_string(),
                label: "Parser".to_string(),
                kind: "module".to_string(),
                file: "lib/parser.ex".to_string(),
                line_count: 12,
                change_status: "unchanged".to_string(),
                functions: vec![Func {
                    name: "parse".to_string(),
                    arity: 2,
                    kind: "function".to_string(),
                }],
            }],
            edges: vec![],
        };
        let model = program_model_from_graph(Path::new("/tmp/repo"), &graph, "abc", 9);

        let snapshot = canonical_snapshot_from_program_model(&model);

        assert!(snapshot.entities.iter().any(|entity| {
            entity.id == "lib/parser.ex" && entity.kind == CanonicalEntityKind::Module
        }));
        assert!(snapshot.entities.iter().any(|entity| {
            entity.id == "lib/parser.ex#function:parse:0"
                && entity.parent_id.as_deref() == Some("lib/parser.ex")
        }));
        assert!(snapshot.edges.iter().any(|edge| {
            edge.src_id == "lib/parser.ex"
                && edge.dst_id == "lib/parser.ex#function:parse:0"
                && edge.kind == "contains"
        }));
    }

    #[test]
    fn snapshot_from_graph_creates_entities_and_contains_edges() {
        let graph = Graph {
            nodes: vec![
                Node {
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
                },
                Node {
                    id: "src/main.rs".to_string(),
                    label: "main.rs".to_string(),
                    kind: "file".to_string(),
                    file: "src/main.rs".to_string(),
                    line_count: 5,
                    change_status: "unchanged".to_string(),
                    functions: vec![],
                },
            ],
            edges: vec![Edge {
                source: "src/lib.rs".to_string(),
                target: "src/main.rs".to_string(),
                kind: "call".to_string(),
            }],
        };
        let snapshot = snapshot_from_graph(Path::new("/tmp/repo"), &graph, "abc123", 42);
        assert_eq!(snapshot.entities.len(), 3);
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
    fn schema_v1_migrates_to_v2_without_losing_legacy_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store_dir = dir.path().join(".sloposcope");
        fs::create_dir_all(&store_dir).expect("store dir");
        let db_path = store_dir.join("graph.sqlite3");
        let conn = Connection::open(&db_path).expect("legacy database");
        conn.execute_batch(
            "
            CREATE TABLE repo_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            INSERT INTO repo_meta (key, value) VALUES ('checkout_revision', 'legacy-head');
            PRAGMA user_version = 1;
            ",
        )
        .expect("legacy schema");
        drop(conn);

        let store = SqliteGraphStore::open_or_create(dir.path()).expect("migrate store");
        let conn = store.open_connection().expect("open migrated store");
        let version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        let model_table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('filesystem_entry', 'syntax_entity', 'static_relation')",
                [],
                |row| row.get(0),
            )
            .expect("model tables");

        assert_eq!(version, 2);
        assert_eq!(model_table_count, 3);
        assert_eq!(
            store
                .load_snapshot()
                .expect("load legacy snapshot")
                .repo_meta
                .get("checkout_revision")
                .map(String::as_str),
            Some("legacy-head")
        );
    }

    #[test]
    fn sqlite_store_roundtrips_expanded_program_model_and_legacy_projection() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteGraphStore::open_or_create(dir.path()).expect("open store");
        let graph = Graph {
            nodes: vec![Node {
                id: "src/parser.rs".to_string(),
                label: "parser.rs".to_string(),
                kind: "module".to_string(),
                file: "src/parser.rs".to_string(),
                line_count: 30,
                change_status: "unchanged".to_string(),
                functions: vec![Func {
                    name: "parse".to_string(),
                    arity: 2,
                    kind: "function".to_string(),
                }],
            }],
            edges: vec![],
        };
        let model = program_model_from_graph(dir.path(), &graph, "head", 10);

        store
            .replace_program_model(&model)
            .expect("replace expanded model");

        assert_eq!(store.load_program_model().expect("load model"), model);
        let legacy = store.load_snapshot().expect("load legacy projection");
        assert!(legacy
            .entities
            .iter()
            .any(|entity| entity.id == "src/parser.rs#function:parse:0"));
    }

    #[test]
    fn incremental_program_file_update_replaces_only_its_owned_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteGraphStore::open_or_create(dir.path()).expect("open store");
        let initial_graph = Graph {
            nodes: vec![
                Node {
                    id: "src/a.ts".into(),
                    label: "a.ts".into(),
                    kind: "file".into(),
                    file: "src/a.ts".into(),
                    line_count: 10,
                    change_status: "unchanged".into(),
                    functions: vec![Func {
                        name: "oldName".into(),
                        arity: 0,
                        kind: "function".into(),
                    }],
                },
                Node {
                    id: "src/b.ts".into(),
                    label: "b.ts".into(),
                    kind: "file".into(),
                    file: "src/b.ts".into(),
                    line_count: 5,
                    change_status: "unchanged".into(),
                    functions: vec![],
                },
            ],
            edges: vec![Edge {
                source: "src/a.ts".into(),
                target: "src/b.ts".into(),
                kind: "import".into(),
            }],
        };
        let initial = program_model_from_graph(dir.path(), &initial_graph, "one", 1);
        store.replace_program_model(&initial).expect("seed model");

        let updated_graph = Graph {
            nodes: vec![Node {
                id: "src/a.ts".into(),
                label: "a.ts".into(),
                kind: "file".into(),
                file: "src/a.ts".into(),
                line_count: 12,
                change_status: "unchanged".into(),
                functions: vec![Func {
                    name: "newName".into(),
                    arity: 1,
                    kind: "function".into(),
                }],
            }],
            edges: vec![],
        };
        let updated = program_model_from_graph(dir.path(), &updated_graph, "two", 2);
        store
            .upsert_program_file("src/a.ts", &updated)
            .expect("update one file");

        let loaded = store.load_program_model().expect("load updated model");
        assert!(loaded
            .filesystem
            .entries
            .iter()
            .any(|entry| entry.id == "fs-file:src/b.ts"));
        assert!(loaded
            .syntax
            .entities
            .iter()
            .any(|entity| entity.name == "newName"));
        assert!(!loaded
            .syntax
            .entities
            .iter()
            .any(|entity| entity.name == "oldName"));
        assert!(loaded.static_relations.is_empty());

        store
            .remove_program_file("src/b.ts")
            .expect("remove neighboring file");
        let removed = store.load_program_model().expect("load removed model");
        assert!(!removed
            .filesystem
            .entries
            .iter()
            .any(|entry| entry.id == "fs-file:src/b.ts"));
        assert!(removed
            .filesystem
            .entries
            .iter()
            .any(|entry| entry.id == "fs-dir:src"));
        assert!(!store
            .load_snapshot()
            .expect("load legacy after remove")
            .entities
            .iter()
            .any(|entity| entity.file_path == "src/b.ts"));
    }

    #[test]
    fn removing_program_file_removes_relations_pointing_to_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteGraphStore::open_or_create(dir.path()).expect("open store");
        let graph = Graph {
            nodes: vec![
                Node {
                    id: "src/a.ts".into(),
                    label: "a.ts".into(),
                    kind: "file".into(),
                    file: "src/a.ts".into(),
                    line_count: 1,
                    change_status: "unchanged".into(),
                    functions: vec![],
                },
                Node {
                    id: "src/b.ts".into(),
                    label: "b.ts".into(),
                    kind: "file".into(),
                    file: "src/b.ts".into(),
                    line_count: 1,
                    change_status: "unchanged".into(),
                    functions: vec![],
                },
            ],
            edges: vec![Edge {
                source: "src/a.ts".into(),
                target: "src/b.ts".into(),
                kind: "import".into(),
            }],
        };
        let model = program_model_from_graph(dir.path(), &graph, "one", 1);
        store.replace_program_model(&model).expect("seed model");

        store
            .remove_program_file("src/b.ts")
            .expect("remove relation target");

        assert!(store
            .load_program_model()
            .expect("load model")
            .static_relations
            .is_empty());
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

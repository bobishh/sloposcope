use crate::graph_store::CanonicalGraphSnapshot;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::doc;
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING,
};
use tantivy::{Index, TantivyDocument};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub entity_id: String,
    pub display_name: String,
    pub file_path: String,
    pub language: String,
    pub score: f32,
    pub why: String,
}

#[derive(Debug, Clone)]
struct SearchFields {
    entity_id: Field,
    display_name: Field,
    name: Field,
    file_path: Field,
    language: Field,
}

#[derive(Debug, Clone)]
pub struct SearchService {
    index: Index,
    fields: SearchFields,
    _index_path: PathBuf,
}

impl SearchService {
    pub fn index_path(repo: &Path) -> PathBuf {
        repo.join(".sloposcope").join("search-index")
    }

    pub fn open_or_create(repo: &Path) -> Result<Self, String> {
        let index_path = Self::index_path(repo);
        fs::create_dir_all(&index_path).map_err(|err| {
            format!(
                "Failed to create search index dir at {}: {err}",
                index_path.display()
            )
        })?;

        let schema = search_schema();
        let index = if index_path.join("meta.json").exists() {
            Index::open_in_dir(&index_path)
                .map_err(|err| format!("Failed to open search index: {err}"))?
        } else {
            Index::create_in_dir(&index_path, schema.clone())
                .map_err(|err| format!("Failed to create search index: {err}"))?
        };
        let fields = load_fields(index.schema())?;
        Ok(Self {
            index,
            fields,
            _index_path: index_path,
        })
    }

    pub fn rebuild_from_snapshot(&self, snapshot: &CanonicalGraphSnapshot) -> Result<(), String> {
        let mut writer = self
            .index
            .writer(50_000_000)
            .map_err(|err| format!("Failed to open search writer: {err}"))?;
        writer
            .delete_all_documents()
            .map_err(|err| format!("Failed to clear search index: {err}"))?;

        for entity in &snapshot.entities {
            writer
                .add_document(doc!(
                    self.fields.entity_id => entity.id.clone(),
                    self.fields.display_name => entity.display_name.clone(),
                    self.fields.name => entity.name.clone(),
                    self.fields.file_path => entity.file_path.clone(),
                    self.fields.language => entity.language.clone(),
                ))
                .map_err(|err| format!("Failed to add search document {}: {err}", entity.id))?;
        }

        writer
            .commit()
            .map_err(|err| format!("Failed to commit search index: {err}"))?;
        Ok(())
    }

    pub fn query(&self, q: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
        let query_text = q.trim();
        if query_text.is_empty() {
            return Ok(Vec::new());
        }

        let reader = self
            .index
            .reader()
            .map_err(|err| format!("Failed to open search reader: {err}"))?;
        let searcher = reader.searcher();
        let parser = QueryParser::for_index(
            &self.index,
            vec![
                self.fields.display_name,
                self.fields.name,
                self.fields.file_path,
                self.fields.language,
            ],
        );
        let query = parser
            .parse_query(query_text)
            .map_err(|err| format!("Failed to parse search query: {err}"))?;
        let docs = searcher
            .search(&query, &TopDocs::with_limit(limit.max(1)))
            .map_err(|err| format!("Failed to run search query: {err}"))?;
        let tokens = tokenize(query_text);

        docs.into_iter()
            .map(|(score, address)| {
                let doc: TantivyDocument = searcher
                    .doc(address)
                    .map_err(|err| format!("Failed to load search document: {err}"))?;
                let entity_id = first_text(&doc, self.fields.entity_id)
                    .ok_or_else(|| "Search result missing entity_id".to_string())?;
                let display_name = first_text(&doc, self.fields.display_name)
                    .ok_or_else(|| format!("Search result missing display_name for {entity_id}"))?;
                let name = first_text(&doc, self.fields.name).unwrap_or_default();
                let file_path = first_text(&doc, self.fields.file_path)
                    .ok_or_else(|| format!("Search result missing file_path for {entity_id}"))?;
                let language = first_text(&doc, self.fields.language)
                    .ok_or_else(|| format!("Search result missing language for {entity_id}"))?;
                Ok(SearchResult {
                    entity_id,
                    display_name: display_name.clone(),
                    file_path: file_path.clone(),
                    language: language.clone(),
                    score,
                    why: explain_match(&tokens, &display_name, &name, &file_path, &language),
                })
            })
            .collect()
    }
}

fn search_schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_text_field("entity_id", STRING | STORED);
    builder.add_text_field("display_name", searchable_text_options());
    builder.add_text_field("name", searchable_text_options());
    builder.add_text_field("file_path", searchable_text_options());
    builder.add_text_field("language", searchable_text_options());
    builder.build()
}

fn searchable_text_options() -> TextOptions {
    let indexing = TextFieldIndexing::default()
        .set_tokenizer("default")
        .set_index_option(IndexRecordOption::WithFreqsAndPositions);
    TextOptions::default()
        .set_indexing_options(indexing)
        .set_stored()
}

fn load_fields(schema: Schema) -> Result<SearchFields, String> {
    Ok(SearchFields {
        entity_id: schema
            .get_field("entity_id")
            .map_err(|_| "Search schema missing field entity_id".to_string())?,
        display_name: schema
            .get_field("display_name")
            .map_err(|_| "Search schema missing field display_name".to_string())?,
        name: schema
            .get_field("name")
            .map_err(|_| "Search schema missing field name".to_string())?,
        file_path: schema
            .get_field("file_path")
            .map_err(|_| "Search schema missing field file_path".to_string())?,
        language: schema
            .get_field("language")
            .map_err(|_| "Search schema missing field language".to_string())?,
    })
}

fn first_text(doc: &TantivyDocument, field: Field) -> Option<String> {
    doc.get_first(field)
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
}

fn tokenize(q: &str) -> Vec<String> {
    q.split_whitespace()
        .map(|part| part.trim().to_ascii_lowercase())
        .filter(|part| !part.is_empty())
        .collect()
}

fn explain_match(
    tokens: &[String],
    display_name: &str,
    name: &str,
    file_path: &str,
    language: &str,
) -> String {
    let display_lower = display_name.to_ascii_lowercase();
    let name_lower = name.to_ascii_lowercase();
    let file_lower = file_path.to_ascii_lowercase();
    let lang_lower = language.to_ascii_lowercase();

    if tokens.iter().all(|token| display_lower.contains(token)) {
        return "matched display name".to_string();
    }
    if tokens.iter().all(|token| name_lower.contains(token)) {
        return "matched entity name".to_string();
    }
    if tokens.iter().all(|token| file_lower.contains(token)) {
        return "matched file path".to_string();
    }
    if tokens.iter().all(|token| lang_lower.contains(token)) {
        return "matched language".to_string();
    }
    "matched indexed graph entity".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_store::{CanonicalEntity, CanonicalEntityKind};
    use std::collections::HashMap;

    fn snapshot() -> CanonicalGraphSnapshot {
        CanonicalGraphSnapshot {
            entities: vec![
                CanonicalEntity {
                    id: "src/services/user_service.ts".to_string(),
                    kind: CanonicalEntityKind::File,
                    name: "src/services/user_service.ts".to_string(),
                    display_name: "UserService".to_string(),
                    file_path: "src/services/user_service.ts".to_string(),
                    language: "typescript".to_string(),
                    parent_id: None,
                    start_line: Some(1),
                    end_line: Some(40),
                    line_count: 40,
                },
                CanonicalEntity {
                    id: "src/models/user.ts".to_string(),
                    kind: CanonicalEntityKind::File,
                    name: "src/models/user.ts".to_string(),
                    display_name: "User".to_string(),
                    file_path: "src/models/user.ts".to_string(),
                    language: "typescript".to_string(),
                    parent_id: None,
                    start_line: Some(1),
                    end_line: Some(20),
                    line_count: 20,
                },
            ],
            edges: vec![],
            repo_meta: HashMap::new(),
        }
    }

    #[test]
    fn query_returns_matching_entities_from_rebuilt_index() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = SearchService::open_or_create(dir.path()).expect("open");
        service
            .rebuild_from_snapshot(&snapshot())
            .expect("rebuild search index");

        let results = service.query("user service", 10).expect("query");
        assert!(!results.is_empty(), "expected at least one result");
        assert_eq!(results[0].entity_id, "src/services/user_service.ts");
        assert!(
            results[0].why.to_ascii_lowercase().contains("display")
                || results[0].why.to_ascii_lowercase().contains("name")
        );
    }
}

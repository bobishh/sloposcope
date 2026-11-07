use serde_json::{json, Value};
use sloposcope_lib::jev_analysis::{AnalysisService, Batch, JevTransport, TransportFuture};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn inventory_exports_full_source_evidence_without_sending() {
    use sha2::{Digest, Sha256};
    let repo = tempfile::tempdir().unwrap();
    let source = "fn sample() {}\n";
    std::fs::write(repo.path().join("sample.rs"), source).unwrap();
    let inventory = AnalysisService::default().index(repo.path()).unwrap();
    let value = serde_json::to_value(&inventory.groups[0]).unwrap();
    assert_eq!(value["source_id"], format!("sha256:{:x}", Sha256::digest(source.as_bytes())));
}

struct FakeTransport {
    calls: AtomicUsize,
    fail: bool,
}
impl JevTransport for FakeTransport {
    fn classify<'a>(&'a self, batch: &'a Batch) -> TransportFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err("fake failure; credential must not leak".into());
            }
            let mut answers = serde_json::Map::new();
            for i in 0..batch.groups.len() {
                answers.insert(format!("{i}_role"), json!({"choice":"transformation","probabilities":{"transformation":1.0,"orchestration":0.0,"boundary":0.0,"representation":0.0,"unknown":0.0},"confidence":1.0}));
                answers.insert(format!("{i}_multiple"), json!({"noul":0.2}));
                let criteria = batch.payload["questions"][format!("{i}_hiding")]["criteria"]
                    .as_array()
                    .unwrap();
                let legend: serde_json::Map<String, Value> = criteria
                    .iter()
                    .enumerate()
                    .map(|(j, v)| (j.to_string(), v.clone()))
                    .collect();
                answers.insert(format!("{i}_hiding"), json!({"score":1.0,"legend":legend,"probabilities":{"0":0.0,"1":1.0,"2":0.0},"confidence":1.0}));
                answers.insert(format!("{i}_context"), json!({"choice":"sufficient","probabilities":{"sufficient":1.0,"insufficient":0.0},"confidence":1.0}));
            }
            Ok(
                json!({"model":"jev-1.13.0","answers":Value::Object(answers),"usage":{"input_tokens":120,"output_tokens":40}}),
            )
        })
    }
}

struct PartialTransport {
    calls: AtomicUsize,
    valid: FakeTransport,
}
impl JevTransport for PartialTransport {
    fn classify<'a>(&'a self, batch: &'a Batch) -> TransportFuture<'a> {
        Box::pin(async move {
            if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
                return Err("second batch failed".into());
            }
            self.valid.classify(batch).await
        })
    }
}

#[test]
fn successful_batches_survive_later_failure_and_next_plan_skips_them() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..9 {
        std::fs::write(
            dir.path().join(format!("{i}.rs")),
            format!("fn f{i}() {{}}"),
        )
        .unwrap();
    }
    let service = AnalysisService::default();
    let ids: Vec<_> = service
        .index(dir.path())
        .unwrap()
        .groups
        .iter()
        .map(|g| g.id.clone())
        .collect();
    let plan = service.prepare(dir.path(), ids.clone(), 3).unwrap();
    let transport = PartialTransport {
        calls: AtomicUsize::new(0),
        valid: FakeTransport {
            calls: AtomicUsize::new(0),
            fail: false,
        },
    };
    let outcome =
        tauri::async_runtime::block_on(service.execute(dir.path(), &plan.id, 3, &transport))
            .unwrap();
    assert_eq!(outcome.requests_attempted, 2);
    assert_eq!(outcome.requests_completed, 1);
    assert_eq!(outcome.assessments.len(), 4);
    assert_eq!(outcome.input_tokens, 120);
    let plan = service.prepare(dir.path(), ids, 3).unwrap();
    assert_eq!(plan.request_count, 2);
    assert_eq!(plan.cached.len(), 4);
    assert_eq!(
        plan.batches.iter().map(|b| b.groups.len()).sum::<usize>(),
        5
    );
}

#[test]
fn wrong_confirmation_count_and_repository_never_send() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("one.rs"), "fn x() {}").unwrap();
    let service = AnalysisService::default();
    let id = service.index(dir.path()).unwrap().groups[0].id.clone();
    let transport = FakeTransport {
        calls: AtomicUsize::new(0),
        fail: false,
    };
    let plan = service.prepare(dir.path(), vec![id.clone()], 1).unwrap();
    assert!(
        tauri::async_runtime::block_on(service.execute(dir.path(), &plan.id, 2, &transport))
            .is_err()
    );
    assert!(
        tauri::async_runtime::block_on(service.execute(dir.path(), &plan.id, 1, &transport))
            .is_err()
    );
    let plan = service.prepare(dir.path(), vec![id], 1).unwrap();
    assert!(
        tauri::async_runtime::block_on(service.execute(other.path(), &plan.id, 1, &transport))
            .is_err()
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn given_offline_index_when_confirmed_then_dedup_cache_and_one_use_approval() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("one.rs"), "pub fn transform() -> i32 { 3 }").unwrap();
    std::fs::write(
        dir.path().join("copy.rs"),
        "pub fn transform() -> i32 { 3 }",
    )
    .unwrap();
    let service = AnalysisService::default();
    let index = service.index(dir.path()).unwrap();
    assert_eq!(index.groups.len(), 1);
    assert_eq!(index.groups[0].paths.len(), 2);
    let transport = FakeTransport {
        calls: AtomicUsize::new(0),
        fail: false,
    };
    let plan = service
        .prepare(dir.path(), vec![index.groups[0].id.clone()], 1)
        .unwrap();
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    assert_eq!(plan.request_count, 1);
    let outcome =
        tauri::async_runtime::block_on(service.execute(dir.path(), &plan.id, 1, &transport))
            .unwrap();
    assert_eq!(outcome.requests_attempted, 1);
    assert_eq!(outcome.assessments.len(), 1);
    assert!(
        tauri::async_runtime::block_on(service.execute(dir.path(), &plan.id, 1, &transport))
            .is_err()
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let index = service.index(dir.path()).unwrap();
    assert!(index.groups[0].cached.is_some());
    let plan = service
        .prepare(dir.path(), vec![index.groups[0].id.clone()], 1)
        .unwrap();
    assert_eq!(plan.request_count, 0);
}

#[test]
fn given_failure_when_sending_then_stop_without_retry() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        std::fs::write(
            dir.path().join(format!("{i}.rs")),
            format!("fn f{i}() {{}}"),
        )
        .unwrap();
    }
    let service = AnalysisService::default();
    let index = service.index(dir.path()).unwrap();
    let plan = service
        .prepare(
            dir.path(),
            index.groups.iter().map(|g| g.id.clone()).collect(),
            2,
        )
        .unwrap();
    assert_eq!(plan.request_count, 2);
    let transport = FakeTransport {
        calls: AtomicUsize::new(0),
        fail: true,
    };
    let outcome =
        tauri::async_runtime::block_on(service.execute(dir.path(), &plan.id, 2, &transport))
            .unwrap();
    assert_eq!(outcome.requests_attempted, 1);
    assert_eq!(
        outcome.error.as_deref(),
        Some("Stopped after request failure; no automatic retry")
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(service.index(dir.path()).unwrap().uncertain_requests, 1);
}

#[test]
fn offline_filters_generated_dependencies_secrets_and_bounds_excerpts() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("node_modules")).unwrap();
    std::fs::write(dir.path().join("node_modules/vendor.ts"), "let x = 1;").unwrap();
    std::fs::write(dir.path().join("generated.rs"), "// @generated\nfn x() {}").unwrap();
    std::fs::write(
        dir.path().join("secret.rs"),
        "const KEY: &str = \"ghp_abcdefghijklmnopqrstuvwxyz0123456789\";",
    )
    .unwrap();
    std::fs::write(dir.path().join("readme.md"), "docs").unwrap();
    std::fs::write(dir.path().join("main.elm"), "α".repeat(8000)).unwrap();
    let index = AnalysisService::default().index(dir.path()).unwrap();
    assert_eq!(index.groups.len(), 1);
    assert_eq!(index.groups[0].language, "elm");
    assert!(index.groups[0].truncated);
    assert!(index.groups[0].excerpt_bytes <= 12_000);
    assert!(index.excluded.values().sum::<usize>() >= 3);
}

#[test]
fn limits_and_stale_content_fail_before_transport() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        std::fs::write(
            dir.path().join(format!("{i}.rs")),
            format!("fn f{i}() {{}}"),
        )
        .unwrap();
    }
    let service = AnalysisService::default();
    let ids: Vec<_> = service
        .index(dir.path())
        .unwrap()
        .groups
        .iter()
        .map(|g| g.id.clone())
        .collect();
    assert!(service
        .prepare(dir.path(), ids.clone(), 1)
        .unwrap_err()
        .contains("limit"));
    assert!(service.prepare(dir.path(), ids.clone(), 21).is_err());
    assert!(service.prepare(dir.path(), vec![], 5).is_err());
    let plan = service.prepare(dir.path(), ids, 2).unwrap();
    std::fs::write(dir.path().join("0.rs"), "fn changed() {}").unwrap();
    let transport = FakeTransport {
        calls: AtomicUsize::new(0),
        fail: false,
    };
    assert!(
        tauri::async_runtime::block_on(service.execute(dir.path(), &plan.id, 2, &transport))
            .unwrap_err()
            .contains("changed")
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

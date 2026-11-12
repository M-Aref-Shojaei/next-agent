use next_agent_core::store::SCHEMA_VERSION;
use next_agent_core::{Store, Workspace};
use std::fs;
use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("na-store-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn migrations_apply_and_are_idempotent() {
    let store = Store::open_in_memory().expect("open");
    assert_eq!(store.schema_version().expect("version"), SCHEMA_VERSION);
}

#[test]
fn reopening_a_file_keeps_data() {
    let dir = temp_dir("reopen");
    let db = dir.join("next-agent.db");
    let workspace_path = dir.join("project");
    fs::create_dir_all(&workspace_path).expect("project");
    let workspace = Workspace::open(&workspace_path).expect("workspace");

    {
        let store = Store::open(&db).expect("open");
        assert!(store.upsert_workspace(&workspace).expect("insert"));
        store.set_setting("router.url", "\"http://127.0.0.1:20128/v1\"").expect("setting");
    }

    let store = Store::open(&db).expect("reopen");
    assert_eq!(store.list_workspaces().expect("list").len(), 1);
    assert_eq!(
        store.get_setting("router.url").expect("get").as_deref(),
        Some("\"http://127.0.0.1:20128/v1\"")
    );
}

#[test]
fn upsert_is_idempotent_and_preserves_order() {
    let dir = temp_dir("upsert");
    let store = Store::open_in_memory().expect("open");
    let a = Workspace::open(&dir).expect("a");
    assert!(store.upsert_workspace(&a).expect("insert"));
    assert!(!store.upsert_workspace(&a).expect("update"));

    let list = store.list_workspaces().expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, a.id());
    assert_eq!(list[0].name, a.name());
}

#[test]
fn settings_round_trip_and_overwrite() {
    // see events_are_persisted_in_order below for the event table
    let store = Store::open_in_memory().expect("open");
    assert_eq!(store.get_setting("missing").expect("get"), None);
    store.set_setting("mode", "\"manual\"").expect("set");
    store.set_setting("mode", "\"auto\"").expect("overwrite");
    assert_eq!(store.get_setting("mode").expect("get").as_deref(), Some("\"auto\""));
}

#[test]
fn events_are_persisted_in_order_and_unique() {
    let dir = temp_dir("events");
    let store = Store::open_in_memory().expect("open");
    let workspace = Workspace::open(&dir).expect("workspace");
    store.upsert_workspace(&workspace).expect("insert");
    store
        .create_task("tsk_0000000000000001", workspace.id(), "t", "p", "auto")
        .expect("task");

    store.append_event("tsk_0000000000000001", 1, "tool_requested", "{}").expect("seq1");
    store.append_event("tsk_0000000000000001", 0, "task_started", "{}").expect("seq0");
    assert_eq!(
        store.event_kinds("tsk_0000000000000001").expect("kinds"),
        ["task_started", "tool_requested"]
    );
    assert!(
        store.append_event("tsk_0000000000000001", 1, "dup", "{}").is_err(),
        "duplicate seq must be rejected"
    );
    assert!(
        store.append_event("tsk_missing", 0, "x", "{}").is_err(),
        "unknown task must be rejected by the foreign key"
    );
}


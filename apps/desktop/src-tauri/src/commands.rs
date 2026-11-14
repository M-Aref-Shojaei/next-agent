//! Tauri command wrappers. Each is a one-liner over [`AppService`].
#![allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.

use crate::review::ReviewOutcome;
use crate::service::{
    AppService, AuditStatus, HealthDto, RuleDto, Sink, TaskRequest, WorkspaceDto,
};
use crate::wire::StreamMessage;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::Emitter;
use tauri::State;

type Svc<'a> = State<'a, Arc<AppService>>;

#[tauri::command]
pub fn workspace_open(service: Svc, path: String) -> Result<WorkspaceDto, String> {
    service.workspace_open(&path)
}

#[tauri::command]
pub fn workspace_list(service: Svc) -> Result<Vec<WorkspaceDto>, String> {
    service.workspace_list()
}

#[tauri::command]
pub fn vault_set(service: Svc, path: String) -> Result<String, String> {
    service.vault_set(&path)
}

#[tauri::command]
pub fn vault_get(service: Svc) -> Result<Option<String>, String> {
    service.vault_get()
}

#[tauri::command]
pub fn setting_get(service: Svc, key: String) -> Result<Option<String>, String> {
    service.setting_get(&key)
}

#[tauri::command]
pub fn setting_set(service: Svc, key: String, value: String) -> Result<(), String> {
    service.setting_set(&key, &value)
}

#[tauri::command]
pub fn rules_list(service: Svc, workspace_id: String) -> Result<Vec<RuleDto>, String> {
    service.rules_list(&workspace_id)
}

#[tauri::command]
pub fn rule_delete(service: Svc, id: String) -> Result<bool, String> {
    service.rule_delete(&id)
}

#[tauri::command]
pub fn audit_verify(service: Svc) -> Result<AuditStatus, String> {
    service.audit_verify()
}

#[tauri::command]
pub fn task_start(
    app: tauri::AppHandle,
    service: Svc,
    workspace_id: String,
    prompt: String,
    mode: String,
    on_event: Channel<StreamMessage>,
) -> Result<String, String> {
    let direct: Sink = Arc::new(move |message| {
        let _ = on_event.send(message);
    });
    // Same stream for the overlay window: broadcast alongside the Channel.
    let handle = app.clone();
    let broadcast: Sink = Arc::new(move |message| {
        let _ = handle.emit("task-stream", &message);
    });
    let sink = crate::overlay::fanout(direct, broadcast);
    let title: String = prompt.chars().take(60).collect();
    let request = TaskRequest {
        workspace_id,
        prompt,
        mode: mode.clone(),
    };
    let id = Arc::clone(&service).task_start(&request, sink)?;
    let _ = app.emit(
        "task-started",
        serde_json::json!({ "taskId": id, "title": title, "mode": mode }),
    );
    Ok(id)
}

#[tauri::command]
pub fn task_cancel(service: Svc, task_id: String) {
    service.task_cancel(&task_id);
}

#[tauri::command]
pub fn approval_respond(
    service: Svc,
    task_id: String,
    approval_id: String,
    choice: String,
) -> Result<(), String> {
    service.approval_respond(&task_id, &approval_id, &choice)
}

#[tauri::command]
pub fn review_action(
    service: Svc,
    task_id: String,
    action: String,
) -> Result<ReviewOutcome, String> {
    service.review_action(&task_id, &action)
}

#[tauri::command]
#[must_use]
pub fn router_health(service: Svc) -> Option<HealthDto> {
    service.health()
}

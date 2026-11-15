//! Tauri command wrappers. Each is a one-liner over [`AppService`].
#![allow(clippy::needless_pass_by_value, clippy::missing_errors_doc)] // Tauri commands take owned args.

use crate::boards;
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
#[allow(clippy::too_many_arguments)] // Tauri injects the state and the channel.
pub fn task_start(
    app: tauri::AppHandle,
    service: Svc,
    adapter: State<crate::coucou::Adapter>,
    workspace_id: String,
    prompt: String,
    mode: String,
    on_event: Channel<StreamMessage>,
    profile: Option<String>,
) -> Result<String, String> {
    // The same events reach Coucou's island as hook events (coucou.rs).
    let cwd = service
        .workspace(&workspace_id)
        .map_or_else(|_| String::new(), |w| w.path);
    let sink = task_sink(
        &adapter,
        on_event,
        crate::coucou::TaskContext {
            prompt: prompt.clone(),
            cwd,
        },
    );
    let title: String = prompt.chars().take(60).collect();
    let request = TaskRequest {
        workspace_id,
        prompt,
        mode: mode.clone(),
        profile,
    };
    let id = Arc::clone(&service).task_start(&request, sink)?;
    announce_task(&app, &id, &title, &mode);
    Ok(id)
}

/// A task's event sink: the caller's Channel, with the same events passed to
/// Coucou's island as hook events (coucou.rs).
pub(crate) fn task_sink(
    adapter: &crate::coucou::Adapter,
    on_event: Channel<StreamMessage>,
    ctx: crate::coucou::TaskContext,
) -> Sink {
    let direct: Sink = Arc::new(move |message| {
        let _ = on_event.send(message);
    });
    adapter.wrap(ctx, direct)
}

/// Tell every window a task started (sidebar, overlay).
pub(crate) fn announce_task(app: &tauri::AppHandle, id: &str, title: &str, mode: &str) {
    let _ = app.emit(
        "task-started",
        serde_json::json!({ "taskId": id, "title": title, "mode": mode }),
    );
}

/// Settings → Overlay: opens Coucou's own settings window on `page`
/// (`general` when none).
#[tauri::command]
pub fn overlay_open(app: tauri::AppHandle, page: Option<String>) {
    coucou_lib::open_window(&app, Some(page.as_deref().unwrap_or("general")));
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

// ---- Design boards (vault `boards/*.excalidraw`) ----

fn vault_dir(service: &AppService) -> Result<std::path::PathBuf, String> {
    service
        .vault_get()?
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "no vault configured: set a vault folder in Settings first".to_string())
}

#[tauri::command]
pub fn board_list(service: Svc) -> Result<Vec<boards::BoardMeta>, String> {
    boards::list(&vault_dir(&service)?)
}

#[tauri::command]
pub fn board_create(
    service: Svc,
    name: String,
    scene: Option<String>,
) -> Result<boards::BoardDoc, String> {
    boards::create(&vault_dir(&service)?, &name, scene.as_deref())
}

#[tauri::command]
pub fn board_open(service: Svc, name: String) -> Result<boards::BoardDoc, String> {
    boards::open(&vault_dir(&service)?, &name)
}

#[tauri::command]
pub fn board_save(
    service: Svc,
    name: String,
    content: String,
    base_revision: Option<String>,
) -> Result<String, String> {
    boards::save(
        &vault_dir(&service)?,
        &name,
        &content,
        base_revision.as_deref(),
    )
}

#[tauri::command]
pub fn board_rename(service: Svc, from: String, to: String) -> Result<boards::BoardMeta, String> {
    boards::rename(&vault_dir(&service)?, &from, &to)
}

#[tauri::command]
pub fn board_delete(service: Svc, name: String) -> Result<String, String> {
    boards::delete(&vault_dir(&service)?, &name)
}

/// A board's live elements, for the Design Studio.
#[tauri::command]
pub fn board_sketch(service: Svc, name: String) -> Result<boards::BoardSketch, String> {
    boards::sketch(&vault_dir(&service)?, &name)
}

/// Save an exported PNG (base64) or SVG (text) under the vault's `assets/`.
#[tauri::command]
pub fn board_export(
    service: Svc,
    name: String,
    ext: String,
    data: String,
) -> Result<String, String> {
    boards::export_asset(&vault_dir(&service)?, &name, &ext, &data)
}

#[tauri::command]
pub fn agent_profiles(
    service: Svc,
    workspace_id: String,
) -> Result<crate::tasks::ProfileList, String> {
    service.agent_profiles(&workspace_id)
}

#[tauri::command]
pub fn task_undo_step(service: Svc, task_id: String) -> Result<crate::tasks::UndoDto, String> {
    service.task_undo_step(&task_id)
}

#[tauri::command]
pub fn task_undoable(service: Svc, task_id: String) -> Result<usize, String> {
    service.task_undoable(&task_id)
}

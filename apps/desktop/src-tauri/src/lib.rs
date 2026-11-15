//! Tauri 2 shell for Next Agent.

pub mod boards;
pub mod commands;
pub mod coucou;
pub mod gateway;
pub mod learning;
pub mod notes;
pub mod review;
pub mod service;
pub mod tasks;
pub mod upstreams;
pub mod wire;

use next_agent_process_manager::{
    HealthMonitor, HealthProbe, HealthReport, HealthStatus, MonitorConfig,
};
use service::AppService;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// Product name shown in the window title and installers.
pub const PRODUCT_NAME: &str = "Next Agent";

/// Identifier used by the OS for app data directories.
pub const APP_IDENTIFIER: &str = "dev.nextagent.desktop";

/// Start the desktop app.
///
/// # Panics
/// Panics if the Tauri runtime cannot start (unrecoverable at launch).
pub fn run() {
    // The overlay is Coucou's Tauri app, vendored whole (ADR-0011): its
    // plugins and state, its setup, its commands and its run-event handling.
    let (builder, boot) = coucou_lib::builder(tauri::Builder::default());
    builder
        .setup(move |app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let service = Arc::new(AppService::new(dir.join("next-agent.db"))?);
            app.manage(Arc::clone(&service));
            let notes = Arc::new(notes::NotesService::new(dir.join("graph-index")));
            let handle = app.handle().clone();
            notes.set_listener(Arc::new(move |files| {
                let _ = handle.emit("graph-changed", files);
            }));
            app.manage(notes);
            app.manage(coucou::Adapter::start(
                app.handle().clone(),
                Arc::clone(&service),
            ));
            // Settings → Upstreams: a daily, report-only look at upstream GitHub repos.
            app.manage(upstreams::start(app.handle(), &dir));
            learning::start_scheduler(app.handle().clone(), Arc::clone(&service));
            tauri::async_runtime::spawn(poll_router_health(app.handle().clone(), service));
            coucou_lib::setup(app, boot)?;
            Ok(())
        })
        .invoke_handler(coucou_lib::invoke_handler(tauri::generate_handler![
            commands::workspace_open,
            commands::workspace_list,
            commands::vault_set,
            commands::vault_get,
            commands::setting_get,
            commands::setting_set,
            commands::rules_list,
            commands::rule_delete,
            commands::audit_verify,
            commands::task_start,
            commands::task_cancel,
            commands::agent_profiles,
            commands::task_undo_step,
            commands::task_undoable,
            commands::approval_respond,
            commands::review_action,
            commands::router_health,
            notes::graph_open,
            notes::graph_search,
            notes::graph_page,
            notes::graph_backlinks,
            notes::graph_journals,
            notes::graph_save_page,
            notes::graph_rebuild,
            // --- notes-ui ---
            notes::graph_pages,
            notes::graph_graph,
            notes::graph_query,
            notes::graph_journal_name,
            notes::graph_block,
            // --- end notes-ui ---
            gateway::gateway_presets,
            gateway::gateway_connections,
            gateway::gateway_connection_save,
            gateway::gateway_connection_delete,
            gateway::gateway_connection_test,
            gateway::gateway_combos,
            gateway::gateway_combo_save,
            gateway::gateway_combo_delete,
            gateway::gateway_settings_get,
            gateway::gateway_settings_set,
            gateway::gateway_targets,
            gateway::usage_summary,
            gateway::gateway_catalog_status,
            gateway::gateway_catalog_refresh,
            gateway::gateway_cache_count,
            gateway::gateway_cache_clear,
            commands::overlay_open,
            upstreams::upstream_watch_status,
            upstreams::upstream_watch_check,
            upstreams::upstream_watch_mark_seen,
            upstreams::upstream_open,
            commands::board_list,
            commands::board_create,
            commands::board_open,
            commands::board_save,
            commands::board_rename,
            commands::board_delete,
            commands::board_sketch,
            commands::board_export,
            learning::learning_settings_get,
            learning::learning_settings_set,
            learning::memory_get,
            learning::memory_save,
            learning::skills_overview,
            learning::skill_read,
            learning::skill_pin,
            learning::skill_archive,
            learning::skill_restore,
            learning::curator_run,
            learning::cron_jobs,
            learning::cron_save,
            learning::cron_pause,
            learning::cron_delete,
            learning::cron_run_now,
        ]))
        .build(tauri::generate_context!())
        .expect("error while running Next Agent")
        .run(|app, event| coucou_lib::on_run_event(app, &event));
}

/// Health probe for the status-bar dot: the provider behind the `chat` role
/// (a built-in provider, or the external router from `router.url`).
fn health_probe(service: &AppService) -> Result<Arc<dyn HealthProbe>, String> {
    Ok(Arc::new(service.gateway("chat")?))
}

fn publish(app: &AppHandle, service: &AppService, report: &HealthReport) {
    service.set_health(report);
    let _ = app.emit("router-health", service.health());
}

/// Poll the router and push status changes to the status bar. Rebuilds the
/// client whenever router settings change.
async fn poll_router_health(app: AppHandle, service: Arc<AppService>) {
    let mut generation = service.router_generation();
    loop {
        let monitor = match health_probe(&service) {
            Ok(probe) => Some(HealthMonitor::spawn(probe, MonitorConfig::default())),
            Err(error) => {
                publish(
                    &app,
                    &service,
                    &HealthReport::new(HealthStatus::Down, error),
                );
                None
            }
        };
        if let Some(monitor) = monitor {
            let mut reports = monitor.subscribe();
            loop {
                tokio::select! {
                    changed = reports.changed() => {
                        if changed.is_err() { break; }
                        let report = reports.borrow().clone();
                        publish(&app, &service, &report);
                    }
                    _ = generation.changed() => break,
                }
            }
        } else if generation.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_stable() {
        assert_eq!(PRODUCT_NAME, "Next Agent");
        assert_eq!(APP_IDENTIFIER, "dev.nextagent.desktop");
    }
}

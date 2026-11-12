//! Tauri 2 shell for Next Agent.

pub mod commands;
pub mod review;
pub mod service;
pub mod tasks;
pub mod wire;

use next_agent_core::router::{KeyringSecret, RouterClient, RouterSettings};
use next_agent_process_manager::{HealthMonitor, HealthReport, HealthStatus, MonitorConfig};
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
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let service = Arc::new(AppService::new(dir.join("next-agent.db"))?);
            app.manage(Arc::clone(&service));
            tauri::async_runtime::spawn(poll_router_health(app.handle().clone(), service));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            commands::approval_respond,
            commands::review_action,
            commands::router_health,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Next Agent");
}

/// Router client for the configured settings. The API key is optional for
/// health probes: without one the router answers 401 and shows `degraded`.
fn router_client(service: &AppService) -> Result<RouterClient, String> {
    let settings = service
        .store()
        .ok()
        .and_then(|s| RouterSettings::load(&s).ok())
        .unwrap_or_default();
    RouterClient::from_secret(settings.clone(), &KeyringSecret::default())
        .or_else(|_| RouterClient::new(settings, None))
        .map_err(|e| e.to_string())
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
        let monitor = match router_client(&service) {
            Ok(client) => Some(HealthMonitor::spawn(
                Arc::new(client),
                MonitorConfig::default(),
            )),
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

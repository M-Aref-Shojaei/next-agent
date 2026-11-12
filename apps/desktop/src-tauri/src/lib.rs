//! Tauri 2 shell for Next Agent.

pub mod commands;
pub mod service;
pub mod wire;

use std::sync::Arc;
use tauri::Manager;

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
            let service = service::AppService::new(dir.join("next-agent.db"))?;
            app.manage(Arc::new(service));
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Next Agent");
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

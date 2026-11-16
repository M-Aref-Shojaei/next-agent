// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src-tauri/src/webview_memory.rs @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
#![cfg_attr(not(target_os = "macos"), allow(unsafe_code))] // Next Agent: upstream's Windows/Linux calls (ADR-0011)
// Memory of the webviews that nobody is looking at.
//
// Coucou keeps four WebView2 windows alive (the island, the Coucou window, the
// pill, Mochi on the desktop): windows made later come up blank (see
// create_settings_window in lib.rs), so they cannot simply be closed and made
// again. What WebView2 offers instead is a memory target: at "low" it trims what
// a webview holds while it is hidden, and gives it back when it is shown.

use tauri::WebviewWindow;

/// Asks WebView2 to keep `win`'s webview small (`low`) or normal. Best effort: an old
/// WebView2 runtime without the setting, or a window that is not ready yet, is left as is.
#[cfg(windows)]
pub fn set_low(win: &WebviewWindow, low: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use wv2_core::Interface;
    let label = win.label().to_string();
    let result = win.with_webview(move |webview| unsafe {
        let Ok(core) = webview.controller().CoreWebView2() else { return };
        let Ok(core) = core.cast::<ICoreWebView2_19>() else { return }; // an old WebView2 runtime
        let level = if low {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
        } else {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
        };
        if let Err(err) = core.SetMemoryUsageTargetLevel(level) {
            // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
            tracing::warn!(target: "overlay", window = %label, error = %err, "webview_memory");
        }
    });
    if let Err(err) = result {
        // Next Agent: event name plus allowlisted fields for the shared diagnostics log (ADR-0013).
        tracing::warn!(target: "overlay", error = %err, kind = "with_webview_failed", "webview_memory");
    }
}

#[cfg(not(windows))]
pub fn set_low(_win: &WebviewWindow, _low: bool) {}

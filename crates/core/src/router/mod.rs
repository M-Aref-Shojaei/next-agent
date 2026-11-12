//! 9Router (OpenAI-compatible) adapter — the only module that speaks HTTP.
//!
//! - [`settings`]: base URL / default model in the `settings` table, API key
//!   in the OS keychain.
//! - [`client`]: health (`GET {base}/models`) and streaming chat completions
//!   (`POST {base}/chat/completions`, SSE), implementing
//!   [`CompletionClient`](crate::llm::CompletionClient) and
//!   [`HealthProbe`](next_agent_process_manager::HealthProbe).
//! - [`fetch`]: the `http_fetch` tool's transport, kept here so HTTP stays in
//!   one adapter (see `docs/architecture/Module Boundaries.md`).
//!
//! Router calls are the app's own LLM channel: they bypass network permission
//! prompts but are logged (metadata only) through `tracing`.

pub mod client;
pub mod fetch;
pub mod settings;

pub use client::{RetryPolicy, RouterClient};
pub use fetch::ReqwestFetcher;
pub use settings::{
    KeyringSecret, MemorySecret, RouterSettings, SecretError, SecretStore, DEFAULT_BASE_URL,
};

use std::future::Future;
use std::sync::OnceLock;

/// Drive a future to completion from synchronous code (the task thread).
///
/// Uses the ambient tokio runtime when called from one of its blocking
/// threads (e.g. `spawn_blocking`), otherwise a private runtime shared by all
/// router clients. Must not be called from an async worker thread.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return handle.block_on(future);
    }
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("router-io")
                .enable_all()
                .build()
                .expect("failed to build the router I/O runtime")
        })
        .block_on(future)
}

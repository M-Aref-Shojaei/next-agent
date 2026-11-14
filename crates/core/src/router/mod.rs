//! LLM transport — the only module that speaks HTTP.
//!
//! - [`gateway`]: the built-in LLM gateway (ported from 9Router): provider
//!   connections, OpenAI ⇄ Anthropic ⇄ Gemini translation ([`translate`]),
//!   combos with fallback and cooldown ([`combo`]), smart combos that keep a
//!   model per task ([`smart`]), token savers
//!   ([`savers`]), usage and cost ([`pricing`]) and model roles. Implements
//!   [`CompletionClient`](crate::llm::CompletionClient) and
//!   [`HealthProbe`](next_agent_process_manager::HealthProbe).
//! - [`providers`]: connections and presets; keys in the OS keychain.
//! - [`settings`]: legacy `router.*` settings (an external OpenAI-compatible
//!   router such as 9Router) and the keychain entry type.
//! - [`client`]: client for a single external OpenAI-compatible router.
//! - [`fetch`]: the `http_fetch` tool's transport, kept here so HTTP stays in
//!   one adapter (see `docs/architecture/Module Boundaries.md`).
//!
//! Router calls are the app's own LLM channel: they bypass network permission
//! prompts but are logged (metadata only) through `tracing`.

// Provider and format names (OpenAI, Gemini, SSE…) appear in prose throughout.
#![allow(clippy::doc_markdown)]

pub mod client;
pub mod combo;
pub mod fetch;
pub mod gateway;
pub mod pricing;
pub mod providers;
pub mod savers;
pub mod settings;
pub mod smart;
pub mod translate;

pub use client::{RetryPolicy, RouterClient};
pub use combo::{Combo, CooldownPolicy, GatewayState, Strategy};
pub use fetch::ReqwestFetcher;
pub use gateway::{
    Gateway, GatewayConfig, GatewaySettings, RequestOptions, StoreUsage, UsageSink, ROLES,
};
pub use pricing::Pricing;
pub use providers::{
    Connection, ConnectionData, KeySource, KeychainKeys, MemoryKeys, ProviderKind, PRESETS,
};
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

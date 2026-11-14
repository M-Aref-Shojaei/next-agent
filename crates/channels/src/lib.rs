#![forbid(unsafe_code)]

//! Channel gateway: chat channels (Telegram first) as another way to start
//! tasks, inside the app. Design ported from OpenClaw (MIT, © 2026 OpenClaw
//! Foundation) at commit `1ac07b3`; see `docs/upstream/OpenClaw.md` and
//! `docs/architecture/Channels.md`.
//!
//! Our rules are stricter than OpenClaw's:
//! - channel tasks run only in `manual` or `plan` mode, never bypass
//!   ([`config::ChannelMode`], [`config::guard_mode`]);
//! - a deny-by-default tool profile that can only tighten ([`profile`]);
//! - inbound text is wrapped as untrusted ([`untrusted`]);
//! - approvals happen only on the desktop or the overlay; none in chat, and
//!   no answer in time is a deny;
//! - bot tokens live in the OS keychain ([`secret`]), never in SQLite.

pub mod channel;
pub mod chunk;
pub mod config;
pub mod hub;
pub mod outbound;
pub mod pairing;
pub mod policy;
pub mod profile;
pub mod queue;
pub mod routing;
pub mod secret;
pub mod state;
pub mod supervisor;
pub mod telegram;
pub mod types;
pub mod untrusted;

pub use channel::{BoxFuture, Channel, ChannelError};
pub use config::{ChannelConfig, ChannelMode, DmPolicy, DmScope, GroupPolicy};
pub use hub::{ChannelJob, Delivery, Handled, Hub, HubEvent, TaskOutcome, TaskProgress, TaskRunner};
pub use pairing::PairingRequest;
pub use profile::{channel_profile, ChannelExecutor};
pub use queue::StopToken;
pub use state::{Activity, AllowEntry, ChannelState, MemoryState, StoreState};
pub use supervisor::{supervise, Backoff, Health, HealthState, Supervised};
pub use types::{BotIdentity, Capabilities, ChatKind, Inbound, MessageRef, Outbound};

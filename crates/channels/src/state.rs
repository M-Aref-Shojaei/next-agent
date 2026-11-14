//! Persistent channel state behind a trait: pending pairings, the
//! allow-list, idempotency keys and the activity log. The app implements it
//! over the SQLite store ([`StoreState`]); tests use [`MemoryState`].
//! Bot tokens are never part of it.

use crate::pairing::PairingRequest;
use next_agent_core::store::{ChannelActivityRow, ChannelAllowRow, ChannelPairingRow};
use next_agent_core::Store;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

/// A sender allowed to talk to the bot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowEntry {
    pub sender_id: String,
    pub label: String,
    /// `pairing` (approved code) or `manual` (added in Settings).
    pub source: String,
    pub added_at: String,
}

/// One line of the activity log shown in Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub ts: String,
    pub channel_id: String,
    /// `in`, `out` or `system`.
    pub direction: String,
    /// `message`, `pairing`, `denied`, `task`, `error` …
    pub kind: String,
    pub peer: String,
    pub text: String,
    pub task_id: Option<String>,
}

/// Storage the hub needs. Every method fails only with a storage error,
/// as a human-readable string.
#[allow(clippy::missing_errors_doc)]
pub trait ChannelState: Send + Sync {
    fn pairings(&self, channel_id: &str) -> Result<Vec<PairingRequest>, String>;
    /// Replace the pending list of `channel_id`.
    fn set_pairings(&self, channel_id: &str, pending: &[PairingRequest]) -> Result<(), String>;

    fn allow_list(&self, channel_id: &str) -> Result<Vec<AllowEntry>, String>;
    fn allow_add(&self, channel_id: &str, entry: &AllowEntry) -> Result<(), String>;
    fn allow_remove(&self, channel_id: &str, sender_id: &str) -> Result<bool, String>;

    /// Record `key`; `false` if it was already recorded (a duplicate).
    fn claim(&self, key: &str, value: &str) -> Result<bool, String>;
    /// The value stored with `key`, if any.
    fn claimed(&self, key: &str) -> Result<Option<String>, String>;

    fn log(&self, entry: &Activity) -> Result<(), String>;
    fn activity(&self, channel_id: &str, limit: usize) -> Result<Vec<Activity>, String>;
}

/// In-memory [`ChannelState`] for tests and headless use.
#[derive(Debug, Default)]
pub struct MemoryState {
    inner: Mutex<Memory>,
}

#[derive(Debug, Default)]
struct Memory {
    pairings: HashMap<String, Vec<PairingRequest>>,
    allow: HashMap<String, Vec<AllowEntry>>,
    keys: HashMap<String, String>,
    activity: Vec<Activity>,
}

impl MemoryState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Memory> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ChannelState for MemoryState {
    fn pairings(&self, channel_id: &str) -> Result<Vec<PairingRequest>, String> {
        Ok(self
            .lock()
            .pairings
            .get(channel_id)
            .cloned()
            .unwrap_or_default())
    }

    fn set_pairings(&self, channel_id: &str, pending: &[PairingRequest]) -> Result<(), String> {
        self.lock()
            .pairings
            .insert(channel_id.to_string(), pending.to_vec());
        Ok(())
    }

    fn allow_list(&self, channel_id: &str) -> Result<Vec<AllowEntry>, String> {
        Ok(self
            .lock()
            .allow
            .get(channel_id)
            .cloned()
            .unwrap_or_default())
    }

    fn allow_add(&self, channel_id: &str, entry: &AllowEntry) -> Result<(), String> {
        let mut inner = self.lock();
        let list = inner.allow.entry(channel_id.to_string()).or_default();
        list.retain(|e| e.sender_id != entry.sender_id);
        list.push(entry.clone());
        Ok(())
    }

    fn allow_remove(&self, channel_id: &str, sender_id: &str) -> Result<bool, String> {
        let mut inner = self.lock();
        let Some(list) = inner.allow.get_mut(channel_id) else {
            return Ok(false);
        };
        let before = list.len();
        list.retain(|e| e.sender_id != sender_id);
        Ok(list.len() != before)
    }

    fn claim(&self, key: &str, value: &str) -> Result<bool, String> {
        let mut inner = self.lock();
        if inner.keys.contains_key(key) {
            return Ok(false);
        }
        inner.keys.insert(key.to_string(), value.to_string());
        Ok(true)
    }

    fn claimed(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.lock().keys.get(key).cloned())
    }

    fn log(&self, entry: &Activity) -> Result<(), String> {
        self.lock().activity.push(entry.clone());
        Ok(())
    }

    fn activity(&self, channel_id: &str, limit: usize) -> Result<Vec<Activity>, String> {
        Ok(self
            .lock()
            .activity
            .iter()
            .rev()
            .filter(|a| a.channel_id == channel_id)
            .take(limit)
            .cloned()
            .collect())
    }
}

/// [`ChannelState`] over the app's SQLite store (tables in the `channels`
/// block of `crates/core/src/store.rs`), on one long-lived connection.
#[derive(Debug)]
pub struct StoreState {
    store: Mutex<Store>,
}

impl StoreState {
    /// Open the database (and apply migrations) once.
    ///
    /// # Errors
    /// The database cannot be opened.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, String> {
        Ok(Self {
            store: Mutex::new(Store::open(db_path).map_err(|e| e.to_string())?),
        })
    }

    #[allow(clippy::unnecessary_wraps)]
    fn store(&self) -> Result<std::sync::MutexGuard<'_, Store>, String> {
        Ok(self.store.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

fn to_row(r: &PairingRequest) -> ChannelPairingRow {
    ChannelPairingRow {
        channel_id: r.channel_id.clone(),
        sender_id: r.sender_id.clone(),
        chat_id: r.chat_id.clone(),
        label: r.label.clone(),
        code: r.code.clone(),
        created_at: r.created_at,
        last_seen_at: r.last_seen_at,
    }
}

fn from_row(r: ChannelPairingRow) -> PairingRequest {
    PairingRequest {
        channel_id: r.channel_id,
        sender_id: r.sender_id,
        chat_id: r.chat_id,
        label: r.label,
        code: r.code,
        created_at: r.created_at,
        last_seen_at: r.last_seen_at,
    }
}

impl ChannelState for StoreState {
    fn pairings(&self, channel_id: &str) -> Result<Vec<PairingRequest>, String> {
        let rows = self
            .store()?
            .channel_pairings(channel_id)
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(from_row).collect())
    }

    fn set_pairings(&self, channel_id: &str, pending: &[PairingRequest]) -> Result<(), String> {
        let rows: Vec<ChannelPairingRow> = pending.iter().map(to_row).collect();
        self.store()?
            .channel_pairings_replace(channel_id, &rows)
            .map_err(|e| e.to_string())
    }

    fn allow_list(&self, channel_id: &str) -> Result<Vec<AllowEntry>, String> {
        let rows = self
            .store()?
            .channel_allow_list(channel_id)
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|r| AllowEntry {
                sender_id: r.sender_id,
                label: r.label,
                source: r.source,
                added_at: r.added_at,
            })
            .collect())
    }

    fn allow_add(&self, channel_id: &str, entry: &AllowEntry) -> Result<(), String> {
        self.store()?
            .channel_allow_add(&ChannelAllowRow {
                channel_id: channel_id.to_string(),
                sender_id: entry.sender_id.clone(),
                label: entry.label.clone(),
                source: entry.source.clone(),
                added_at: String::new(),
            })
            .map_err(|e| e.to_string())
    }

    fn allow_remove(&self, channel_id: &str, sender_id: &str) -> Result<bool, String> {
        self.store()?
            .channel_allow_remove(channel_id, sender_id)
            .map_err(|e| e.to_string())
    }

    fn claim(&self, key: &str, value: &str) -> Result<bool, String> {
        self.store()?
            .channel_claim(key, value)
            .map_err(|e| e.to_string())
    }

    fn claimed(&self, key: &str) -> Result<Option<String>, String> {
        self.store()?
            .channel_claimed(key)
            .map_err(|e| e.to_string())
    }

    fn log(&self, entry: &Activity) -> Result<(), String> {
        self.store()?
            .channel_activity_add(&ChannelActivityRow {
                ts: String::new(),
                channel_id: entry.channel_id.clone(),
                direction: entry.direction.clone(),
                kind: entry.kind.clone(),
                peer: entry.peer.clone(),
                text: entry.text.clone(),
                task_id: entry.task_id.clone(),
            })
            .map_err(|e| e.to_string())
    }

    fn activity(&self, channel_id: &str, limit: usize) -> Result<Vec<Activity>, String> {
        let rows = self
            .store()?
            .channel_activity(channel_id, limit)
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|r| Activity {
                ts: r.ts,
                channel_id: r.channel_id,
                direction: r.direction,
                kind: r.kind,
                peer: r.peer,
                text: r.text,
                task_id: r.task_id,
            })
            .collect())
    }
}

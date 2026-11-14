//! SQLite metadata storage for workspaces, tasks, events and settings.
//!
//! The `WebView` never touches the database; this module is the only writer.

use crate::workspace::Workspace;
use std::path::Path;

/// Schema version applied by [`Store::open`].
/// 3: Wave 1 tables (agent-engine snapshots, llm-gateway connections, combos,
/// usage, smart stickiness, response cache).
pub const SCHEMA_VERSION: i32 = 3;

/// All schema migrations, in order. Every statement is idempotent so `open`
/// can run the whole list on each start and converge to [`SCHEMA_VERSION`].
pub const MIGRATIONS: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS workspaces (
        id TEXT PRIMARY KEY,
        path TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        last_opened_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        default_mode TEXT NOT NULL DEFAULT 'manual'
            CHECK (default_mode IN ('plan','manual','auto','bypassPermissions'))
    )",
    "CREATE TABLE IF NOT EXISTS vaults (
        id TEXT PRIMARY KEY,
        path TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    )",
    "CREATE TABLE IF NOT EXISTS workspace_vaults (
        workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
        vault_id TEXT NOT NULL REFERENCES vaults(id) ON DELETE CASCADE,
        subfolder TEXT,
        PRIMARY KEY (workspace_id, vault_id)
    )",
    "CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
        mode TEXT NOT NULL CHECK (mode IN ('plan','manual','auto','bypassPermissions')),
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    )",
    "CREATE TABLE IF NOT EXISTS tasks (
        id TEXT PRIMARY KEY,
        session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
        workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
        title TEXT NOT NULL,
        prompt TEXT NOT NULL,
        mode TEXT NOT NULL CHECK (mode IN ('plan','manual','auto','bypassPermissions')),
        executor TEXT,
        model TEXT,
        status TEXT NOT NULL DEFAULT 'pending'
            CHECK (status IN ('pending','running','completed','failed','cancelled')),
        base_commit TEXT,
        worktree_path TEXT,
        branch TEXT,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        finished_at TEXT,
        vault_record_path TEXT
    )",
    "CREATE INDEX IF NOT EXISTS tasks_by_workspace ON tasks(workspace_id, created_at DESC)",
    "CREATE TABLE IF NOT EXISTS events (
        task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
        seq INTEGER NOT NULL,
        id TEXT NOT NULL UNIQUE,
        kind TEXT NOT NULL,
        data TEXT NOT NULL DEFAULT '{}',
        ts TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        PRIMARY KEY (task_id, seq)
    )",
    "CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    )",
    // --- v2: permission engine storage ---------------------------------------
    "CREATE TABLE IF NOT EXISTS permission_rules (
        id TEXT PRIMARY KEY,
        scope TEXT NOT NULL CHECK (scope IN ('global','workspace')),
        workspace_id TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
        effect TEXT NOT NULL CHECK (effect IN ('allow','deny')),
        kind TEXT NOT NULL,
        pattern TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        created_by TEXT NOT NULL DEFAULT 'user',
        CHECK (scope = 'global' OR workspace_id IS NOT NULL)
    )",
    "CREATE INDEX IF NOT EXISTS rules_by_workspace ON permission_rules(workspace_id, effect)",
    "CREATE TABLE IF NOT EXISTS audit_log (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        ts TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        -- Deliberately not a foreign key: the table is append-only, and a
        -- cascade or SET NULL would be an UPDATE that the triggers below reject.
        task_id TEXT,
        mode TEXT NOT NULL,
        action TEXT NOT NULL,
        target TEXT NOT NULL,
        decision TEXT NOT NULL,
        reason TEXT NOT NULL,
        actor TEXT NOT NULL DEFAULT 'system',
        prev_hash TEXT NOT NULL,
        hash TEXT NOT NULL UNIQUE
    )",
    "CREATE TRIGGER IF NOT EXISTS audit_log_no_update
        BEFORE UPDATE ON audit_log
        BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END",
    "CREATE TRIGGER IF NOT EXISTS audit_log_no_delete
        BEFORE DELETE ON audit_log
        BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END",
    // --- agent-engine ---
    // Shadow-git snapshots taken before each mutating step (per-step undo).
    "CREATE TABLE IF NOT EXISTS agent_step_snapshots (
        task_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        call_id TEXT NOT NULL,
        tool TEXT NOT NULL,
        work_tree TEXT NOT NULL,
        git_dir TEXT NOT NULL,
        tree TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        undone_at TEXT,
        PRIMARY KEY (task_id, seq)
    )",
    // --- end agent-engine ---
    // --- llm-gateway ---
    // Provider connections. Secrets (API keys) are never stored here: they
    // live in the OS keychain, one entry per connection id.
    "CREATE TABLE IF NOT EXISTS llm_connections (
        id TEXT PRIMARY KEY,
        provider TEXT NOT NULL,
        auth_type TEXT NOT NULL DEFAULT 'api_key',
        name TEXT NOT NULL,
        priority INTEGER NOT NULL DEFAULT 100,
        is_active INTEGER NOT NULL DEFAULT 1,
        data TEXT NOT NULL DEFAULT '{}',
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    )",
    "CREATE TABLE IF NOT EXISTS llm_combos (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        kind TEXT NOT NULL DEFAULT 'llm',
        models TEXT NOT NULL DEFAULT '[]',
        strategy TEXT,
        sticky_limit INTEGER,
        confidence_threshold REAL,
        weights TEXT
    )",
    "CREATE TABLE IF NOT EXISTS llm_usage (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        ts TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        connection_id TEXT,
        role TEXT NOT NULL DEFAULT '',
        combo TEXT,
        endpoint TEXT NOT NULL DEFAULT 'chat',
        status INTEGER NOT NULL,
        prompt_tokens INTEGER NOT NULL DEFAULT 0,
        completion_tokens INTEGER NOT NULL DEFAULT 0,
        cached_tokens INTEGER NOT NULL DEFAULT 0,
        cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
        reasoning_tokens INTEGER NOT NULL DEFAULT 0,
        cost REAL NOT NULL DEFAULT 0,
        cost_known INTEGER NOT NULL DEFAULT 0,
        latency_ms INTEGER NOT NULL DEFAULT 0,
        saver_bytes_before INTEGER NOT NULL DEFAULT 0,
        saver_bytes_after INTEGER NOT NULL DEFAULT 0
    )",
    "CREATE INDEX IF NOT EXISTS llm_usage_by_ts ON llm_usage(ts DESC)",
    "CREATE TABLE IF NOT EXISTS llm_usage_daily (
        day TEXT NOT NULL,
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        role TEXT NOT NULL,
        requests INTEGER NOT NULL DEFAULT 0,
        failures INTEGER NOT NULL DEFAULT 0,
        prompt_tokens INTEGER NOT NULL DEFAULT 0,
        completion_tokens INTEGER NOT NULL DEFAULT 0,
        cached_tokens INTEGER NOT NULL DEFAULT 0,
        reasoning_tokens INTEGER NOT NULL DEFAULT 0,
        cost REAL NOT NULL DEFAULT 0,
        PRIMARY KEY (day, provider, model, role)
    )",
    // Smart combos: the sticky model of each session (task) per combo, with
    // its recent quality window (JSON array of booleans, `true` = bad).
    "CREATE TABLE IF NOT EXISTS llm_smart_sticky (
        session TEXT NOT NULL,
        combo TEXT NOT NULL,
        model TEXT NOT NULL,
        quality TEXT NOT NULL DEFAULT '[]',
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        PRIMARY KEY (session, combo)
    )",
    "CREATE TABLE IF NOT EXISTS llm_switches (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        ts TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        session TEXT NOT NULL,
        combo TEXT NOT NULL,
        from_model TEXT NOT NULL,
        to_model TEXT NOT NULL,
        reason TEXT NOT NULL,
        confidence REAL NOT NULL DEFAULT 1
    )",
    // Exact-match response cache. `key` is a SHA-256 of the canonical
    // request (no account, no key material); `body` is the stored answer.
    "CREATE TABLE IF NOT EXISTS llm_response_cache (
        key TEXT PRIMARY KEY,
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        body TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS llm_cache_hits (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        ts TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        role TEXT NOT NULL DEFAULT '',
        saved_tokens INTEGER NOT NULL DEFAULT 0,
        saved_cost REAL NOT NULL DEFAULT 0
    )",
    // --- end llm-gateway ---
    // --- channels ---
    // Channel accounts (Telegram bots …). `config` is non-secret JSON; bot
    // tokens live in the OS keychain (`channel:<id>`), never here.
    "CREATE TABLE IF NOT EXISTS channels (
        id TEXT PRIMARY KEY,
        kind TEXT NOT NULL,
        name TEXT NOT NULL,
        config TEXT NOT NULL DEFAULT '{}',
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    )",
    // DM pairing requests waiting for the owner (at most 3 per channel,
    // 1 h expiry; enforced in crates/channels/src/pairing.rs).
    "CREATE TABLE IF NOT EXISTS channel_pairings (
        channel_id TEXT NOT NULL,
        sender_id TEXT NOT NULL,
        chat_id TEXT NOT NULL,
        label TEXT NOT NULL DEFAULT '',
        code TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        last_seen_at INTEGER NOT NULL,
        PRIMARY KEY (channel_id, sender_id)
    )",
    // Senders allowed to talk to a channel (approved pairings and manual
    // entries; `*` = anyone, required for the open DM policy).
    "CREATE TABLE IF NOT EXISTS channel_allow (
        channel_id TEXT NOT NULL,
        sender_id TEXT NOT NULL,
        label TEXT NOT NULL DEFAULT '',
        source TEXT NOT NULL DEFAULT 'manual',
        added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        PRIMARY KEY (channel_id, sender_id)
    )",
    // Idempotency keys: inbound updates already handled and outbound chunks
    // already sent (value = sent message id). Pruned after 30 days.
    "CREATE TABLE IF NOT EXISTS channel_idempotency (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL DEFAULT '',
        created_at INTEGER NOT NULL DEFAULT (CAST(strftime('%s','now') AS INTEGER))
    )",
    "CREATE TABLE IF NOT EXISTS channel_activity (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        ts TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        channel_id TEXT NOT NULL,
        direction TEXT NOT NULL,
        kind TEXT NOT NULL,
        peer TEXT NOT NULL DEFAULT '',
        text TEXT NOT NULL DEFAULT '',
        task_id TEXT
    )",
    "CREATE INDEX IF NOT EXISTS channel_activity_by_channel ON channel_activity(channel_id, id DESC)",
    // --- end channels ---
];

/// Handle to the metadata database.
#[derive(Debug)]
pub struct Store {
    conn: rusqlite::Connection,
}

/// Storage failures surfaced to the UI as recoverable errors.
#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    JoinPath(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "database error: {e}"),
            Self::JoinPath(p) => write!(f, "workspace path is not valid UTF-8: {p}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl Store {
    /// Open (or create) the metadata database and apply migrations.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the file cannot be opened or a migration fails.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let conn = rusqlite::Connection::open(path)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// In-memory database for tests.
    ///
    /// # Errors
    /// Returns [`StoreError`] if migration fails.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = rusqlite::Connection::open_in_memory()?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), StoreError> {
        // Pragmas do not accept bound parameters, so they go through pragma_update.
        self.conn.pragma_update(None, "journal_mode", "WAL")?;
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        for statement in MIGRATIONS {
            self.conn.execute_batch(statement)?;
        }
        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    /// Current schema version recorded in the file.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the pragma cannot be read.
    pub fn schema_version(&self) -> Result<i32, StoreError> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    /// Insert or refresh a workspace, returning whether it was new.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the path is not UTF-8 or the write fails.
    pub fn upsert_workspace(&self, workspace: &Workspace) -> Result<bool, StoreError> {
        let path = workspace
            .root()
            .to_str()
            .ok_or_else(|| StoreError::JoinPath(workspace.root().display().to_string()))?;
        // SQLite counts an upsert's UPDATE branch as a change too, so check
        // for an existing row first to tell insert from update.
        let existed: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM workspaces WHERE path = ?1)",
            [path],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO workspaces (id, path, name) VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET last_opened_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
            rusqlite::params![workspace.id(), path, workspace.name()],
        )?;
        Ok(!existed)
    }

    /// All known workspaces, most recently opened first.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn list_workspaces(&self) -> Result<Vec<StoredWorkspace>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT id, path, name FROM workspaces ORDER BY last_opened_at DESC, name ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredWorkspace {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Append one event to a task's timeline (`data` is a JSON object).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the task row is missing or the sequence repeats.
    pub fn append_event(
        &self,
        task_id: &str,
        seq: u64,
        kind: &str,
        data: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO events (task_id, seq, id, kind, data) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![task_id, seq, format!("{task_id}:{seq}"), kind, data],
        )?;
        Ok(())
    }

    /// Kinds of a task's events in sequence order.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn event_kinds(&self, task_id: &str) -> Result<Vec<String>, StoreError> {
        let mut statement = self
            .conn
            .prepare("SELECT kind FROM events WHERE task_id = ?1 ORDER BY seq ASC")?;
        let rows = statement.query_map(rusqlite::params![task_id], |row| row.get(0))?;
        Ok(rows.collect::<Result<Vec<String>, _>>()?)
    }

    /// A task's persisted events as `(seq, kind, data_json)`, in order —
    /// enough to replay the timeline after a restart.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn task_events(&self, task_id: &str) -> Result<Vec<(u64, String, String)>, StoreError> {
        let mut statement = self
            .conn
            .prepare("SELECT seq, kind, data FROM events WHERE task_id = ?1 ORDER BY seq ASC")?;
        let rows = statement.query_map(rusqlite::params![task_id], |row| {
            Ok((row.get::<_, u64>(0)?, row.get(1)?, row.get(2)?))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Record a task's lifecycle status (and executor, once known). Terminal
    /// statuses also stamp `finished_at`.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the update fails.
    pub fn set_task_status(
        &self,
        task_id: &str,
        status: crate::task::TaskStatus,
        executor: Option<&str>,
    ) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "UPDATE tasks SET status = ?2,
                 executor = COALESCE(?3, executor),
                 finished_at = CASE WHEN ?4 THEN strftime('%Y-%m-%dT%H:%M:%fZ','now') ELSE finished_at END
             WHERE id = ?1",
            rusqlite::params![task_id, status.as_str(), executor, status.is_terminal()],
        )?;
        Ok(changed > 0)
    }

    /// Current status of a task, if the row exists.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn task_status(
        &self,
        task_id: &str,
    ) -> Result<Option<crate::task::TaskStatus>, StoreError> {
        let mut statement = self
            .conn
            .prepare("SELECT status FROM tasks WHERE id = ?1")?;
        let mut rows = statement.query(rusqlite::params![task_id])?;
        Ok(match rows.next()? {
            Some(row) => crate::task::TaskStatus::parse(&row.get::<_, String>(0)?),
            None => None,
        })
    }

    /// Create a task row so events can reference it.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the workspace does not exist or the id repeats.
    pub fn create_task(
        &self,
        task_id: &str,
        workspace_id: &str,
        title: &str,
        prompt: &str,
        mode: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO tasks (id, workspace_id, title, prompt, mode) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![task_id, workspace_id, title, prompt, mode],
        )?;
        Ok(())
    }

    /// Persist a JSON setting.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }

    /// Read a JSON setting.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        let mut statement = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = statement.query(rusqlite::params![key])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    /// Persist a permission rule. `kind` is the tool name or `path`/`command`.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the scope/workspace combination is invalid.
    pub fn insert_rule(&self, rule: &RuleRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO permission_rules (id, scope, workspace_id, effect, kind, pattern)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET effect = excluded.effect, pattern = excluded.pattern",
            rusqlite::params![
                rule.id,
                rule.scope,
                rule.workspace_id,
                rule.effect,
                rule.kind,
                rule.pattern
            ],
        )?;
        Ok(())
    }

    /// Rules that apply to a workspace: its own plus the global ones.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn rules_for(&self, workspace_id: &str) -> Result<Vec<RuleRow>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT id, scope, workspace_id, effect, kind, pattern FROM permission_rules
             WHERE scope = 'global' OR workspace_id = ?1
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = statement.query_map(rusqlite::params![workspace_id], |row| {
            Ok(RuleRow {
                id: row.get(0)?,
                scope: row.get(1)?,
                workspace_id: row.get(2)?,
                effect: row.get(3)?,
                kind: row.get(4)?,
                pattern: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Remove a rule by id, returning whether a row was deleted.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn delete_rule(&self, id: &str) -> Result<bool, StoreError> {
        let removed = self.conn.execute(
            "DELETE FROM permission_rules WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(removed == 1)
    }

    /// Append to the tamper-evident audit trail.
    ///
    /// The hash covers the previous hash plus every column of this row, so
    /// editing any earlier row breaks verification of everything after it.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn append_audit(&self, entry: &AuditEntry<'_>) -> Result<String, StoreError> {
        let prev_hash = self.last_audit_hash()?.unwrap_or_else(|| "genesis".into());
        let payload = format!(
            "{prev_hash}|{}|{}|{}|{}|{}|{}|{}",
            entry.mode,
            entry.action,
            entry.target,
            entry.decision,
            entry.reason,
            entry.actor,
            entry.task_id.unwrap_or("")
        );
        let hash = crate::hash::sha256_hex(payload.as_bytes());
        self.conn.execute(
            "INSERT INTO audit_log (task_id, mode, action, target, decision, reason, actor, prev_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                entry.task_id,
                entry.mode,
                entry.action,
                entry.target,
                entry.decision,
                entry.reason,
                entry.actor,
                prev_hash,
                hash
            ],
        )?;
        Ok(hash)
    }

    /// Hash of the newest audit row, if the log is not empty.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn last_audit_hash(&self) -> Result<Option<String>, StoreError> {
        let mut statement = self
            .conn
            .prepare("SELECT hash FROM audit_log ORDER BY seq DESC LIMIT 1")?;
        let mut rows = statement.query([])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    /// Recompute the chain and report the first row that does not match.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn verify_audit(&self) -> Result<AuditVerification, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT seq, task_id, mode, action, target, decision, reason, actor, prev_hash, hash
             FROM audit_log ORDER BY seq ASC",
        )?;
        let mut rows = statement.query([])?;
        let mut expected_prev = "genesis".to_string();
        let mut checked = 0usize;
        while let Some(row) = rows.next()? {
            let seq: i64 = row.get(0)?;
            let task_id: Option<String> = row.get(1)?;
            let mode: String = row.get(2)?;
            let action: String = row.get(3)?;
            let target: String = row.get(4)?;
            let decision: String = row.get(5)?;
            let reason: String = row.get(6)?;
            let actor: String = row.get(7)?;
            let prev_hash: String = row.get(8)?;
            let hash: String = row.get(9)?;
            let payload = format!(
                "{prev_hash}|{mode}|{action}|{target}|{decision}|{reason}|{actor}|{}",
                task_id.unwrap_or_default()
            );
            let recomputed = crate::hash::sha256_hex(payload.as_bytes());
            if prev_hash != expected_prev || recomputed != hash {
                return Ok(AuditVerification {
                    ok: false,
                    checked,
                    broken_at: Some(seq),
                });
            }
            expected_prev = hash;
            checked += 1;
        }
        Ok(AuditVerification {
            ok: true,
            checked,
            broken_at: None,
        })
    }
}

/// Workspace row as stored on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredWorkspace {
    pub id: String,
    pub path: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_default_mode_is_rejected() {
        let store = Store::open_in_memory().expect("open");
        let result = store.conn.execute(
            "INSERT INTO workspaces (id, path, name, default_mode)
             VALUES ('ws_x', '/tmp/x', 'x', 'yolo')",
            [],
        );
        assert!(result.is_err(), "invalid mode must be rejected");
    }

    #[test]
    fn foreign_keys_cascade_from_workspace() {
        let store = Store::open_in_memory().expect("open");
        store
            .conn
            .execute(
                "INSERT INTO workspaces (id, path, name) VALUES ('ws_1', '/tmp/one', 'one')",
                [],
            )
            .expect("workspace");
        store
            .conn
            .execute(
                "INSERT INTO tasks (id, workspace_id, title, prompt, mode)
                 VALUES ('tsk_1', 'ws_1', 't', 'p', 'auto')",
                [],
            )
            .expect("task");
        store
            .conn
            .execute("DELETE FROM workspaces WHERE id = 'ws_1'", [])
            .expect("delete");
        let remaining: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
            .expect("count");
        assert_eq!(remaining, 0);
    }

    fn rule(id: &str, scope: &str, workspace_id: Option<&str>) -> RuleRow {
        RuleRow {
            id: id.into(),
            scope: scope.into(),
            workspace_id: workspace_id.map(str::to_string),
            effect: "allow".into(),
            kind: "path".into(),
            pattern: "/work/project/**".into(),
        }
    }

    #[test]
    fn workspace_scoped_rules_require_a_workspace() {
        let store = Store::open_in_memory().expect("open");
        let result = store.conn.execute(
            "INSERT INTO permission_rules (id, scope, effect, kind, pattern)
             VALUES ('r', 'workspace', 'allow', 'path', '/x/**')",
            [],
        );
        assert!(result.is_err(), "scoped rule without workspace must fail");
    }

    #[test]
    fn rules_for_returns_global_plus_own_workspace_only() {
        let store = Store::open_in_memory().expect("open");
        for (id, path) in [("ws_1", "/tmp/one"), ("ws_2", "/tmp/two")] {
            store
                .conn
                .execute(
                    "INSERT INTO workspaces (id, path, name) VALUES (?1, ?2, ?1)",
                    rusqlite::params![id, path],
                )
                .expect("workspace");
        }
        store
            .insert_rule(&rule("global-a", "global", None))
            .expect("global");
        store
            .insert_rule(&rule("own", "workspace", Some("ws_1")))
            .expect("own");
        store
            .insert_rule(&rule("other", "workspace", Some("ws_2")))
            .expect("other");

        let ids: Vec<String> = store
            .rules_for("ws_1")
            .expect("rules")
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert!(ids.contains(&"global-a".to_string()));
        assert!(ids.contains(&"own".to_string()));
        assert!(!ids.contains(&"other".to_string()));
    }

    #[test]
    fn inserting_the_same_rule_id_updates_instead_of_failing() {
        let store = Store::open_in_memory().expect("open");
        store
            .insert_rule(&rule("r", "global", None))
            .expect("first");
        let mut updated = rule("r", "global", None);
        updated.effect = "deny".into();
        store.insert_rule(&updated).expect("update");
        assert_eq!(store.rules_for("ws_1").expect("rules").len(), 1);
        assert_eq!(store.rules_for("ws_1").expect("rules")[0].effect, "deny");
    }

    #[test]
    fn deleting_a_missing_rule_reports_false() {
        let store = Store::open_in_memory().expect("open");
        assert!(!store.delete_rule("nope").expect("delete"));
        store
            .insert_rule(&rule("r", "global", None))
            .expect("insert");
        assert!(store.delete_rule("r").expect("delete"));
    }

    #[test]
    fn audit_chain_verifies_after_several_appends() {
        let store = Store::open_in_memory().expect("open");
        for index in 0..5 {
            let action = format!("action-{index}");
            store
                .append_audit(&AuditEntry::new("auto", &action, "/w/a", "allow").reason("test"))
                .expect("append");
        }
        let report = store.verify_audit().expect("verify");
        assert!(report.ok);
        assert_eq!(report.checked, 5);
        assert_eq!(report.broken_at, None);
        assert!(store.last_audit_hash().expect("hash").is_some());
    }

    #[test]
    fn empty_chain_verifies() {
        let store = Store::open_in_memory().expect("open");
        assert_eq!(store.last_audit_hash().expect("hash"), None);
        let report = store.verify_audit().expect("verify");
        assert!(report.ok);
        assert_eq!(report.checked, 0);
    }

    #[test]
    fn audit_rows_cannot_be_updated_or_deleted() {
        let store = Store::open_in_memory().expect("open");
        store
            .append_audit(&AuditEntry::new("auto", "run_command", "ls", "allow"))
            .expect("append");
        assert!(store
            .conn
            .execute("UPDATE audit_log SET decision = 'deny'", [])
            .is_err());
        assert!(store.conn.execute("DELETE FROM audit_log", []).is_err());
        assert!(store.verify_audit().expect("verify").ok);
    }

    #[test]
    fn tampering_with_a_hash_breaks_verification_at_that_row() {
        let store = Store::open_in_memory().expect("open");
        for index in 0..3 {
            let action = format!("action-{index}");
            store
                .append_audit(&AuditEntry::new("auto", &action, "/w/a", "allow"))
                .expect("append");
        }
        // The append-only triggers block a normal UPDATE, so drop them first:
        // the point is that the verifier still catches a rewrite.
        store
            .conn
            .execute_batch("DROP TRIGGER audit_log_no_update;")
            .expect("drop");
        store
            .conn
            .execute(
                "UPDATE audit_log SET target = '/etc/passwd' WHERE seq = 2",
                [],
            )
            .expect("tamper");

        let report = store.verify_audit().expect("verify");
        assert!(!report.ok);
        assert_eq!(report.broken_at, Some(2));
        assert_eq!(report.checked, 1);
    }

    #[test]
    fn deleting_a_task_does_not_touch_the_audit_log() {
        let store = Store::open_in_memory().expect("open");
        store
            .conn
            .execute(
                "INSERT INTO workspaces (id, path, name) VALUES ('ws_1', '/tmp/one', 'one')",
                [],
            )
            .expect("workspace");
        store
            .conn
            .execute(
                "INSERT INTO tasks (id, workspace_id, title, prompt, mode)
                 VALUES ('tsk_1', 'ws_1', 't', 'p', 'auto')",
                [],
            )
            .expect("task");
        store
            .append_audit(&AuditEntry::new("auto", "write_file", "/w/a", "allow").task("tsk_1"))
            .expect("append");

        store
            .conn
            .execute("DELETE FROM tasks WHERE id = 'tsk_1'", [])
            .expect("delete task");
        let report = store.verify_audit().expect("verify");
        assert!(report.ok);
        assert_eq!(report.checked, 1);
    }

    #[test]
    fn schema_version_is_three() {
        let store = Store::open_in_memory().expect("open");
        assert_eq!(store.schema_version().expect("version"), 3);
        assert_eq!(SCHEMA_VERSION, 3);
    }
}

/// Row in `permission_rules`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRow {
    pub id: String,
    /// `global` or `workspace`.
    pub scope: String,
    pub workspace_id: Option<String>,
    /// `allow` or `deny`.
    pub effect: String,
    /// Tool name, or `path`/`command` for pattern rules.
    pub kind: String,
    /// JSON-encoded pattern (glob string or word array).
    pub pattern: String,
}

/// One audit record to append.
#[derive(Debug, Clone, Copy)]
pub struct AuditEntry<'a> {
    pub task_id: Option<&'a str>,
    pub mode: &'a str,
    pub action: &'a str,
    pub target: &'a str,
    pub decision: &'a str,
    pub reason: &'a str,
    pub actor: &'a str,
}

impl<'a> AuditEntry<'a> {
    #[must_use]
    pub fn new(mode: &'a str, action: &'a str, target: &'a str, decision: &'a str) -> Self {
        Self {
            task_id: None,
            mode,
            action,
            target,
            decision,
            reason: "",
            actor: "system",
        }
    }

    #[must_use]
    pub fn reason(mut self, reason: &'a str) -> Self {
        self.reason = reason;
        self
    }

    #[must_use]
    pub fn actor(mut self, actor: &'a str) -> Self {
        self.actor = actor;
        self
    }

    #[must_use]
    pub fn task(mut self, task_id: &'a str) -> Self {
        self.task_id = Some(task_id);
        self
    }
}

/// Result of [`Store::verify_audit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditVerification {
    pub ok: bool,
    pub checked: usize,
    /// `seq` of the first inconsistent row, when `ok` is false.
    pub broken_at: Option<i64>,
}

// --- agent-engine ---

/// One row of `agent_step_snapshots`: the work tree as it was right before a
/// mutating tool call ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSnapshotRow {
    pub task_id: String,
    pub seq: i64,
    pub call_id: String,
    pub tool: String,
    pub work_tree: String,
    pub git_dir: String,
    pub tree: String,
    pub created_at: String,
    pub undone_at: Option<String>,
}

impl Store {
    /// Record a snapshot taken before `call_id` ran. Returns its sequence.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the insert fails.
    pub fn insert_step_snapshot(
        &self,
        task_id: &str,
        call_id: &str,
        tool: &str,
        work_tree: &str,
        git_dir: &str,
        tree: &str,
    ) -> Result<i64, StoreError> {
        let seq: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM agent_step_snapshots WHERE task_id = ?1",
            rusqlite::params![task_id],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO agent_step_snapshots (task_id, seq, call_id, tool, work_tree, git_dir, tree)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![task_id, seq, call_id, tool, work_tree, git_dir, tree],
        )?;
        Ok(seq)
    }

    /// A task's snapshots, oldest first.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn step_snapshots(&self, task_id: &str) -> Result<Vec<StepSnapshotRow>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT task_id, seq, call_id, tool, work_tree, git_dir, tree, created_at, undone_at
             FROM agent_step_snapshots WHERE task_id = ?1 ORDER BY seq ASC",
        )?;
        let rows = statement.query_map(rusqlite::params![task_id], |row| {
            Ok(StepSnapshotRow {
                task_id: row.get(0)?,
                seq: row.get(1)?,
                call_id: row.get(2)?,
                tool: row.get(3)?,
                work_tree: row.get(4)?,
                git_dir: row.get(5)?,
                tree: row.get(6)?,
                created_at: row.get(7)?,
                undone_at: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Mark a snapshot as undone (restored). Returns false if it was already.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the update fails.
    pub fn mark_step_undone(&self, task_id: &str, seq: i64) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "UPDATE agent_step_snapshots SET undone_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE task_id = ?1 AND seq = ?2 AND undone_at IS NULL",
            rusqlite::params![task_id, seq],
        )?;
        Ok(changed > 0)
    }
}

#[cfg(test)]
mod agent_engine_tests {
    use super::*;

    #[test]
    fn step_snapshots_sequence_per_task_and_undo_once() {
        let store = Store::open_in_memory().expect("open");
        let a = store
            .insert_step_snapshot("t1", "c1", "write_file", "/w", "/d", "tree1")
            .expect("insert");
        let b = store
            .insert_step_snapshot("t1", "c2", "edit", "/w", "/d", "tree2")
            .expect("insert");
        let other = store
            .insert_step_snapshot("t2", "c1", "run_command", "/w", "/d", "tree3")
            .expect("insert");
        assert_eq!((a, b, other), (1, 2, 1));
        let rows = store.step_snapshots("t1").expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].tree, "tree2");
        assert!(store.mark_step_undone("t1", 2).expect("undo"));
        assert!(!store.mark_step_undone("t1", 2).expect("again"));
        assert!(store.step_snapshots("t1").expect("rows")[1]
            .undone_at
            .is_some());
    }
}

// --- end agent-engine ---

// --- llm-gateway ---------------------------------------------------------------
// Storage for the built-in LLM gateway (`crate::router`): provider
// connections, combos and usage. Kept in one block so parallel branches merge
// cleanly.

/// Row in `llm_connections`. `data` is non-secret JSON (base URL, models…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionRow {
    pub id: String,
    pub provider: String,
    pub auth_type: String,
    pub name: String,
    pub priority: i64,
    pub is_active: bool,
    pub data: String,
}

/// Row in `llm_combos`. `models` is a JSON array of `provider/model` strings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComboRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub models: String,
    pub strategy: Option<String>,
    pub sticky_limit: Option<i64>,
    /// Smart strategy: switch below this confidence (`None` = 0.8).
    pub confidence_threshold: Option<f64>,
    /// Weighted strategy: JSON array of weights, one per model.
    pub weights: Option<String>,
}

/// Rows kept in `llm_cache_hits` (older ones are pruned on insert).
pub const CACHE_HITS_KEPT: i64 = 50_000;

/// Row in `llm_cache_hits`: an answer served from the response cache.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CacheHitRow {
    pub provider: String,
    pub model: String,
    pub role: String,
    /// Prompt + completion tokens the provider did not have to process.
    pub saved_tokens: i64,
    pub saved_cost: f64,
}

/// Row in `llm_smart_sticky`: a session's sticky model for one combo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StickyRow {
    pub session: String,
    pub combo: String,
    pub model: String,
    /// JSON array of booleans, oldest first (`true` = bad answer).
    pub window: String,
}

/// Row in `llm_switches`: a smart combo changed a session's model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SwitchRow {
    /// RFC 3339 timestamp; `None` = now. Filled on reads.
    pub ts: Option<String>,
    pub session: String,
    pub combo: String,
    pub from_model: String,
    pub to_model: String,
    /// `failover` or `quality:<signal>`.
    pub reason: String,
    /// Confidence when the switch happened (1 for failover).
    pub confidence: f64,
}

/// One gateway request, as recorded in `llm_usage`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageRow {
    /// RFC 3339 timestamp; `None` = now. Filled on reads.
    pub ts: Option<String>,
    pub provider: String,
    pub model: String,
    pub connection_id: Option<String>,
    pub role: String,
    pub combo: Option<String>,
    pub endpoint: String,
    /// HTTP status (0 = transport error).
    pub status: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub cache_creation_tokens: i64,
    pub reasoning_tokens: i64,
    /// Estimated USD.
    pub cost: f64,
    /// Whether the model had a price entry (otherwise `cost` is 0).
    pub cost_known: bool,
    pub latency_ms: i64,
    pub saver_bytes_before: i64,
    pub saver_bytes_after: i64,
}

/// Aggregated numbers for one group (a provider, model, role or day).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageTotals {
    pub key: String,
    pub requests: i64,
    pub failures: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub cost: f64,
}

/// Result of [`Store::usage_summary`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageSummary {
    pub totals: UsageTotals,
    pub by_provider: Vec<UsageTotals>,
    pub by_model: Vec<UsageTotals>,
    pub by_role: Vec<UsageTotals>,
    /// One entry per day that has data, oldest first (`key` = `YYYY-MM-DD`).
    pub daily: Vec<UsageTotals>,
    pub recent: Vec<UsageRow>,
    /// Smart-combo model switches in the period, newest first.
    pub switches: Vec<SwitchRow>,
    /// Answers served from the response cache in the period.
    pub cache_hits: i64,
    pub cache_saved_tokens: i64,
    pub cache_saved_cost: f64,
    /// Bytes of tool output before / after RTK compression in the period.
    pub saver_bytes_before: i64,
    pub saver_bytes_after: i64,
}

impl Store {
    /// Insert or replace a provider connection.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn upsert_connection(&self, row: &ConnectionRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO llm_connections (id, provider, auth_type, name, priority, is_active, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET provider = excluded.provider,
                auth_type = excluded.auth_type, name = excluded.name,
                priority = excluded.priority, is_active = excluded.is_active,
                data = excluded.data",
            rusqlite::params![
                row.id,
                row.provider,
                row.auth_type,
                row.name,
                row.priority,
                row.is_active,
                row.data
            ],
        )?;
        Ok(())
    }

    /// All connections, by priority then name.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn list_connections(&self) -> Result<Vec<ConnectionRow>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT id, provider, auth_type, name, priority, is_active, data
             FROM llm_connections ORDER BY priority ASC, name ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ConnectionRow {
                id: row.get(0)?,
                provider: row.get(1)?,
                auth_type: row.get(2)?,
                name: row.get(3)?,
                priority: row.get(4)?,
                is_active: row.get(5)?,
                data: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Delete a connection; `false` if it did not exist.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn delete_connection(&self, id: &str) -> Result<bool, StoreError> {
        Ok(self
            .conn
            .execute("DELETE FROM llm_connections WHERE id = ?1", [id])?
            > 0)
    }

    /// Insert or replace a combo (names are unique).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails or the name is taken.
    pub fn upsert_combo(&self, row: &ComboRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO llm_combos (id, name, kind, models, strategy, sticky_limit,
                confidence_threshold, weights)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, kind = excluded.kind,
                models = excluded.models, strategy = excluded.strategy,
                sticky_limit = excluded.sticky_limit,
                confidence_threshold = excluded.confidence_threshold,
                weights = excluded.weights",
            rusqlite::params![
                row.id,
                row.name,
                row.kind,
                row.models,
                row.strategy,
                row.sticky_limit,
                row.confidence_threshold,
                row.weights
            ],
        )?;
        Ok(())
    }

    /// All combos by name.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn list_combos(&self) -> Result<Vec<ComboRow>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, kind, models, strategy, sticky_limit, confidence_threshold, weights
             FROM llm_combos ORDER BY name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ComboRow {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                models: row.get(3)?,
                strategy: row.get(4)?,
                sticky_limit: row.get(5)?,
                confidence_threshold: row.get(6)?,
                weights: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Delete a combo; `false` if it did not exist.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn delete_combo(&self, id: &str) -> Result<bool, StoreError> {
        Ok(self
            .conn
            .execute("DELETE FROM llm_combos WHERE id = ?1", [id])?
            > 0)
    }

    /// A session's sticky model for a combo.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn get_sticky(&self, session: &str, combo: &str) -> Result<Option<StickyRow>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT session, combo, model, quality FROM llm_smart_sticky
             WHERE session = ?1 AND combo = ?2",
        )?;
        let mut rows = statement.query_map([session, combo], |row| {
            Ok(StickyRow {
                session: row.get(0)?,
                combo: row.get(1)?,
                model: row.get(2)?,
                window: row.get(3)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    /// Insert or replace a session's sticky model.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn set_sticky(&self, row: &StickyRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO llm_smart_sticky (session, combo, model, quality) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session, combo) DO UPDATE SET model = excluded.model,
                quality = excluded.quality,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
            rusqlite::params![row.session, row.combo, row.model, row.window],
        )?;
        Ok(())
    }

    /// Record a smart-combo model switch.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn record_switch(&self, row: &SwitchRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO llm_switches (ts, session, combo, from_model, to_model, reason, confidence)
             VALUES (COALESCE(?1, strftime('%Y-%m-%dT%H:%M:%fZ','now')), ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                row.ts,
                row.session,
                row.combo,
                row.from_model,
                row.to_model,
                row.reason,
                row.confidence
            ],
        )?;
        Ok(())
    }

    /// A cached answer that has not expired at `now` (Unix seconds).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn cache_get(&self, key: &str, now: i64) -> Result<Option<String>, StoreError> {
        let mut statement = self.conn.prepare_cached(
            "SELECT body FROM llm_response_cache WHERE key = ?1 AND expires_at > ?2",
        )?;
        let mut rows = statement.query_map(rusqlite::params![key, now], |row| row.get(0))?;
        Ok(rows.next().transpose()?)
    }

    /// Store an answer, then drop expired entries and the oldest beyond
    /// `max_entries`.
    ///
    /// # Errors
    /// Returns [`StoreError`] if a write fails.
    #[allow(clippy::too_many_arguments)] // One row's columns.
    pub fn cache_put(
        &self,
        key: &str,
        provider: &str,
        model: &str,
        body: &str,
        now: i64,
        ttl_secs: i64,
        max_entries: i64,
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO llm_response_cache (key, provider, model, body, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![key, provider, model, body, now, now.saturating_add(ttl_secs)],
        )?;
        tx.execute(
            "DELETE FROM llm_response_cache WHERE expires_at <= ?1",
            [now],
        )?;
        tx.execute(
            "DELETE FROM llm_response_cache WHERE key NOT IN
                (SELECT key FROM llm_response_cache ORDER BY created_at DESC, rowid DESC LIMIT ?1)",
            [max_entries.max(0)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Remove every cached answer; returns how many there were.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn cache_clear(&self) -> Result<usize, StoreError> {
        Ok(self.conn.execute("DELETE FROM llm_response_cache", [])?)
    }

    /// Number of cached answers.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn cache_count(&self) -> Result<i64, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM llm_response_cache", [], |row| {
                row.get(0)
            })?)
    }

    /// Record an answer served from the cache.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn record_cache_hit(&self, row: &CacheHitRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO llm_cache_hits (provider, model, role, saved_tokens, saved_cost)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                row.provider,
                row.model,
                row.role,
                row.saved_tokens,
                row.saved_cost
            ],
        )?;
        // Keep the table bounded: only the newest rows feed the Usage page.
        self.conn
            .prepare_cached(
                "DELETE FROM llm_cache_hits WHERE id <= (SELECT MAX(id) FROM llm_cache_hits) - ?1",
            )?
            .execute([CACHE_HITS_KEPT])?;
        Ok(())
    }

    /// Recorded spend (USD) today or this calendar month (UTC), for one role
    /// or (`None`) overall.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn spend(&self, role: Option<&str>, monthly: bool) -> Result<f64, StoreError> {
        let sql = if monthly {
            "SELECT COALESCE(SUM(cost), 0) FROM llm_usage_daily
             WHERE day >= date('now', 'start of month') AND (?1 IS NULL OR role = ?1)"
        } else {
            "SELECT COALESCE(SUM(cost), 0) FROM llm_usage_daily
             WHERE day >= date('now') AND (?1 IS NULL OR role = ?1)"
        };
        Ok(self
            .conn
            .prepare_cached(sql)?
            .query_row([role], |row| row.get(0))?)
    }

    fn switches_since(&self, since: &str, limit: i64) -> Result<Vec<SwitchRow>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT ts, session, combo, from_model, to_model, reason, confidence
             FROM llm_switches WHERE substr(ts, 1, 10) >= ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(rusqlite::params![since, limit], |row| {
            Ok(SwitchRow {
                ts: row.get(0)?,
                session: row.get(1)?,
                combo: row.get(2)?,
                from_model: row.get(3)?,
                to_model: row.get(4)?,
                reason: row.get(5)?,
                confidence: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Record one request and fold it into the daily rollup, atomically.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn record_usage(&self, row: &UsageRow) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO llm_usage (ts, provider, model, connection_id, role, combo, endpoint,
                status, prompt_tokens, completion_tokens, cached_tokens, cache_creation_tokens,
                reasoning_tokens, cost, cost_known, latency_ms, saver_bytes_before,
                saver_bytes_after)
             VALUES (COALESCE(?1, strftime('%Y-%m-%dT%H:%M:%fZ','now')), ?2, ?3, ?4, ?5, ?6, ?7,
                ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            rusqlite::params![
                row.ts,
                row.provider,
                row.model,
                row.connection_id,
                row.role,
                row.combo,
                row.endpoint,
                row.status,
                row.prompt_tokens,
                row.completion_tokens,
                row.cached_tokens,
                row.cache_creation_tokens,
                row.reasoning_tokens,
                row.cost,
                row.cost_known,
                row.latency_ms,
                row.saver_bytes_before,
                row.saver_bytes_after
            ],
        )?;
        let failed = i64::from(!(200..300).contains(&row.status));
        tx.execute(
            "INSERT INTO llm_usage_daily (day, provider, model, role, requests, failures,
                prompt_tokens, completion_tokens, cached_tokens, reasoning_tokens, cost)
             VALUES (substr(COALESCE(?1, strftime('%Y-%m-%dT%H:%M:%fZ','now')), 1, 10),
                ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(day, provider, model, role) DO UPDATE SET
                requests = requests + 1,
                failures = failures + excluded.failures,
                prompt_tokens = prompt_tokens + excluded.prompt_tokens,
                completion_tokens = completion_tokens + excluded.completion_tokens,
                cached_tokens = cached_tokens + excluded.cached_tokens,
                reasoning_tokens = reasoning_tokens + excluded.reasoning_tokens,
                cost = cost + excluded.cost",
            rusqlite::params![
                row.ts,
                row.provider,
                row.model,
                row.role,
                failed,
                row.prompt_tokens,
                row.completion_tokens,
                row.cached_tokens,
                row.reasoning_tokens,
                row.cost
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Usage for the last `days` days including today (`0` = all time), with
    /// the `recent` most recent requests.
    ///
    /// # Errors
    /// Returns [`StoreError`] if a query fails.
    #[allow(clippy::too_many_lines)] // One query per section of the summary.
    pub fn usage_summary(&self, days: u32, recent: usize) -> Result<UsageSummary, StoreError> {
        // `date('now', '-0 days')` is today, so N days = offset N-1.
        let since = if days == 0 {
            "0000-00-00".to_string()
        } else {
            let offset = format!("-{} days", days - 1);
            self.conn
                .query_row("SELECT date('now', ?1)", [offset], |row| row.get(0))?
        };
        let group = |column: &str| -> Result<Vec<UsageTotals>, StoreError> {
            let sql = format!(
                "SELECT {column}, SUM(requests), SUM(failures), SUM(prompt_tokens),
                    SUM(completion_tokens), SUM(cached_tokens), SUM(reasoning_tokens), SUM(cost)
                 FROM llm_usage_daily WHERE day >= ?1 GROUP BY {column}
                 ORDER BY {}",
                if column == "day" {
                    "day ASC"
                } else {
                    "SUM(cost) DESC, SUM(requests) DESC"
                }
            );
            let mut statement = self.conn.prepare(&sql)?;
            let rows = statement.query_map([&since], |row| {
                Ok(UsageTotals {
                    key: row.get(0)?,
                    requests: row.get(1)?,
                    failures: row.get(2)?,
                    prompt_tokens: row.get(3)?,
                    completion_tokens: row.get(4)?,
                    cached_tokens: row.get(5)?,
                    reasoning_tokens: row.get(6)?,
                    cost: row.get(7)?,
                })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        };
        let by_provider = group("provider")?;
        let by_model = group("model")?;
        let by_role = group("role")?;
        let daily = group("day")?;
        let mut totals = UsageTotals {
            key: "total".into(),
            ..UsageTotals::default()
        };
        for day in &daily {
            totals.requests += day.requests;
            totals.failures += day.failures;
            totals.prompt_tokens += day.prompt_tokens;
            totals.completion_tokens += day.completion_tokens;
            totals.cached_tokens += day.cached_tokens;
            totals.reasoning_tokens += day.reasoning_tokens;
            totals.cost += day.cost;
        }
        let (saver_bytes_before, saver_bytes_after) = self.conn.query_row(
            "SELECT COALESCE(SUM(saver_bytes_before), 0), COALESCE(SUM(saver_bytes_after), 0)
             FROM llm_usage WHERE substr(ts, 1, 10) >= ?1",
            [&since],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut statement = self.conn.prepare(
            "SELECT ts, provider, model, connection_id, role, combo, endpoint, status,
                prompt_tokens, completion_tokens, cached_tokens, cache_creation_tokens,
                reasoning_tokens, cost, cost_known, latency_ms, saver_bytes_before,
                saver_bytes_after
             FROM llm_usage WHERE substr(ts, 1, 10) >= ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let limit = i64::try_from(recent).unwrap_or(i64::MAX);
        let rows = statement.query_map(rusqlite::params![since, limit], |row| {
            Ok(UsageRow {
                ts: row.get(0)?,
                provider: row.get(1)?,
                model: row.get(2)?,
                connection_id: row.get(3)?,
                role: row.get(4)?,
                combo: row.get(5)?,
                endpoint: row.get(6)?,
                status: row.get(7)?,
                prompt_tokens: row.get(8)?,
                completion_tokens: row.get(9)?,
                cached_tokens: row.get(10)?,
                cache_creation_tokens: row.get(11)?,
                reasoning_tokens: row.get(12)?,
                cost: row.get(13)?,
                cost_known: row.get(14)?,
                latency_ms: row.get(15)?,
                saver_bytes_before: row.get(16)?,
                saver_bytes_after: row.get(17)?,
            })
        })?;
        let recent_rows = rows.collect::<Result<_, _>>()?;
        let switches = self.switches_since(&since, limit)?;
        let (cache_hits, cache_saved_tokens, cache_saved_cost) = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(saved_tokens), 0), COALESCE(SUM(saved_cost), 0)
             FROM llm_cache_hits WHERE substr(ts, 1, 10) >= ?1",
            [&since],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(UsageSummary {
            totals,
            by_provider,
            by_model,
            by_role,
            daily,
            recent: recent_rows,
            switches,
            cache_hits,
            cache_saved_tokens,
            cache_saved_cost,
            saver_bytes_before,
            saver_bytes_after,
        })
    }
}

#[cfg(test)]
mod gateway_tests {
    use super::*;

    #[test]
    fn connections_and_combos_round_trip() {
        let store = Store::open_in_memory().expect("open");
        let row = ConnectionRow {
            id: "c1".into(),
            provider: "openai".into(),
            auth_type: "api_key".into(),
            name: "Work".into(),
            priority: 2,
            is_active: true,
            data: r#"{"models":["gpt-4o"]}"#.into(),
        };
        store.upsert_connection(&row).expect("insert");
        let mut changed = row.clone();
        changed.priority = 1;
        store.upsert_connection(&changed).expect("update");
        assert_eq!(store.list_connections().expect("list"), vec![changed]);
        assert!(store.delete_connection("c1").expect("delete"));
        assert!(!store.delete_connection("c1").expect("delete again"));

        let combo = ComboRow {
            id: "k1".into(),
            name: "fast".into(),
            kind: "llm".into(),
            models: r#"["openai/gpt-4o-mini"]"#.into(),
            strategy: Some("smart".into()),
            sticky_limit: None,
            confidence_threshold: Some(0.9),
            weights: None,
        };
        store.upsert_combo(&combo).expect("combo");
        let clash = ComboRow {
            id: "k2".into(),
            ..combo.clone()
        };
        assert!(store.upsert_combo(&clash).is_err(), "names are unique");
        assert_eq!(store.list_combos().expect("list"), vec![combo]);
        assert!(store.delete_combo("k1").expect("delete"));
    }

    #[test]
    fn response_cache_expires_evicts_and_counts_hits() {
        let store = Store::open_in_memory().expect("open");
        store
            .cache_put("k1", "openai", "m", "one", 100, 60, 2)
            .expect("put");
        assert_eq!(
            store.cache_get("k1", 120).expect("get").as_deref(),
            Some("one")
        );
        assert_eq!(store.cache_get("k1", 160).expect("expired"), None);
        store
            .cache_put("k2", "openai", "m", "two", 101, 60, 2)
            .expect("put");
        store
            .cache_put("k3", "openai", "m", "three", 102, 60, 2)
            .expect("put");
        assert_eq!(store.cache_count().expect("count"), 2, "oldest evicted");
        assert_eq!(store.cache_get("k1", 103).expect("get"), None);
        store
            .record_cache_hit(&CacheHitRow {
                provider: "openai".into(),
                model: "m".into(),
                role: "chat".into(),
                saved_tokens: 150,
                saved_cost: 0.25,
            })
            .expect("hit");
        let summary = store.usage_summary(1, 5).expect("summary");
        assert_eq!((summary.cache_hits, summary.cache_saved_tokens), (1, 150));
        assert!((summary.cache_saved_cost - 0.25).abs() < 1e-9);
        assert_eq!(store.cache_clear().expect("clear"), 2);
    }

    #[test]
    fn spend_is_summed_per_role_and_period() {
        let store = Store::open_in_memory().expect("open");
        let row = |role: &str, cost: f64, ts: Option<&str>| UsageRow {
            provider: "p".into(),
            model: "m".into(),
            role: role.into(),
            status: 200,
            cost,
            ts: ts.map(str::to_string),
            ..UsageRow::default()
        };
        store.record_usage(&row("chat", 1.5, None)).expect("chat");
        store
            .record_usage(&row("review", 0.5, None))
            .expect("review");
        store
            .record_usage(&row("chat", 9.0, Some("2001-01-01T00:00:00Z")))
            .expect("old");
        assert!((store.spend(Some("chat"), false).expect("day") - 1.5).abs() < 1e-9);
        assert!((store.spend(None, false).expect("all") - 2.0).abs() < 1e-9);
        assert!((store.spend(None, true).expect("month") - 2.0).abs() < 1e-9);
    }

    #[test]
    fn sticky_choices_and_switches_persist() {
        let store = Store::open_in_memory().expect("open");
        assert_eq!(store.get_sticky("t1", "smart").expect("get"), None);
        let row = StickyRow {
            session: "t1".into(),
            combo: "smart".into(),
            model: "a/x".into(),
            window: "[false,true]".into(),
        };
        store.set_sticky(&row).expect("set");
        let moved = StickyRow {
            model: "b/y".into(),
            window: "[]".into(),
            ..row.clone()
        };
        store.set_sticky(&moved).expect("replace");
        assert_eq!(store.get_sticky("t1", "smart").expect("get"), Some(moved));
        assert_eq!(
            store.get_sticky("t2", "smart").expect("other session"),
            None
        );
        let switch = SwitchRow {
            session: "t1".into(),
            combo: "smart".into(),
            from_model: "a/x".into(),
            to_model: "b/y".into(),
            reason: "quality:empty".into(),
            confidence: 0.7,
            ..SwitchRow::default()
        };
        store.record_switch(&switch).expect("switch");
        store
            .record_switch(&SwitchRow {
                ts: Some("2001-01-01T00:00:00Z".into()),
                ..switch.clone()
            })
            .expect("old switch");
        let week = store.usage_summary(7, 20).expect("summary");
        assert_eq!(
            week.switches.len(),
            1,
            "old switches fall outside the period"
        );
        assert_eq!(week.switches[0].reason, "quality:empty");
        assert!(week.switches[0].ts.is_some());
        assert_eq!(store.usage_summary(0, 20).expect("all").switches.len(), 2);
    }

    #[test]
    fn usage_rolls_up_by_day_provider_model_and_role() {
        let store = Store::open_in_memory().expect("open");
        let base = UsageRow {
            provider: "openai".into(),
            model: "gpt-4o".into(),
            role: "chat".into(),
            endpoint: "chat".into(),
            status: 200,
            prompt_tokens: 100,
            completion_tokens: 10,
            cost: 0.5,
            cost_known: true,
            saver_bytes_before: 1000,
            saver_bytes_after: 400,
            ..UsageRow::default()
        };
        store.record_usage(&base).expect("one");
        store.record_usage(&base).expect("two");
        store
            .record_usage(&UsageRow {
                provider: "anthropic".into(),
                model: "claude".into(),
                role: "review".into(),
                status: 429,
                cost: 0.0,
                ..base.clone()
            })
            .expect("three");
        store
            .record_usage(&UsageRow {
                ts: Some("2001-01-01T00:00:00Z".into()),
                ..base.clone()
            })
            .expect("old");
        let week = store.usage_summary(7, 20).expect("summary");
        assert_eq!(week.totals.requests, 3);
        assert_eq!(week.totals.failures, 1);
        assert_eq!(week.totals.prompt_tokens, 300);
        assert!((week.totals.cost - 1.0).abs() < 1e-9);
        assert_eq!(week.by_provider[0].key, "openai");
        assert_eq!(week.by_provider[0].requests, 2);
        assert_eq!(week.by_role.len(), 2);
        assert_eq!(week.daily.len(), 1);
        assert_eq!(week.recent.len(), 3);
        assert_eq!(week.recent[0].provider, "anthropic", "newest first");
        assert_eq!(week.saver_bytes_before, 3000);
        let all = store.usage_summary(0, 2).expect("all");
        assert_eq!(all.totals.requests, 4);
        assert_eq!(all.daily[0].key, "2001-01-01");
        assert_eq!(all.recent.len(), 2);
    }
}
// --- end llm-gateway -----------------------------------------------------------

// --- channels -------------------------------------------------------------------
// Storage for the channel gateway (`crates/channels`). No secrets: bot tokens
// are in the OS keychain.

/// Row in `channels`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRow {
    pub id: String,
    pub kind: String,
    pub name: String,
    /// Non-secret JSON configuration.
    pub config: String,
}

/// Row in `channel_pairings`. Times are Unix seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelPairingRow {
    pub channel_id: String,
    pub sender_id: String,
    pub chat_id: String,
    pub label: String,
    pub code: String,
    pub created_at: u64,
    pub last_seen_at: u64,
}

/// Row in `channel_allow`. `added_at` is filled on reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelAllowRow {
    pub channel_id: String,
    pub sender_id: String,
    pub label: String,
    pub source: String,
    pub added_at: String,
}

/// Row in `channel_activity`. `ts` is filled on reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelActivityRow {
    pub ts: String,
    pub channel_id: String,
    pub direction: String,
    pub kind: String,
    pub peer: String,
    pub text: String,
    pub task_id: Option<String>,
}

/// Activity rows kept per channel (older ones are pruned on insert).
pub const CHANNEL_ACTIVITY_KEPT: i64 = 2_000;
/// Idempotency keys older than this many seconds are pruned.
pub const CHANNEL_IDEMPOTENCY_TTL_SECS: i64 = 30 * 86_400;

#[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
impl Store {
    /// Insert or replace a channel account.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn channel_upsert(&self, row: &ChannelRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO channels (id, kind, name, config) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, name = excluded.name,
                config = excluded.config",
            rusqlite::params![row.id, row.kind, row.name, row.config],
        )?;
        Ok(())
    }

    /// Every channel account, oldest first.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the read fails.
    pub fn channel_rows(&self) -> Result<Vec<ChannelRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, kind, name, config FROM channels ORDER BY created_at, id")?;
        let rows = stmt.query_map([], |r| {
            Ok(ChannelRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                config: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Delete a channel and its pairings, allow-list and activity.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn channel_delete(&self, id: &str) -> Result<bool, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let removed = tx.execute("DELETE FROM channels WHERE id = ?1", [id])?;
        for table in ["channel_pairings", "channel_allow", "channel_activity"] {
            tx.execute(&format!("DELETE FROM {table} WHERE channel_id = ?1"), [id])?;
        }
        tx.commit()?;
        Ok(removed > 0)
    }

    /// Pending pairing requests of a channel, oldest first.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the read fails.
    pub fn channel_pairings(&self, channel_id: &str) -> Result<Vec<ChannelPairingRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT channel_id, sender_id, chat_id, label, code, created_at, last_seen_at
             FROM channel_pairings WHERE channel_id = ?1 ORDER BY created_at, sender_id",
        )?;
        let rows = stmt.query_map([channel_id], |r| {
            Ok(ChannelPairingRow {
                channel_id: r.get(0)?,
                sender_id: r.get(1)?,
                chat_id: r.get(2)?,
                label: r.get(3)?,
                code: r.get(4)?,
                created_at: r.get::<_, i64>(5)?.max(0) as u64,
                last_seen_at: r.get::<_, i64>(6)?.max(0) as u64,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Replace a channel's pending pairing requests in one transaction.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn channel_pairings_replace(
        &self,
        channel_id: &str,
        rows: &[ChannelPairingRow],
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM channel_pairings WHERE channel_id = ?1", [channel_id])?;
        for row in rows {
            tx.execute(
                "INSERT INTO channel_pairings
                 (channel_id, sender_id, chat_id, label, code, created_at, last_seen_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    channel_id,
                    row.sender_id,
                    row.chat_id,
                    row.label,
                    row.code,
                    row.created_at as i64,
                    row.last_seen_at as i64
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// A channel's allow-list, oldest first.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the read fails.
    pub fn channel_allow_list(&self, channel_id: &str) -> Result<Vec<ChannelAllowRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT channel_id, sender_id, label, source, added_at FROM channel_allow
             WHERE channel_id = ?1 ORDER BY added_at, sender_id",
        )?;
        let rows = stmt.query_map([channel_id], |r| {
            Ok(ChannelAllowRow {
                channel_id: r.get(0)?,
                sender_id: r.get(1)?,
                label: r.get(2)?,
                source: r.get(3)?,
                added_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Add (or relabel) a sender on a channel's allow-list.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn channel_allow_add(&self, row: &ChannelAllowRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO channel_allow (channel_id, sender_id, label, source)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(channel_id, sender_id) DO UPDATE SET label = excluded.label,
                source = excluded.source",
            rusqlite::params![row.channel_id, row.sender_id, row.label, row.source],
        )?;
        Ok(())
    }

    /// Remove a sender from a channel's allow-list.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn channel_allow_remove(&self, channel_id: &str, sender_id: &str) -> Result<bool, StoreError> {
        Ok(self.conn.execute(
            "DELETE FROM channel_allow WHERE channel_id = ?1 AND sender_id = ?2",
            [channel_id, sender_id],
        )? > 0)
    }

    /// Record an idempotency key; `false` if it already existed. Prunes
    /// keys older than [`CHANNEL_IDEMPOTENCY_TTL_SECS`].
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn channel_claim(&self, key: &str, value: &str) -> Result<bool, StoreError> {
        self.conn.execute(
            "DELETE FROM channel_idempotency
             WHERE created_at < CAST(strftime('%s','now') AS INTEGER) - ?1",
            [CHANNEL_IDEMPOTENCY_TTL_SECS],
        )?;
        Ok(self.conn.execute(
            "INSERT OR IGNORE INTO channel_idempotency (key, value) VALUES (?1, ?2)",
            [key, value],
        )? == 1)
    }

    /// The value recorded with an idempotency key.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the read fails.
    pub fn channel_claimed(&self, key: &str) -> Result<Option<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM channel_idempotency WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    /// Append to a channel's activity log, keeping the newest
    /// [`CHANNEL_ACTIVITY_KEPT`] rows.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn channel_activity_add(&self, row: &ChannelActivityRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO channel_activity (channel_id, direction, kind, peer, text, task_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                row.channel_id,
                row.direction,
                row.kind,
                row.peer,
                row.text,
                row.task_id
            ],
        )?;
        self.conn.execute(
            "DELETE FROM channel_activity WHERE channel_id = ?1 AND id NOT IN
             (SELECT id FROM channel_activity WHERE channel_id = ?1 ORDER BY id DESC LIMIT ?2)",
            rusqlite::params![row.channel_id, CHANNEL_ACTIVITY_KEPT],
        )?;
        Ok(())
    }

    /// The newest `limit` activity rows of a channel, newest first.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the read fails.
    pub fn channel_activity(
        &self,
        channel_id: &str,
        limit: usize,
    ) -> Result<Vec<ChannelActivityRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT ts, channel_id, direction, kind, peer, text, task_id FROM channel_activity
             WHERE channel_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt.query_map(rusqlite::params![channel_id, limit], |r| {
            Ok(ChannelActivityRow {
                ts: r.get(0)?,
                channel_id: r.get(1)?,
                direction: r.get(2)?,
                kind: r.get(3)?,
                peer: r.get(4)?,
                text: r.get(5)?,
                task_id: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod channel_tests {
    use super::*;

    fn pairing(sender: &str, code: &str) -> ChannelPairingRow {
        ChannelPairingRow {
            channel_id: "ch_1".into(),
            sender_id: sender.into(),
            chat_id: sender.into(),
            label: format!("@{sender}"),
            code: code.into(),
            created_at: 10,
            last_seen_at: 11,
        }
    }

    #[test]
    fn channels_round_trip_and_delete_cascades() {
        let store = Store::open_in_memory().expect("store");
        let row = ChannelRow {
            id: "ch_1".into(),
            kind: "telegram".into(),
            name: "Bot".into(),
            config: "{}".into(),
        };
        store.channel_upsert(&row).expect("upsert");
        store.channel_upsert(&ChannelRow { name: "Bot 2".into(), ..row.clone() }).expect("update");
        assert_eq!(store.channel_rows().expect("rows")[0].name, "Bot 2");
        store.channel_pairings_replace("ch_1", &[pairing("a", "AAAA")]).expect("pair");
        store
            .channel_allow_add(&ChannelAllowRow {
                channel_id: "ch_1".into(),
                sender_id: "a".into(),
                label: "@a".into(),
                source: "pairing".into(),
                added_at: String::new(),
            })
            .expect("allow");
        assert!(store.channel_delete("ch_1").expect("delete"));
        assert!(store.channel_rows().expect("rows").is_empty());
        assert!(store.channel_pairings("ch_1").expect("p").is_empty());
        assert!(store.channel_allow_list("ch_1").expect("a").is_empty());
    }

    #[test]
    fn pairings_replace_atomically() {
        let store = Store::open_in_memory().expect("store");
        store
            .channel_pairings_replace("ch_1", &[pairing("a", "AAAA"), pairing("b", "BBBB")])
            .expect("two");
        store.channel_pairings_replace("ch_1", &[pairing("b", "BBBB")]).expect("one");
        let rows = store.channel_pairings("ch_1").expect("rows");
        assert_eq!(rows, [pairing("b", "BBBB")]);
    }

    #[test]
    fn allow_list_add_relabel_remove() {
        let store = Store::open_in_memory().expect("store");
        let mut row = ChannelAllowRow {
            channel_id: "ch_1".into(),
            sender_id: "42".into(),
            label: "@a".into(),
            source: "manual".into(),
            added_at: String::new(),
        };
        store.channel_allow_add(&row).expect("add");
        row.label = "@b".into();
        store.channel_allow_add(&row).expect("relabel");
        let list = store.channel_allow_list("ch_1").expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label, "@b");
        assert!(!list[0].added_at.is_empty());
        assert!(store.channel_allow_remove("ch_1", "42").expect("rm"));
        assert!(!store.channel_allow_remove("ch_1", "42").expect("rm again"));
    }

    #[test]
    fn claims_are_idempotent() {
        let store = Store::open_in_memory().expect("store");
        assert!(store.channel_claim("in:ch_1:1", "").expect("first"));
        assert!(!store.channel_claim("in:ch_1:1", "").expect("second"));
        assert!(store.channel_claim("out:ch_1:1:0", "m9").expect("out"));
        assert_eq!(store.channel_claimed("out:ch_1:1:0").expect("get"), Some("m9".into()));
        assert_eq!(store.channel_claimed("missing").expect("none"), None);
    }

    #[test]
    fn activity_is_newest_first_and_bounded() {
        let store = Store::open_in_memory().expect("store");
        for n in 0..5 {
            store
                .channel_activity_add(&ChannelActivityRow {
                    ts: String::new(),
                    channel_id: "ch_1".into(),
                    direction: "in".into(),
                    kind: "message".into(),
                    peer: "42".into(),
                    text: format!("m{n}"),
                    task_id: None,
                })
                .expect("add");
        }
        let rows = store.channel_activity("ch_1", 2).expect("rows");
        assert_eq!(rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["m4", "m3"]);
        assert!(store.channel_activity("other", 10).expect("none").is_empty());
    }
}
// --- end channels ---------------------------------------------------------------

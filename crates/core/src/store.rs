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
    // --- memory-skills ---
    // Session store (Hermes `hermes_state`): every task run is a session; its
    // messages are searchable through an external-content FTS5 index over a
    // view that cuts tool output to 8,192 characters.
    "CREATE TABLE IF NOT EXISTS agent_system_prompts (
        hash TEXT PRIMARY KEY,
        prompt TEXT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS agent_sessions (
        id TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL,
        task_id TEXT,
        parent_session_id TEXT,
        source TEXT NOT NULL DEFAULT 'task',
        title TEXT,
        model TEXT,
        system_prompt_hash TEXT,
        started_at INTEGER NOT NULL,
        ended_at INTEGER,
        end_reason TEXT,
        message_count INTEGER NOT NULL DEFAULT 0
    )",
    "CREATE INDEX IF NOT EXISTS agent_sessions_by_workspace
        ON agent_sessions(workspace_id, started_at DESC)",
    "CREATE TABLE IF NOT EXISTS agent_messages (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
        role TEXT NOT NULL,
        content TEXT,
        tool_calls TEXT,
        tool_call_id TEXT,
        tool_name TEXT,
        ts INTEGER NOT NULL,
        token_count INTEGER,
        active INTEGER NOT NULL DEFAULT 1,
        compacted INTEGER NOT NULL DEFAULT 0
    )",
    "CREATE INDEX IF NOT EXISTS agent_messages_by_session ON agent_messages(session_id, id)",
    "CREATE VIEW IF NOT EXISTS agent_messages_fts_src AS
        SELECT id,
               CASE WHEN role = 'tool' THEN substr(COALESCE(content, ''), 1, 8192)
                    ELSE content END AS content,
               tool_name, tool_calls
        FROM agent_messages",
    "CREATE VIRTUAL TABLE IF NOT EXISTS agent_messages_fts USING fts5(
        content, tool_name, tool_calls,
        content='agent_messages_fts_src', content_rowid='id'
    )",
    "CREATE TRIGGER IF NOT EXISTS agent_messages_fts_insert AFTER INSERT ON agent_messages BEGIN
        INSERT INTO agent_messages_fts(rowid, content, tool_name, tool_calls)
        VALUES (new.id,
                CASE WHEN new.role = 'tool' THEN substr(COALESCE(new.content, ''), 1, 8192) ELSE new.content END,
                new.tool_name, new.tool_calls);
    END",
    "CREATE TRIGGER IF NOT EXISTS agent_messages_fts_delete AFTER DELETE ON agent_messages BEGIN
        INSERT INTO agent_messages_fts(agent_messages_fts, rowid, content, tool_name, tool_calls)
        VALUES ('delete', old.id,
                CASE WHEN old.role = 'tool' THEN substr(COALESCE(old.content, ''), 1, 8192) ELSE old.content END,
                old.tool_name, old.tool_calls);
    END",
    "CREATE TRIGGER IF NOT EXISTS agent_messages_fts_update
        AFTER UPDATE OF content, tool_name, tool_calls, role ON agent_messages
        WHEN old.content IS NOT new.content OR old.tool_name IS NOT new.tool_name
          OR old.tool_calls IS NOT new.tool_calls OR old.role IS NOT new.role
    BEGIN
        INSERT INTO agent_messages_fts(agent_messages_fts, rowid, content, tool_name, tool_calls)
        VALUES ('delete', old.id,
                CASE WHEN old.role = 'tool' THEN substr(COALESCE(old.content, ''), 1, 8192) ELSE old.content END,
                old.tool_name, old.tool_calls);
        INSERT INTO agent_messages_fts(rowid, content, tool_name, tool_calls)
        VALUES (new.id,
                CASE WHEN new.role = 'tool' THEN substr(COALESCE(new.content, ''), 1, 8192) ELSE new.content END,
                new.tool_name, new.tool_calls);
    END",
    // Skill usage telemetry (Hermes keeps it in `.usage.json`), keyed by the
    // skills folder and the skill name.
    "CREATE TABLE IF NOT EXISTS skill_usage (
        root TEXT NOT NULL,
        name TEXT NOT NULL,
        created_by TEXT,
        use_count INTEGER NOT NULL DEFAULT 0,
        view_count INTEGER NOT NULL DEFAULT 0,
        patch_count INTEGER NOT NULL DEFAULT 0,
        last_used_at INTEGER,
        last_viewed_at INTEGER,
        last_patched_at INTEGER,
        created_at INTEGER NOT NULL,
        state TEXT NOT NULL DEFAULT 'active' CHECK (state IN ('active','stale','archived')),
        pinned INTEGER NOT NULL DEFAULT 0,
        archived_at INTEGER,
        PRIMARY KEY (root, name)
    )",
    // Cron jobs (Hermes `cron/jobs.json`). `next_run_at` is advanced with a
    // compare-and-swap before a run is dispatched (at most once per slot).
    "CREATE TABLE IF NOT EXISTS cron_jobs (
        id TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL,
        name TEXT NOT NULL,
        prompt TEXT NOT NULL,
        schedule TEXT NOT NULL,
        skills TEXT NOT NULL DEFAULT '[]',
        model_role TEXT,
        mode TEXT NOT NULL DEFAULT 'auto',
        workdir TEXT,
        timezone TEXT,
        grace_minutes INTEGER,
        repeat_times INTEGER,
        run_count INTEGER NOT NULL DEFAULT 0,
        state TEXT NOT NULL DEFAULT 'scheduled'
            CHECK (state IN ('scheduled','paused','completed','error')),
        next_run_at INTEGER,
        last_run_at INTEGER,
        last_status TEXT,
        last_error TEXT,
        last_task_id TEXT,
        origin TEXT NOT NULL DEFAULT 'user',
        created_at INTEGER NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS cron_jobs_due ON cron_jobs(state, next_run_at)",
    // --- end memory-skills ---
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
    "CREATE INDEX IF NOT EXISTS channel_idempotency_by_age ON channel_idempotency(created_at)",
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
    // --- design-studio ---
    // Design directions generated in the Design Studio. Each chat refinement
    // is a new row whose `parent_id` is the version it changed; `body` is the
    // direction as JSON (html, rationale, critique, lint).
    "CREATE TABLE IF NOT EXISTS design_directions (
        id TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL,
        batch_id TEXT NOT NULL,
        parent_id TEXT,
        prompt TEXT NOT NULL,
        frame TEXT NOT NULL,
        title TEXT NOT NULL,
        body TEXT NOT NULL,
        note TEXT NOT NULL DEFAULT '',
        reply TEXT NOT NULL DEFAULT '',
        chosen INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    )",
    "CREATE INDEX IF NOT EXISTS design_directions_by_workspace
        ON design_directions (workspace_id, created_at)",
    // --- end design-studio ---
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
        let rows = statement.query_map(rusqlite::params![workspace_id], rule_from_row)?;
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
        let rows = statement.query_map(rusqlite::params![task_id], step_snapshot_from_row)?;
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
        let rows = statement.query_map([], connection_from_row)?;
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
        let rows = statement.query_map([], combo_from_row)?;
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
        let mut rows = statement.query_map([session, combo], sticky_from_row)?;
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
        let rows = statement.query_map(rusqlite::params![since, limit], switch_from_row)?;
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
        let by_cost = "SUM(cost) DESC, SUM(requests) DESC";
        let group = |column: &str, order: &str| -> Result<Vec<UsageTotals>, StoreError> {
            let sql = format!(
                "SELECT {column}, SUM(requests), SUM(failures), SUM(prompt_tokens),
                    SUM(completion_tokens), SUM(cached_tokens), SUM(reasoning_tokens), SUM(cost)
                 FROM llm_usage_daily WHERE day >= ?1 GROUP BY {column}
                 ORDER BY {order}"
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
        let by_provider = group("provider", by_cost)?;
        let by_model = group("model", by_cost)?;
        let by_role = group("role", by_cost)?;
        let daily = group("day", "day ASC")?;
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
        let rows = statement.query_map(rusqlite::params![since, limit], usage_from_row)?;
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

// --- memory-skills ---------------------------------------------------------------
// Storage for `crate::learning` (ported from Hermes Agent): the session store
// and its FTS5 search, skill usage telemetry and cron jobs. Kept in one block
// so parallel branches merge cleanly.

/// `(task_id, action, target, decision, reason)` from [`Store::audit_recent`].
pub type AuditRowTuple = (Option<String>, String, String, String, String);

/// One `agent_sessions` row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionRow {
    pub id: String,
    pub workspace_id: String,
    pub task_id: Option<String>,
    /// Lineage: the session this one continues (search dedupes by root).
    pub parent_session_id: Option<String>,
    /// `task`, `cron` or `review`.
    pub source: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub system_prompt_hash: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub end_reason: Option<String>,
    pub message_count: i64,
}

/// One `agent_messages` row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MessageRow {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: Option<String>,
    /// JSON array of tool calls (assistant messages).
    pub tool_calls: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub ts: i64,
    pub token_count: Option<i64>,
    pub active: bool,
    pub compacted: bool,
}

/// Ordering of search hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchSort {
    #[default]
    Relevance,
    Newest,
    Oldest,
}

/// An FTS query over one workspace's messages.
#[derive(Debug, Clone, Default)]
pub struct MessageSearch<'a> {
    pub workspace_id: &'a str,
    /// An already-sanitised FTS5 expression.
    pub fts: &'a str,
    pub roles: &'a [&'a str],
    pub exclude_sources: &'a [&'a str],
    pub exclude_sessions: &'a [String],
    /// Session start bounds (unix seconds): `after` inclusive, `before` exclusive.
    pub after: Option<i64>,
    pub before: Option<i64>,
    pub sort: SearchSort,
    pub limit: usize,
}

/// One search hit.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageHit {
    pub message: MessageRow,
    pub snippet: String,
    pub session: SessionRow,
}

/// One `skill_usage` row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillUsageRow {
    pub root: String,
    pub name: String,
    pub created_by: Option<String>,
    pub use_count: i64,
    pub view_count: i64,
    pub patch_count: i64,
    pub last_used_at: Option<i64>,
    pub last_viewed_at: Option<i64>,
    pub last_patched_at: Option<i64>,
    pub created_at: i64,
    pub state: String,
    pub pinned: bool,
    pub archived_at: Option<i64>,
}

impl SkillUsageRow {
    /// Newest of last use, view and patch.
    #[must_use]
    pub fn last_activity_at(&self) -> Option<i64> {
        [self.last_used_at, self.last_viewed_at, self.last_patched_at]
            .into_iter()
            .flatten()
            .max()
    }
}

/// Which usage counter to bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillCounter {
    /// `skill_view`: view and use.
    View,
    /// Any change through `skill_manage`.
    Patch,
}

/// One `cron_jobs` row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CronJobRow {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub prompt: String,
    /// What the user typed (`every monday 9am`, `0 9 * * *`, …).
    pub schedule: String,
    /// JSON array of skill names loaded into the run's first message.
    pub skills: String,
    pub model_role: Option<String>,
    pub mode: String,
    pub workdir: Option<String>,
    pub timezone: Option<String>,
    pub grace_minutes: Option<i64>,
    /// `None` = forever.
    pub repeat_times: Option<i64>,
    pub run_count: i64,
    pub state: String,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_task_id: Option<String>,
    /// Who scheduled it: `user`, or e.g. `channel:telegram:<chat>`.
    pub origin: String,
    pub created_at: i64,
}

fn session_from_row(row: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        id: row.get(at)?,
        workspace_id: row.get(at + 1)?,
        task_id: row.get(at + 2)?,
        parent_session_id: row.get(at + 3)?,
        source: row.get(at + 4)?,
        title: row.get(at + 5)?,
        model: row.get(at + 6)?,
        system_prompt_hash: row.get(at + 7)?,
        started_at: row.get(at + 8)?,
        ended_at: row.get(at + 9)?,
        end_reason: row.get(at + 10)?,
        message_count: row.get(at + 11)?,
    })
}

const SESSION_COLUMNS: &str = "s.id, s.workspace_id, s.task_id, s.parent_session_id, s.source, s.title, s.model, s.system_prompt_hash, s.started_at, s.ended_at, s.end_reason, s.message_count";
const MESSAGE_COLUMNS: &str = "m.id, m.session_id, m.role, m.content, m.tool_calls, m.tool_call_id, m.tool_name, m.ts, m.token_count, m.active, m.compacted";

fn message_from_row(row: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<MessageRow> {
    Ok(MessageRow {
        id: row.get(at)?,
        session_id: row.get(at + 1)?,
        role: row.get(at + 2)?,
        content: row.get(at + 3)?,
        tool_calls: row.get(at + 4)?,
        tool_call_id: row.get(at + 5)?,
        tool_name: row.get(at + 6)?,
        ts: row.get(at + 7)?,
        token_count: row.get(at + 8)?,
        active: row.get::<_, i64>(at + 9)? != 0,
        compacted: row.get::<_, i64>(at + 10)? != 0,
    })
}

const SKILL_USAGE_COLUMNS: &str = "root, name, created_by, use_count, view_count, patch_count, last_used_at, last_viewed_at, last_patched_at, created_at, state, pinned, archived_at";

fn skill_usage_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillUsageRow> {
    Ok(SkillUsageRow {
        root: row.get(0)?,
        name: row.get(1)?,
        created_by: row.get(2)?,
        use_count: row.get(3)?,
        view_count: row.get(4)?,
        patch_count: row.get(5)?,
        last_used_at: row.get(6)?,
        last_viewed_at: row.get(7)?,
        last_patched_at: row.get(8)?,
        created_at: row.get(9)?,
        state: row.get(10)?,
        pinned: row.get::<_, i64>(11)? != 0,
        archived_at: row.get(12)?,
    })
}

const CRON_COLUMNS: &str = "id, workspace_id, name, prompt, schedule, skills, model_role, mode, workdir, timezone, grace_minutes, repeat_times, run_count, state, next_run_at, last_run_at, last_status, last_error, last_task_id, origin, created_at";

fn cron_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CronJobRow> {
    Ok(CronJobRow {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        name: row.get(2)?,
        prompt: row.get(3)?,
        schedule: row.get(4)?,
        skills: row.get(5)?,
        model_role: row.get(6)?,
        mode: row.get(7)?,
        workdir: row.get(8)?,
        timezone: row.get(9)?,
        grace_minutes: row.get(10)?,
        repeat_times: row.get(11)?,
        run_count: row.get(12)?,
        state: row.get(13)?,
        next_run_at: row.get(14)?,
        last_run_at: row.get(15)?,
        last_status: row.get(16)?,
        last_error: row.get(17)?,
        last_task_id: row.get(18)?,
        origin: row.get(19)?,
        created_at: row.get(20)?,
    })
}

fn connection_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectionRow> {
    Ok(ConnectionRow {
        id: row.get(0)?,
        provider: row.get(1)?,
        auth_type: row.get(2)?,
        name: row.get(3)?,
        priority: row.get(4)?,
        is_active: row.get(5)?,
        data: row.get(6)?,
    })
}

fn combo_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ComboRow> {
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
}

fn sticky_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StickyRow> {
    Ok(StickyRow {
        session: row.get(0)?,
        combo: row.get(1)?,
        model: row.get(2)?,
        window: row.get(3)?,
    })
}

fn rule_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RuleRow> {
    Ok(RuleRow {
        id: row.get(0)?,
        scope: row.get(1)?,
        workspace_id: row.get(2)?,
        effect: row.get(3)?,
        kind: row.get(4)?,
        pattern: row.get(5)?,
    })
}

fn step_snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StepSnapshotRow> {
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
}

fn switch_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SwitchRow> {
    Ok(SwitchRow {
        ts: row.get(0)?,
        session: row.get(1)?,
        combo: row.get(2)?,
        from_model: row.get(3)?,
        to_model: row.get(4)?,
        reason: row.get(5)?,
        confidence: row.get(6)?,
    })
}

fn usage_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageRow> {
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
}

impl Store {
    // -- sessions ------------------------------------------------------------

    /// Start (or restart) a session.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn session_start(&self, row: &SessionRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO agent_sessions (id, workspace_id, task_id, parent_session_id, source, title, model, system_prompt_hash, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET model = excluded.model, system_prompt_hash = excluded.system_prompt_hash",
            rusqlite::params![
                row.id, row.workspace_id, row.task_id, row.parent_session_id, row.source, row.title,
                row.model, row.system_prompt_hash, row.started_at
            ],
        )?;
        Ok(())
    }

    /// Store a system prompt once by content hash.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn system_prompt_put(&self, hash: &str, prompt: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO agent_system_prompts (hash, prompt) VALUES (?1, ?2)",
            rusqlite::params![hash, prompt],
        )?;
        Ok(())
    }

    /// Set a session's system prompt hash.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn session_set_prompt(&self, id: &str, hash: &str, model: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE agent_sessions SET system_prompt_hash = ?2, model = ?3 WHERE id = ?1",
            rusqlite::params![id, hash, model],
        )?;
        Ok(())
    }

    /// Mark a session finished.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn session_end(&self, id: &str, at: i64, reason: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE agent_sessions SET ended_at = ?2, end_reason = ?3 WHERE id = ?1",
            rusqlite::params![id, at, reason],
        )?;
        Ok(())
    }

    /// Append one message. Returns its id.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails (unknown session included).
    pub fn message_append(&self, message: &MessageRow) -> Result<i64, StoreError> {
        Ok(self
            .messages_append(std::slice::from_ref(message))?
            .pop()
            .unwrap_or_default())
    }

    /// Append messages of one session in one transaction. Returns their ids.
    ///
    /// # Errors
    /// Returns [`StoreError`] if a write fails; nothing is kept then.
    pub fn messages_append(&self, messages: &[MessageRow]) -> Result<Vec<i64>, StoreError> {
        let Some(first) = messages.first() else {
            return Ok(Vec::new());
        };
        let tx = self.conn.unchecked_transaction()?;
        let mut ids = Vec::with_capacity(messages.len());
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO agent_messages (session_id, role, content, tool_calls, tool_call_id, tool_name, ts, token_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for m in messages {
                insert.execute(rusqlite::params![
                    m.session_id,
                    m.role,
                    m.content,
                    m.tool_calls,
                    m.tool_call_id,
                    m.tool_name,
                    m.ts,
                    m.token_count
                ])?;
                ids.push(tx.last_insert_rowid());
            }
        }
        tx.execute(
            "UPDATE agent_sessions SET message_count = message_count + ?2 WHERE id = ?1",
            rusqlite::params![
                first.session_id,
                i64::try_from(messages.len()).unwrap_or(i64::MAX)
            ],
        )?;
        tx.commit()?;
        Ok(ids)
    }

    /// The first (`newest == false`) or last `n` messages of a session, in
    /// order.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn session_edge(
        &self,
        session_id: &str,
        newest: bool,
        n: usize,
    ) -> Result<Vec<MessageRow>, StoreError> {
        let sql = format!(
            "SELECT * FROM (SELECT {MESSAGE_COLUMNS} FROM agent_messages m WHERE m.session_id = ?1 ORDER BY m.id {} LIMIT ?2) ORDER BY 1",
            if newest { "DESC" } else { "ASC" }
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let rows = statement.query_map(
            rusqlite::params![session_id, i64::try_from(n).unwrap_or(i64::MAX)],
            |row| message_from_row(row, 0),
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Messages of a session with an id below `below` and above `above`.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn messages_outside(
        &self,
        session_id: &str,
        below: i64,
        above: i64,
    ) -> Result<(i64, i64), StoreError> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(id < ?2), 0), COALESCE(SUM(id > ?3), 0) FROM agent_messages WHERE session_id = ?1",
            rusqlite::params![session_id, below, above],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    }

    /// The first message of a role in a session.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn session_first(
        &self,
        session_id: &str,
        role: &str,
    ) -> Result<Option<MessageRow>, StoreError> {
        let sql = format!(
            "SELECT {MESSAGE_COLUMNS} FROM agent_messages m WHERE m.session_id = ?1 AND m.role = ?2 ORDER BY m.id LIMIT 1"
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let mut rows = statement.query_map(rusqlite::params![session_id, role], |row| {
            message_from_row(row, 0)
        })?;
        Ok(rows.next().transpose()?)
    }

    /// Message count of a session.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn session_len(&self, session_id: &str) -> Result<usize, StoreError> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM agent_messages WHERE session_id = ?1",
            rusqlite::params![session_id],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(n).unwrap_or(0))
    }

    /// Compaction replaced the live context: archive the active messages.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn messages_compacted(&self, session_id: &str) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "UPDATE agent_messages SET active = 0, compacted = 1 WHERE session_id = ?1 AND active = 1",
            rusqlite::params![session_id],
        )?)
    }

    /// One session.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn session_get(&self, id: &str) -> Result<Option<SessionRow>, StoreError> {
        let sql = format!("SELECT {SESSION_COLUMNS} FROM agent_sessions s WHERE s.id = ?1");
        let mut statement = self.conn.prepare_cached(&sql)?;
        let mut rows =
            statement.query_map(rusqlite::params![id], |row| session_from_row(row, 0))?;
        Ok(rows.next().transpose()?)
    }

    /// Most recent sessions of a workspace.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn sessions_recent(
        &self,
        workspace_id: &str,
        exclude_sources: &[&str],
        limit: usize,
    ) -> Result<Vec<SessionRow>, StoreError> {
        let excluded = serde_json::to_string(exclude_sources).unwrap_or_else(|_| "[]".into());
        let sql = format!(
            "SELECT {SESSION_COLUMNS} FROM agent_sessions s
             WHERE s.workspace_id = ?1 AND s.source NOT IN (SELECT value FROM json_each(?2))
             ORDER BY s.started_at DESC, s.rowid DESC LIMIT ?3"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(
            rusqlite::params![
                workspace_id,
                excluded,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            |row| session_from_row(row, 0),
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// A session's messages in order (`active_only` skips compacted ones).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn session_messages(
        &self,
        session_id: &str,
        active_only: bool,
    ) -> Result<Vec<MessageRow>, StoreError> {
        let sql = format!(
            "SELECT {MESSAGE_COLUMNS} FROM agent_messages m WHERE m.session_id = ?1 {} ORDER BY m.id",
            if active_only { "AND m.active = 1" } else { "" }
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params![session_id], |row| {
            message_from_row(row, 0)
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Up to `window` messages on each side of `around` (inclusive).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn messages_around(
        &self,
        session_id: &str,
        around: i64,
        window: usize,
    ) -> Result<Vec<MessageRow>, StoreError> {
        let window = i64::try_from(window).unwrap_or(i64::MAX);
        let sql = format!(
            "SELECT * FROM (
                SELECT {MESSAGE_COLUMNS} FROM agent_messages m WHERE m.session_id = ?1 AND m.id < ?2 ORDER BY m.id DESC LIMIT ?3
             )
             UNION ALL
             SELECT * FROM (
                SELECT {MESSAGE_COLUMNS} FROM agent_messages m WHERE m.session_id = ?1 AND m.id >= ?2 ORDER BY m.id ASC LIMIT ?3 + 1
             )
             ORDER BY 1"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params![session_id, around, window], |row| {
            message_from_row(row, 0)
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Full-text search over a workspace's messages.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails (including an FTS syntax
    /// error the sanitiser did not catch).
    pub fn messages_search(
        &self,
        query: &MessageSearch<'_>,
    ) -> Result<Vec<MessageHit>, StoreError> {
        let roles = serde_json::to_string(query.roles).unwrap_or_else(|_| "[]".into());
        let sources = serde_json::to_string(query.exclude_sources).unwrap_or_else(|_| "[]".into());
        let sessions =
            serde_json::to_string(query.exclude_sessions).unwrap_or_else(|_| "[]".into());
        let order = match query.sort {
            SearchSort::Relevance => "rank",
            SearchSort::Newest => "m.ts DESC, rank",
            SearchSort::Oldest => "m.ts ASC, rank",
        };
        let sql = format!(
            "SELECT {MESSAGE_COLUMNS}, snippet(agent_messages_fts, 0, '>>>', '<<<', '...', 40), {SESSION_COLUMNS}
             FROM agent_messages_fts f
             JOIN agent_messages m ON m.id = f.rowid
             JOIN agent_sessions s ON s.id = m.session_id
             WHERE agent_messages_fts MATCH ?1
               AND s.workspace_id = ?2
               AND m.role IN (SELECT value FROM json_each(?3))
               AND s.source NOT IN (SELECT value FROM json_each(?4))
               AND s.id NOT IN (SELECT value FROM json_each(?5))
               AND (?6 IS NULL OR s.started_at >= ?6)
               AND (?7 IS NULL OR s.started_at < ?7)
             ORDER BY {order}
             LIMIT ?8"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(
            rusqlite::params![
                query.fts,
                query.workspace_id,
                roles,
                sources,
                sessions,
                query.after,
                query.before,
                i64::try_from(query.limit).unwrap_or(i64::MAX)
            ],
            |row| {
                Ok(MessageHit {
                    message: message_from_row(row, 0)?,
                    snippet: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
                    session: session_from_row(row, 12)?,
                })
            },
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Recent audit rows `(task_id, action, target, decision, reason)`,
    /// newest first, optionally of one task.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn audit_recent(
        &self,
        task_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AuditRowTuple>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT task_id, action, target, decision, reason FROM audit_log
             WHERE ?1 IS NULL OR task_id = ?1 ORDER BY seq DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(
            rusqlite::params![task_id, i64::try_from(limit).unwrap_or(i64::MAX)],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // -- skill usage ---------------------------------------------------------

    /// Usage rows of one skills folder.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn skill_usage(&self, root: &str) -> Result<Vec<SkillUsageRow>, StoreError> {
        let sql =
            format!("SELECT {SKILL_USAGE_COLUMNS} FROM skill_usage WHERE root = ?1 ORDER BY name");
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params![root], skill_usage_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// A skill was created: reset its record and note who made it.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn skill_created(
        &self,
        root: &str,
        name: &str,
        created_by: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO skill_usage (root, name, created_by, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(root, name) DO UPDATE SET created_by = excluded.created_by, created_at = excluded.created_at,
                use_count = 0, view_count = 0, patch_count = 0, last_used_at = NULL, last_viewed_at = NULL,
                last_patched_at = NULL, state = 'active', archived_at = NULL",
            rusqlite::params![root, name, created_by, now],
        )?;
        Ok(())
    }

    /// Bump a usage counter (creates the row on first sight).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn skill_bump(
        &self,
        root: &str,
        name: &str,
        counter: SkillCounter,
        now: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO skill_usage (root, name, created_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![root, name, now],
        )?;
        let sql = match counter {
            SkillCounter::View => "UPDATE skill_usage SET view_count = view_count + 1, use_count = use_count + 1,
                last_viewed_at = ?3, last_used_at = ?3, state = CASE WHEN state = 'stale' THEN 'active' ELSE state END
                WHERE root = ?1 AND name = ?2",
            SkillCounter::Patch => "UPDATE skill_usage SET patch_count = patch_count + 1, last_patched_at = ?3,
                state = CASE WHEN state = 'stale' THEN 'active' ELSE state END
                WHERE root = ?1 AND name = ?2",
        };
        self.conn.execute(sql, rusqlite::params![root, name, now])?;
        Ok(())
    }

    /// Set a skill's lifecycle state (`active`, `stale`, `archived`).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails or the state is unknown.
    pub fn skill_set_state(
        &self,
        root: &str,
        name: &str,
        state: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO skill_usage (root, name, created_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![root, name, now],
        )?;
        self.conn.execute(
            "UPDATE skill_usage SET state = ?3,
                archived_at = CASE WHEN ?3 = 'archived' THEN ?4 WHEN ?3 = 'active' THEN NULL ELSE archived_at END
             WHERE root = ?1 AND name = ?2",
            rusqlite::params![root, name, state, now],
        )?;
        Ok(())
    }

    /// Pin or unpin a skill (pinned skills are never curated or deleted).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn skill_set_pinned(
        &self,
        root: &str,
        name: &str,
        pinned: bool,
        now: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO skill_usage (root, name, created_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![root, name, now],
        )?;
        self.conn.execute(
            "UPDATE skill_usage SET pinned = ?3 WHERE root = ?1 AND name = ?2",
            rusqlite::params![root, name, i64::from(pinned)],
        )?;
        Ok(())
    }

    // -- cron ----------------------------------------------------------------

    /// Insert or replace a job.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn cron_put(&self, job: &CronJobRow) -> Result<(), StoreError> {
        self.conn.execute(
            &format!("INSERT OR REPLACE INTO cron_jobs ({CRON_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)"),
            rusqlite::params![
                job.id, job.workspace_id, job.name, job.prompt, job.schedule, job.skills, job.model_role,
                job.mode, job.workdir, job.timezone, job.grace_minutes, job.repeat_times, job.run_count,
                job.state, job.next_run_at, job.last_run_at, job.last_status, job.last_error,
                job.last_task_id, job.origin, job.created_at
            ],
        )?;
        Ok(())
    }

    /// One job.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn cron_get(&self, id: &str) -> Result<Option<CronJobRow>, StoreError> {
        let sql = format!("SELECT {CRON_COLUMNS} FROM cron_jobs WHERE id = ?1");
        let mut statement = self.conn.prepare(&sql)?;
        let mut rows = statement.query_map(rusqlite::params![id], cron_from_row)?;
        Ok(rows.next().transpose()?)
    }

    /// Jobs, optionally of one workspace, oldest first.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn cron_list(&self, workspace_id: Option<&str>) -> Result<Vec<CronJobRow>, StoreError> {
        let sql = format!(
            "SELECT {CRON_COLUMNS} FROM cron_jobs WHERE ?1 IS NULL OR workspace_id = ?1 ORDER BY created_at, id"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params![workspace_id], cron_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Scheduled jobs whose `next_run_at` has come (indexed; the 20 s
    /// scheduler tick reads only these).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn cron_due(&self, now: i64) -> Result<Vec<CronJobRow>, StoreError> {
        let sql = format!(
            "SELECT {CRON_COLUMNS} FROM cron_jobs WHERE state = 'scheduled' AND next_run_at <= ?1 ORDER BY next_run_at, id"
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let rows = statement.query_map(rusqlite::params![now], cron_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Delete a job. Returns whether it existed.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn cron_delete(&self, id: &str) -> Result<bool, StoreError> {
        Ok(self
            .conn
            .execute("DELETE FROM cron_jobs WHERE id = ?1", rusqlite::params![id])?
            > 0)
    }

    /// Claim a due run: move `next_run_at` from `expected` to `next` (and the
    /// state, for a finished one-shot) only if nobody else did. `true` means
    /// this caller owns the run. This is the at-most-once step: it happens
    /// before the run is dispatched.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn cron_claim(
        &self,
        id: &str,
        expected: i64,
        next: Option<i64>,
        state: &str,
        runs: bool,
    ) -> Result<bool, StoreError> {
        // The run counts toward `repeat_times` at claim time, so a run that
        // later fails to start still uses its slot (at most once).
        Ok(self.conn.execute(
            "UPDATE cron_jobs SET next_run_at = ?3, state = ?4, run_count = run_count + ?5
             WHERE id = ?1 AND next_run_at = ?2 AND state = 'scheduled'",
            rusqlite::params![id, expected, next, state, i64::from(runs)],
        )? == 1)
    }

    /// Record a dispatched run.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn cron_record_run(
        &self,
        id: &str,
        at: i64,
        status: &str,
        error: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE cron_jobs SET last_run_at = ?2, last_status = ?3, last_error = ?4,
                last_task_id = COALESCE(?5, last_task_id)
             WHERE id = ?1",
            rusqlite::params![id, at, status, error, task_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::assert_is_empty, clippy::bool_assert_comparison)]
mod memory_skills_tests {
    use super::*;

    fn session(
        store: &Store,
        id: &str,
        ws: &str,
        source: &str,
        started: i64,
        parent: Option<&str>,
    ) {
        store
            .session_start(&SessionRow {
                id: id.into(),
                workspace_id: ws.into(),
                source: source.into(),
                started_at: started,
                parent_session_id: parent.map(str::to_string),
                ..SessionRow::default()
            })
            .expect("session");
    }

    fn message(store: &Store, session: &str, role: &str, content: &str) -> i64 {
        store
            .message_append(&MessageRow {
                session_id: session.into(),
                role: role.into(),
                content: Some(content.into()),
                ts: 1,
                ..MessageRow::default()
            })
            .expect("append")
    }

    fn search<'a>(ws: &'a str, fts: &'a str, roles: &'a [&'a str]) -> MessageSearch<'a> {
        MessageSearch {
            workspace_id: ws,
            fts,
            roles,
            limit: 300,
            ..MessageSearch::default()
        }
    }

    #[test]
    fn fts_finds_messages_in_one_workspace_only() {
        let store = Store::open_in_memory().expect("open");
        session(&store, "s1", "ws1", "task", 10, None);
        session(&store, "s2", "ws2", "task", 20, None);
        message(&store, "s1", "user", "fix the flaky websocket test");
        message(&store, "s2", "user", "websocket in another workspace");
        let hits = store
            .messages_search(&search("ws1", "websocket", &["user", "assistant"]))
            .expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session.id, "s1");
        assert!(
            hits[0].snippet.contains(">>>websocket<<<"),
            "{}",
            hits[0].snippet
        );
        assert_eq!(
            store
                .session_get("s1")
                .expect("get")
                .expect("row")
                .message_count,
            1
        );
    }

    #[test]
    fn tool_output_is_indexed_only_up_to_8192_chars() {
        let store = Store::open_in_memory().expect("open");
        session(&store, "s1", "ws", "task", 1, None);
        let long = format!(
            "{} needleearly {} needlelate",
            "x ".repeat(10),
            "y ".repeat(5_000)
        );
        message(&store, "s1", "tool", &long);
        let tool = ["tool"];
        assert_eq!(
            store
                .messages_search(&search("ws", "needleearly", &tool))
                .expect("q")
                .len(),
            1
        );
        assert!(store
            .messages_search(&search("ws", "needlelate", &tool))
            .expect("q")
            .is_empty());
        // The full text is still stored.
        assert!(store.session_messages("s1", false).expect("m")[0]
            .content
            .as_deref()
            .expect("c")
            .ends_with("needlelate"));
    }

    #[test]
    fn index_follows_updates_and_deletes() {
        let store = Store::open_in_memory().expect("open");
        session(&store, "s1", "ws", "task", 1, None);
        let id = message(&store, "s1", "user", "alpha");
        store
            .conn
            .execute(
                "UPDATE agent_messages SET content = 'beta' WHERE id = ?1",
                [id],
            )
            .expect("update");
        let roles = ["user"];
        assert!(store
            .messages_search(&search("ws", "alpha", &roles))
            .expect("q")
            .is_empty());
        assert_eq!(
            store
                .messages_search(&search("ws", "beta", &roles))
                .expect("q")
                .len(),
            1
        );
        store
            .conn
            .execute("DELETE FROM agent_sessions WHERE id = 's1'", [])
            .expect("delete");
        assert!(store
            .messages_search(&search("ws", "beta", &roles))
            .expect("q")
            .is_empty());
        // The index stays consistent after the cascade.
        store
            .conn
            .execute(
                "INSERT INTO agent_messages_fts(agent_messages_fts) VALUES('integrity-check')",
                [],
            )
            .expect("integrity");
    }

    #[test]
    fn search_filters_sources_sessions_and_time() {
        let store = Store::open_in_memory().expect("open");
        session(&store, "a", "ws", "task", 100, None);
        session(&store, "b", "ws", "review", 200, None);
        session(&store, "c", "ws", "cron", 300, None);
        for s in ["a", "b", "c"] {
            message(&store, s, "assistant", "deploy notes");
        }
        let roles = ["assistant"];
        let excluded = ["c".to_string()];
        let q = MessageSearch {
            exclude_sources: &["review"],
            exclude_sessions: &excluded,
            ..search("ws", "deploy", &roles)
        };
        let ids: Vec<String> = store
            .messages_search(&q)
            .expect("q")
            .into_iter()
            .map(|h| h.session.id)
            .collect();
        assert_eq!(ids, ["a"]);
        let q = MessageSearch {
            after: Some(150),
            before: Some(301),
            ..search("ws", "deploy", &roles)
        };
        assert_eq!(store.messages_search(&q).expect("q").len(), 2);
    }

    #[test]
    fn windows_compaction_and_recent_sessions() {
        let store = Store::open_in_memory().expect("open");
        session(&store, "s", "ws", "task", 5, None);
        session(&store, "t", "ws", "review", 6, None);
        let ids: Vec<i64> = (0..10)
            .map(|i| message(&store, "s", "user", &format!("m{i}")))
            .collect();
        let window = store.messages_around("s", ids[5], 2).expect("window");
        assert_eq!(
            window.iter().map(|m| m.id).collect::<Vec<_>>(),
            ids[3..=7].to_vec()
        );
        assert_eq!(store.messages_compacted("s").expect("compact"), 10);
        assert!(store
            .session_messages("s", true)
            .expect("active")
            .is_empty());
        assert_eq!(store.session_messages("s", false).expect("all").len(), 10);
        let recent = store
            .sessions_recent("ws", &["review"], 10)
            .expect("recent");
        assert_eq!(
            recent.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["s"]
        );
    }

    #[test]
    fn skill_usage_counters_and_state() {
        let store = Store::open_in_memory().expect("open");
        store
            .skill_created("/r", "deploy", "agent", 10)
            .expect("create");
        store
            .skill_bump("/r", "deploy", SkillCounter::View, 20)
            .expect("view");
        store
            .skill_bump("/r", "deploy", SkillCounter::Patch, 30)
            .expect("patch");
        store
            .skill_bump("/r", "hand", SkillCounter::View, 40)
            .expect("view");
        store
            .skill_set_state("/r", "deploy", "archived", 50)
            .expect("state");
        store.skill_set_pinned("/r", "hand", true, 60).expect("pin");
        let rows = store.skill_usage("/r").expect("rows");
        let deploy = &rows[0];
        assert_eq!(
            (deploy.use_count, deploy.view_count, deploy.patch_count),
            (1, 1, 1)
        );
        assert_eq!(
            (deploy.state.as_str(), deploy.archived_at),
            ("archived", Some(50))
        );
        assert_eq!(deploy.created_by.as_deref(), Some("agent"));
        assert!(rows[1].pinned);
        assert_eq!(rows[1].created_by, None);
        assert!(store.skill_set_state("/r", "deploy", "bogus", 1).is_err());
    }

    #[test]
    fn cron_claim_is_compare_and_swap() {
        let store = Store::open_in_memory().expect("open");
        let job = CronJobRow {
            id: "j1".into(),
            workspace_id: "ws".into(),
            name: "n".into(),
            prompt: "p".into(),
            schedule: "every 1h".into(),
            skills: "[]".into(),
            mode: "auto".into(),
            state: "scheduled".into(),
            next_run_at: Some(100),
            origin: "user".into(),
            created_at: 1,
            ..CronJobRow::default()
        };
        store.cron_put(&job).expect("put");
        assert!(store
            .cron_claim("j1", 100, Some(3_700), "scheduled", true)
            .expect("claim"));
        // A second claimer for the same slot loses.
        assert!(!store
            .cron_claim("j1", 100, Some(3_700), "scheduled", true)
            .expect("again"));
        store
            .cron_record_run("j1", 101, "started", None, Some("task1"))
            .expect("run");
        let row = store.cron_get("j1").expect("get").expect("row");
        assert_eq!(
            (row.next_run_at, row.run_count, row.last_task_id.as_deref()),
            (Some(3_700), 1, Some("task1"))
        );
        assert_eq!(store.cron_list(Some("ws")).expect("list").len(), 1);
        assert!(store.cron_list(Some("other")).expect("list").is_empty());
        assert!(store.cron_delete("j1").expect("delete"));
    }
}

// --- end memory-skills -----------------------------------------------------------

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
        tx.execute(
            "DELETE FROM channel_pairings WHERE channel_id = ?1",
            [channel_id],
        )?;
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
    pub fn channel_allow_remove(
        &self,
        channel_id: &str,
        sender_id: &str,
    ) -> Result<bool, StoreError> {
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

    /// Forget an idempotency key (the claimed send failed).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the write fails.
    pub fn channel_release(&self, key: &str) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM channel_idempotency WHERE key = ?1", [key])?;
        Ok(())
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
        store
            .channel_upsert(&ChannelRow {
                name: "Bot 2".into(),
                ..row.clone()
            })
            .expect("update");
        assert_eq!(store.channel_rows().expect("rows")[0].name, "Bot 2");
        store
            .channel_pairings_replace("ch_1", &[pairing("a", "AAAA")])
            .expect("pair");
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
        assert_eq!(store.channel_rows().expect("rows").len(), 0);
        assert_eq!(store.channel_pairings("ch_1").expect("p").len(), 0);
        assert_eq!(store.channel_allow_list("ch_1").expect("a").len(), 0);
    }

    #[test]
    fn pairings_replace_atomically() {
        let store = Store::open_in_memory().expect("store");
        store
            .channel_pairings_replace("ch_1", &[pairing("a", "AAAA"), pairing("b", "BBBB")])
            .expect("two");
        store
            .channel_pairings_replace("ch_1", &[pairing("b", "BBBB")])
            .expect("one");
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
        assert_ne!(list[0].added_at.len(), 0);
        assert!(store.channel_allow_remove("ch_1", "42").expect("rm"));
        assert!(!store.channel_allow_remove("ch_1", "42").expect("rm again"));
    }

    #[test]
    fn claims_are_idempotent() {
        let store = Store::open_in_memory().expect("store");
        assert!(store.channel_claim("in:ch_1:1", "").expect("first"));
        assert!(!store.channel_claim("in:ch_1:1", "").expect("second"));
        assert!(store.channel_claim("out:ch_1:1:0", "m9").expect("out"));
        assert_eq!(
            store.channel_claimed("out:ch_1:1:0").expect("get"),
            Some("m9".into())
        );
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
        assert_eq!(
            rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            ["m4", "m3"]
        );
        assert_eq!(store.channel_activity("other", 10).expect("none").len(), 0);
    }
}
// --- end channels ---------------------------------------------------------------

// --- design-studio -------------------------------------------------------------

/// One stored design direction (or a chat refinement of one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignDirectionRow {
    pub id: String,
    pub workspace_id: String,
    pub batch_id: String,
    pub parent_id: Option<String>,
    pub prompt: String,
    pub frame: String,
    pub title: String,
    /// The direction as JSON.
    pub body: String,
    /// The chat message that produced this version (empty for originals).
    pub note: String,
    /// The model's one-line reply to `note`.
    pub reply: String,
    pub chosen: bool,
    pub created_at: String,
}

const DESIGN_COLUMNS: &str = "id, workspace_id, batch_id, parent_id, prompt, frame, title, body, note, reply, chosen, created_at";

fn design_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DesignDirectionRow> {
    Ok(DesignDirectionRow {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        batch_id: row.get(2)?,
        parent_id: row.get(3)?,
        prompt: row.get(4)?,
        frame: row.get(5)?,
        title: row.get(6)?,
        body: row.get(7)?,
        note: row.get(8)?,
        reply: row.get(9)?,
        chosen: row.get::<_, i64>(10)? != 0,
        created_at: row.get(11)?,
    })
}

impl Store {
    /// Save a direction (`created_at` and `chosen` are set by the database).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the insert fails.
    pub fn design_direction_insert(&self, row: &DesignDirectionRow) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO design_directions (id, workspace_id, batch_id, parent_id, prompt, frame, title, body, note, reply)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                row.id,
                row.workspace_id,
                row.batch_id,
                row.parent_id,
                row.prompt,
                row.frame,
                row.title,
                row.body,
                row.note,
                row.reply
            ],
        )?;
        Ok(())
    }

    /// One direction by id.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn design_direction(&self, id: &str) -> Result<Option<DesignDirectionRow>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {DESIGN_COLUMNS} FROM design_directions WHERE id = ?1"
        ))?;
        let mut rows = statement.query_map(rusqlite::params![id], design_row)?;
        Ok(rows.next().transpose()?)
    }

    /// A workspace's most recent directions, newest first.
    ///
    /// # Errors
    /// Returns [`StoreError`] if the query fails.
    pub fn design_directions(
        &self,
        workspace_id: &str,
        limit: u32,
    ) -> Result<Vec<DesignDirectionRow>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {DESIGN_COLUMNS} FROM design_directions WHERE workspace_id = ?1
             ORDER BY created_at DESC, rowid DESC LIMIT ?2"
        ))?;
        let rows = statement.query_map(rusqlite::params![workspace_id, limit], design_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Mark `id` as the chosen direction of its batch (and no other).
    ///
    /// # Errors
    /// Returns [`StoreError`] if the update fails.
    pub fn design_direction_choose(&self, id: &str) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "UPDATE design_directions SET chosen = (id = ?1)
             WHERE batch_id = (SELECT batch_id FROM design_directions WHERE id = ?1)",
            rusqlite::params![id],
        )?;
        Ok(changed > 0)
    }
}

#[cfg(test)]
mod design_studio_tests {
    use super::*;

    fn row(id: &str, batch: &str, parent: Option<&str>) -> DesignDirectionRow {
        DesignDirectionRow {
            id: id.into(),
            workspace_id: "ws".into(),
            batch_id: batch.into(),
            parent_id: parent.map(str::to_string),
            prompt: "p".into(),
            frame: "web".into(),
            title: "T".into(),
            body: "{}".into(),
            note: String::new(),
            reply: String::new(),
            chosen: false,
            created_at: String::new(),
        }
    }

    #[test]
    fn directions_round_trip_and_choose() {
        let store = Store::open_in_memory().expect("store");
        store
            .design_direction_insert(&row("a", "b1", None))
            .expect("a");
        store
            .design_direction_insert(&row("b", "b1", None))
            .expect("b");
        store
            .design_direction_insert(&row("c", "b1", Some("a")))
            .expect("c");
        store
            .design_direction_insert(&row("z", "b2", None))
            .expect("z");
        let got = store.design_direction("c").expect("q").expect("row");
        assert_eq!(got.parent_id.as_deref(), Some("a"));
        assert!(store.design_direction("nope").expect("q").is_none());
        assert!(store.design_direction_choose("b").expect("choose"));
        assert!(store.design_direction_choose("a").expect("choose"));
        let all = store.design_directions("ws", 10).expect("list");
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].id, "z", "newest first");
        let chosen: Vec<_> = all
            .iter()
            .filter(|r| r.chosen)
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(chosen, vec!["a"]);
        assert_eq!(store.design_directions("other", 10).expect("list").len(), 0);
    }
}

// --- end design-studio ---------------------------------------------------------

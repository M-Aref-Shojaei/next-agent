//! SQLite metadata storage for workspaces, tasks, events and settings.
//!
//! The `WebView` never touches the database; this module is the only writer.

use crate::workspace::Workspace;
use std::path::Path;

/// Schema version applied by [`Store::open`].
pub const SCHEMA_VERSION: i32 = 2;

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
    fn schema_version_is_two() {
        let store = Store::open_in_memory().expect("open");
        assert_eq!(store.schema_version().expect("version"), 2);
        assert_eq!(SCHEMA_VERSION, 2);
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

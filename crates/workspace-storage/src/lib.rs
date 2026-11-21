// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! GPUI-free transactional persistence for shared Workspace state.
//!
//! Callers must run database operations off the GPUI foreground thread. The
//! store serializes writers with SQLite `BEGIN IMMEDIATE` transactions,
//! validates per-window leases, and applies typed changes against a Space
//! revision. Tabs are stored individually, so a write never replaces another
//! Space's or process's complete workspace snapshot.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 1;
const DEFAULT_IDENTITY: &str = r#"{"kind":"standalone"}"#;
const META_LEASE_GENERATION: &str = "lease_generation";
const META_USAGE_SEQUENCE: &str = "space_usage_sequence";
const META_LEGACY_IMPORTED: &str = "legacy_snapshot_imported";

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            pub fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                value.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

id_type!(WorkspaceInstanceId);
id_type!(SpaceId);
id_type!(TabId);
id_type!(PaneId);

/// A lease held by one Workspace window. The generation fences stale
/// mutation, renewal, switching, and release calls from a prior claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceLease {
    pub instance_id: WorkspaceInstanceId,
    pub space_id: SpaceId,
    pub generation: u64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredTab {
    pub id: TabId,
    /// Owner-serialized tab and pane state. Any pane identity inside this
    /// payload is a stable Workspace `PaneId`, not a GPUI entity or counter.
    pub payload: Value,
    /// Durable terminal scrollback keys referenced by this tab.
    #[serde(default)]
    pub scrollback_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSpace {
    pub id: SpaceId,
    pub name: String,
    pub identity: Value,
    pub revision: u64,
    pub last_used_sequence: u64,
    pub tabs: Vec<StoredTab>,
    /// Number of current Workspace windows holding a lease for this Space.
    pub participant_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceSummary {
    pub id: SpaceId,
    pub name: String,
    pub identity: Value,
    pub last_used_sequence: u64,
    pub participant_count: u32,
    pub tab_count: u32,
}

/// Typed structural operation. Every operation is committed at one expected
/// Space revision and produces exactly one change-log entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkspaceChange {
    AddTab { tab: StoredTab, position: u32 },
    RemoveTab { tab_id: TabId },
    MoveTab { tab_id: TabId, position: u32 },
    UpdateTab { tab: StoredTab },
    RenameSpace { name: String },
    SetIdentity { identity: Value },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommittedChange {
    pub space_id: SpaceId,
    pub revision: u64,
    pub change: WorkspaceChange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeBatch {
    pub current_revision: u64,
    /// `true` means the caller must reload the complete Space because the log
    /// no longer contains a contiguous suffix after its last revision.
    pub requires_snapshot: bool,
    pub changes: Vec<CommittedChange>,
}

/// Rows to import from one existing session.json snapshot. The caller assigns
/// UUIDs once while translating the legacy snapshot and leaves the source file
/// in place until it verifies the resulting Store state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedSpace {
    pub id: SpaceId,
    pub name: String,
    pub identity: Value,
    pub tabs: Vec<StoredTab>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcPermit {
    pub key: String,
    pub generation: u64,
    pub expires_at: i64,
}

#[derive(Debug)]
pub enum StoreError {
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    Io(std::io::Error),
    UnsupportedSchema(i64),
    LeaseLost,
    StaleRevision { expected: u64, current: u64 },
    SpaceNotFound(SpaceId),
    TabNotFound(TabId),
    TabAlreadyExists(TabId),
    ScrollbackTombstoned(String),
    Invalid(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql(error) => write!(f, "workspace database error: {error}"),
            Self::Json(error) => write!(f, "invalid workspace data: {error}"),
            Self::Io(error) => write!(f, "workspace storage I/O error: {error}"),
            Self::UnsupportedSchema(version) => {
                write!(
                    f,
                    "workspace database schema {version} is newer than this build"
                )
            }
            Self::LeaseLost => write!(f, "workspace lease is no longer current"),
            Self::StaleRevision { expected, current } => write!(
                f,
                "workspace changed at revision {current}; caller expected revision {expected}"
            ),
            Self::SpaceNotFound(id) => write!(f, "workspace Space {id} does not exist"),
            Self::TabNotFound(id) => write!(f, "workspace tab {id} does not exist"),
            Self::TabAlreadyExists(id) => write!(f, "workspace tab {id} already exists"),
            Self::ScrollbackTombstoned(key) => {
                write!(
                    f,
                    "scrollback key {key} is retired and cannot be reintroduced"
                )
            }
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

type StoreResult<T> = Result<T, StoreError>;

/// Workspace-owned SQLite store. A mutex protects its single connection;
/// SQLite's immediate transaction lock coordinates other app processes.
pub struct WorkspaceStorage {
    connection: Mutex<Connection>,
}

impl WorkspaceStorage {
    /// Open or create the dedicated private workspace database at `path`.
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            let parent_existed = parent.exists();
            fs::create_dir_all(parent)?;
            #[cfg(unix)]
            if parent_existed {
                use std::os::unix::fs::MetadataExt;
                let mode = fs::metadata(parent)?.mode();
                if mode & 0o077 != 0 {
                    return Err(StoreError::Invalid(format!(
                        "workspace database directory {} is accessible by other users",
                        parent.display()
                    )));
                }
            } else {
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
        }

        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        options.mode(0o600);
        drop(options.open(path)?);
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;

        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    /// In-memory store, used by owner tests that do not need process races.
    pub fn in_memory() -> StoreResult<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> StoreResult<Self> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        initialize_schema(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Atomically take the most-recently-used unleased Space, seed the first
    /// Standalone Space, or create a new empty Space when all existing Spaces
    /// have active participants.
    pub fn claim_startup(
        &self,
        instance_id: WorkspaceInstanceId,
        now: i64,
        lease_ttl: Duration,
        restore_enabled: bool,
    ) -> StoreResult<(WorkspaceLease, StoredSpace)> {
        let mut connection = self.connection();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        reap_expired(&tx, now)?;
        if !restore_enabled && active_lease_count(&tx, now)? == 0 {
            purge_workspace_state(&tx)?;
        }

        if let Some(existing) = lease_for_instance(&tx, instance_id)? {
            if existing.expires_at > now {
                let space = read_space(&tx, existing.space_id, now)?
                    .ok_or(StoreError::SpaceNotFound(existing.space_id))?;
                tx.commit()?;
                return Ok((existing, space));
            }
        }

        let spaces = space_order(&tx)?;
        if spaces.is_empty() {
            let id = SpaceId::new();
            tx.execute(
                "INSERT INTO spaces(space_id, name, identity_json, revision, last_used_sequence) \
                 VALUES (?1, 'Default', ?2, 0, 0)",
                params![id.to_string(), DEFAULT_IDENTITY],
            )?;
        }

        let space_id = find_unleased_space(&tx, now)?;
        let space_id = match space_id {
            Some(id) => id,
            None => {
                let (_name, identity_json) = most_recent_identity(&tx)?;
                let id = SpaceId::new();
                let next_number: i64 =
                    tx.query_row("SELECT COUNT(*) + 1 FROM spaces", [], |row| row.get(0))?;
                tx.execute(
                    "INSERT INTO spaces(space_id, name, identity_json, revision, last_used_sequence) \
                     VALUES (?1, ?2, ?3, 0, 0)",
                    params![id.to_string(), format!("Space {next_number}"), identity_json],
                )?;
                id
            }
        };
        let lease = claim_lease(&tx, instance_id, space_id, now, lease_ttl)?;
        advance_usage(&tx, space_id)?;
        let space = read_space(&tx, space_id, now)?.ok_or(StoreError::SpaceNotFound(space_id))?;
        tx.commit()?;
        Ok((lease, space))
    }

    /// Explicitly join an occupied Space or switch this window's current
    /// participant lease to another Space.
    pub fn join_space(
        &self,
        instance_id: WorkspaceInstanceId,
        space_id: SpaceId,
        now: i64,
        lease_ttl: Duration,
    ) -> StoreResult<(WorkspaceLease, StoredSpace)> {
        let mut connection = self.connection();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if read_space(&tx, space_id, now)?.is_none() {
            return Err(StoreError::SpaceNotFound(space_id));
        }
        let lease = claim_lease(&tx, instance_id, space_id, now, lease_ttl)?;
        advance_usage(&tx, space_id)?;
        let space = read_space(&tx, space_id, now)?.ok_or(StoreError::SpaceNotFound(space_id))?;
        tx.commit()?;
        Ok((lease, space))
    }

    /// Create an empty Space owned by the current Workspace identity. It is
    /// initially unleased; callers switch their current lease to it with
    /// `switch_space` after persisting the new summary.
    pub fn create_space(
        &self,
        lease: &WorkspaceLease,
        id: SpaceId,
        name: &str,
        identity: &Value,
        now: i64,
    ) -> StoreResult<StoredSpace> {
        let mut connection = self.connection();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_lease(&tx, lease, now)?;
        let sequence = next_counter(&tx, META_USAGE_SEQUENCE)?;
        tx.execute(
            "INSERT INTO spaces(space_id, name, identity_json, revision, last_used_sequence) \
             VALUES (?1, ?2, ?3, 0, ?4)",
            params![
                id.to_string(),
                name,
                serde_json::to_string(identity)?,
                sequence as i64
            ],
        )?;
        let space = read_space(&tx, id, now)?.ok_or(StoreError::SpaceNotFound(id))?;
        tx.commit()?;
        Ok(space)
    }

    /// Switch the current lease to `target`, fencing the old generation in
    /// the same transaction as claiming the new Space.
    pub fn switch_space(
        &self,
        lease: &WorkspaceLease,
        target: SpaceId,
        now: i64,
        lease_ttl: Duration,
    ) -> StoreResult<(WorkspaceLease, StoredSpace)> {
        let mut connection = self.connection();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_lease(&tx, lease, now)?;
        if read_space(&tx, target, now)?.is_none() {
            return Err(StoreError::SpaceNotFound(target));
        }
        let generation = next_counter(&tx, META_LEASE_GENERATION)?;
        let expires_at = now.saturating_add(duration_seconds(lease_ttl));
        tx.execute(
            "UPDATE leases SET space_id = ?1, generation = ?2, expires_at = ?3 \
             WHERE instance_id = ?4 AND space_id = ?5 AND generation = ?6",
            params![
                target.to_string(),
                generation as i64,
                expires_at,
                lease.instance_id.to_string(),
                lease.space_id.to_string(),
                lease.generation as i64,
            ],
        )?;
        advance_usage(&tx, target)?;
        let next_lease = WorkspaceLease {
            instance_id: lease.instance_id,
            space_id: target,
            generation,
            expires_at,
        };
        let space = read_space(&tx, target, now)?.ok_or(StoreError::SpaceNotFound(target))?;
        tx.commit()?;
        Ok((next_lease, space))
    }

    /// Extend a live lease without changing its fencing generation.
    pub fn renew(
        &self,
        lease: &WorkspaceLease,
        now: i64,
        lease_ttl: Duration,
    ) -> StoreResult<WorkspaceLease> {
        let mut connection = self.connection();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_lease(&tx, lease, now)?;
        let expires_at = now.saturating_add(duration_seconds(lease_ttl));
        tx.execute(
            "UPDATE leases SET expires_at = ?1 WHERE instance_id = ?2 \
             AND space_id = ?3 AND generation = ?4",
            params![
                expires_at,
                lease.instance_id.to_string(),
                lease.space_id.to_string(),
                lease.generation as i64,
            ],
        )?;
        tx.commit()?;
        Ok(WorkspaceLease {
            expires_at,
            ..lease.clone()
        })
    }

    /// Record an active-Space selection and renew the current lease. This
    /// advances recency under the same write lock used for claims.
    pub fn mark_used(
        &self,
        lease: &WorkspaceLease,
        now: i64,
        lease_ttl: Duration,
    ) -> StoreResult<WorkspaceLease> {
        let mut connection = self.connection();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_lease(&tx, lease, now)?;
        let expires_at = now.saturating_add(duration_seconds(lease_ttl));
        tx.execute(
            "UPDATE leases SET expires_at = ?1 WHERE instance_id = ?2 \
             AND space_id = ?3 AND generation = ?4",
            params![
                expires_at,
                lease.instance_id.to_string(),
                lease.space_id.to_string(),
                lease.generation as i64,
            ],
        )?;
        advance_usage(&tx, lease.space_id)?;
        tx.commit()?;
        Ok(WorkspaceLease {
            expires_at,
            ..lease.clone()
        })
    }

    /// Release only the exact lease generation. When restore is disabled, the
    /// final release atomically purges shared Workspace structures and refs.
    pub fn release(
        &self,
        lease: &WorkspaceLease,
        now: i64,
        restore_enabled: bool,
    ) -> StoreResult<()> {
        let mut connection = self.connection();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let deleted = tx.execute(
            "DELETE FROM leases WHERE instance_id = ?1 AND space_id = ?2 AND generation = ?3",
            params![
                lease.instance_id.to_string(),
                lease.space_id.to_string(),
                lease.generation as i64,
            ],
        )?;
        if deleted == 0 {
            return Err(StoreError::LeaseLost);
        }
        reap_expired(&tx, now)?;
        if !restore_enabled && active_lease_count(&tx, now)? == 0 {
            purge_workspace_state(&tx)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Atomically commit one typed structural change at `expected_revision`.
    pub fn apply_change(
        &self,
        lease: &WorkspaceLease,
        expected_revision: u64,
        change: WorkspaceChange,
        now: i64,
    ) -> StoreResult<u64> {
        let mut connection = self.connection();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_lease(&tx, lease, now)?;
        let current_revision = tx
            .query_row(
                "SELECT revision FROM spaces WHERE space_id = ?1",
                [lease.space_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|value| value as u64)
            .ok_or(StoreError::SpaceNotFound(lease.space_id))?;
        if current_revision != expected_revision {
            return Err(StoreError::StaleRevision {
                expected: expected_revision,
                current: current_revision,
            });
        }

        apply_change_rows(&tx, lease.space_id, &change)?;
        let revision = current_revision
            .checked_add(1)
            .ok_or_else(|| StoreError::Invalid("Space revision exhausted".into()))?;
        let payload = serde_json::to_string(&change)?;
        tx.execute(
            "UPDATE spaces SET revision = ?1 WHERE space_id = ?2",
            params![revision as i64, lease.space_id.to_string()],
        )?;
        tx.execute(
            "INSERT INTO changes(space_id, revision, change_json, committed_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![lease.space_id.to_string(), revision as i64, payload, now],
        )?;
        tx.commit()?;
        Ok(revision)
    }

    /// Return a complete stable-ID Space snapshot.
    pub fn load_space(&self, space_id: SpaceId, now: i64) -> StoreResult<Option<StoredSpace>> {
        read_space(&self.connection(), space_id, now)
    }

    /// Load all persisted Spaces in the same stable order as the picker.
    pub fn load_spaces(&self, now: i64) -> StoreResult<Vec<StoredSpace>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT space_id FROM spaces ORDER BY last_used_sequence DESC, space_id ASC",
        )?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| {
                let id = parse_id(&id)?;
                read_space(&connection, id, now)?.ok_or(StoreError::SpaceNotFound(id))
            })
            .collect()
    }

    /// List Spaces ordered by persisted use recency, with active participant
    /// counts suitable for the explicit join surface.
    pub fn list_spaces(&self, now: i64) -> StoreResult<Vec<SpaceSummary>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT s.space_id, s.name, s.identity_json, s.last_used_sequence, \
                    (SELECT COUNT(*) FROM leases l WHERE l.space_id = s.space_id AND l.expires_at > ?1) \
                    , (SELECT COUNT(*) FROM tabs t WHERE t.space_id = s.space_id) \
             FROM spaces s ORDER BY s.last_used_sequence DESC, s.space_id ASC",
        )?;
        let rows = statement.query_map([now], |row| {
            let id: String = row.get(0)?;
            Ok((
                id,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (id, name, identity, sequence, participants, tab_count) = row?;
            Ok(SpaceSummary {
                id: parse_id(&id)?,
                name,
                identity: serde_json::from_str(&identity)?,
                last_used_sequence: sequence as u64,
                participant_count: participants as u32,
                tab_count: tab_count as u32,
            })
        })
        .collect()
    }

    /// Load committed typed changes after a client's revision. A missing log
    /// prefix or a future client revision requires a complete snapshot.
    pub fn changes_since(
        &self,
        space_id: SpaceId,
        after_revision: u64,
    ) -> StoreResult<ChangeBatch> {
        let connection = self.connection();
        let current: Option<i64> = connection
            .query_row(
                "SELECT revision FROM spaces WHERE space_id = ?1",
                [space_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(current) = current else {
            return Err(StoreError::SpaceNotFound(space_id));
        };
        let current_revision = current as u64;
        let first: Option<i64> = connection.query_row(
            "SELECT MIN(revision) FROM changes WHERE space_id = ?1",
            [space_id.to_string()],
            |row| row.get(0),
        )?;
        let requires_snapshot = after_revision > current_revision
            || first.is_some_and(|first| after_revision.saturating_add(1) < first as u64);
        if requires_snapshot {
            return Ok(ChangeBatch {
                current_revision,
                requires_snapshot: true,
                changes: Vec::new(),
            });
        }
        let mut statement = connection.prepare(
            "SELECT revision, change_json FROM changes \
             WHERE space_id = ?1 AND revision > ?2 ORDER BY revision ASC",
        )?;
        let rows = statement.query_map(
            params![space_id.to_string(), after_revision as i64],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )?;
        let mut changes = Vec::new();
        let mut expected = after_revision.saturating_add(1);
        for row in rows {
            let (revision, payload) = row?;
            if revision as u64 != expected {
                return Ok(ChangeBatch {
                    current_revision,
                    requires_snapshot: true,
                    changes: Vec::new(),
                });
            }
            changes.push(CommittedChange {
                space_id,
                revision: revision as u64,
                change: serde_json::from_str(&payload)?,
            });
            expected = expected.saturating_add(1);
        }
        Ok(ChangeBatch {
            current_revision,
            requires_snapshot: false,
            changes,
        })
    }

    /// Import an existing snapshot once. The active entry receives the highest
    /// bootstrap usage sequence; all other entries retain stored order. A
    /// caller retries safely after a crash because the import marker and rows
    /// commit together.
    pub fn import_legacy_once(
        &self,
        spaces: &[ImportedSpace],
        active_index: usize,
    ) -> StoreResult<bool> {
        let mut connection = self.connection();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let imported: Option<String> = tx
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                [META_LEGACY_IMPORTED],
                |row| row.get(0),
            )
            .optional()?;
        if imported.is_some() {
            tx.commit()?;
            return Ok(false);
        }
        if spaces.is_empty() {
            tx.execute(
                "INSERT INTO meta(key, value) VALUES (?1, '1') \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [META_LEGACY_IMPORTED],
            )?;
            tx.commit()?;
            return Ok(true);
        }
        if tx.query_row("SELECT COUNT(*) FROM spaces", [], |row| {
            row.get::<_, i64>(0)
        })? > 0
        {
            // A live Workspace store already has authority over the older
            // full-snapshot file. Mark the one-time check complete atomically
            // so enabling restore later cannot make bootstrap fail or replace
            // newer shared state with stale legacy data.
            tx.execute(
                "INSERT INTO meta(key, value) VALUES (?1, '1') \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [META_LEGACY_IMPORTED],
            )?;
            tx.commit()?;
            return Ok(false);
        }

        let actual_active = active_index.min(spaces.len() - 1);
        let mut migration_order: Vec<usize> = (0..spaces.len())
            .filter(|index| *index != actual_active)
            .collect();
        migration_order.push(actual_active);
        let mut usage_by_index = vec![0_u64; spaces.len()];
        for (sequence, index) in migration_order.into_iter().enumerate() {
            usage_by_index[index] = sequence as u64 + 1;
        }
        for (index, space) in spaces.iter().enumerate() {
            let identity = serde_json::to_string(&space.identity)?;
            tx.execute(
                "INSERT INTO spaces(space_id, name, identity_json, revision, last_used_sequence) \
                 VALUES (?1, ?2, ?3, 0, ?4)",
                params![
                    space.id.to_string(),
                    space.name,
                    identity,
                    usage_by_index[index] as i64,
                ],
            )?;
            for (position, tab) in space.tabs.iter().enumerate() {
                insert_tab(&tx, space.id, tab, position as u32)?;
            }
        }
        set_counter(&tx, META_USAGE_SEQUENCE, spaces.len() as u64)?;
        tx.execute(
            "INSERT INTO meta(key, value) VALUES (?1, '1') \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [META_LEGACY_IMPORTED],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// All distinct scrollback keys still referenced by shared tabs.
    pub fn referenced_scrollback_keys(&self) -> StoreResult<Vec<String>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT DISTINCT scrollback_key FROM scrollback_refs ORDER BY scrollback_key",
        )?;
        let keys = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(keys)
    }

    /// Issue a generation-fenced permit only when a key has no Workspace
    /// references and no live Detached host session owns it. Tombstoning is
    /// atomic with the reference check, so a stale tab write cannot revive it.
    pub fn issue_gc_permit(
        &self,
        key: &str,
        live_host_keys: &std::collections::HashSet<String>,
        now: i64,
        permit_ttl: Duration,
    ) -> StoreResult<Option<GcPermit>> {
        if live_host_keys.contains(key) {
            return Ok(None);
        }
        let mut connection = self.connection();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let refs: i64 = tx.query_row(
            "SELECT COUNT(*) FROM scrollback_refs WHERE scrollback_key = ?1",
            [key],
            |row| row.get(0),
        )?;
        if refs > 0 {
            tx.commit()?;
            return Ok(None);
        }
        let existing: Option<(i64, i64)> = tx
            .query_row(
                "SELECT generation, expires_at FROM gc_permits WHERE scrollback_key = ?1",
                [key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((generation, expires_at)) = existing {
            if expires_at > now {
                tx.commit()?;
                return Ok(Some(GcPermit {
                    key: key.to_string(),
                    generation: generation as u64,
                    expires_at,
                }));
            }
        }
        let generation = existing.map_or(1, |(generation, _)| generation as u64 + 1);
        let expires_at = now.saturating_add(duration_seconds(permit_ttl));
        tx.execute(
            "INSERT INTO gc_tombstones(scrollback_key, generation) VALUES (?1, ?2) \
             ON CONFLICT(scrollback_key) DO UPDATE SET generation = excluded.generation",
            params![key, generation as i64],
        )?;
        tx.execute(
            "INSERT INTO gc_permits(scrollback_key, generation, expires_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(scrollback_key) DO UPDATE SET generation = excluded.generation, expires_at = excluded.expires_at",
            params![key, generation as i64, expires_at],
        )?;
        tx.commit()?;
        Ok(Some(GcPermit {
            key: key.to_string(),
            generation,
            expires_at,
        }))
    }

    /// Release a failed or missing-key deletion for a later generation-fenced
    /// retry. The tombstone remains so retired keys cannot be reintroduced.
    pub fn release_gc_permit(&self, permit: &GcPermit) -> StoreResult<bool> {
        let connection = self.connection();
        let changed = connection.execute(
            "DELETE FROM gc_permits WHERE scrollback_key = ?1 AND generation = ?2",
            params![permit.key, permit.generation as i64],
        )?;
        Ok(changed > 0)
    }

    /// Confirm that the file was deleted. A stale cleaner cannot complete a
    /// newer permit generation.
    pub fn confirm_gc(&self, permit: &GcPermit) -> StoreResult<bool> {
        self.release_gc_permit(permit)
    }
}

fn initialize_schema(connection: &Connection) -> StoreResult<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::UnsupportedSchema(version));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let tx = connection.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta(\
             key TEXT PRIMARY KEY, value TEXT NOT NULL\
         );\
         CREATE TABLE IF NOT EXISTS spaces(\
             space_id TEXT PRIMARY KEY, name TEXT NOT NULL, identity_json TEXT NOT NULL,\
             revision INTEGER NOT NULL DEFAULT 0, last_used_sequence INTEGER NOT NULL DEFAULT 0\
         );\
         CREATE TABLE IF NOT EXISTS leases(\
             instance_id TEXT PRIMARY KEY, space_id TEXT NOT NULL REFERENCES spaces(space_id) ON DELETE CASCADE,\
             generation INTEGER NOT NULL, expires_at INTEGER NOT NULL\
         );\
         CREATE INDEX IF NOT EXISTS leases_by_space ON leases(space_id);\
         CREATE TABLE IF NOT EXISTS tabs(\
             space_id TEXT NOT NULL REFERENCES spaces(space_id) ON DELETE CASCADE,\
             tab_id TEXT NOT NULL, position INTEGER NOT NULL, payload_json TEXT NOT NULL,\
             PRIMARY KEY(space_id, tab_id)\
         );\
         CREATE INDEX IF NOT EXISTS tabs_by_order ON tabs(space_id, position);\
         CREATE TABLE IF NOT EXISTS scrollback_refs(\
             space_id TEXT NOT NULL, tab_id TEXT NOT NULL, scrollback_key TEXT NOT NULL,\
             PRIMARY KEY(space_id, tab_id, scrollback_key),\
             FOREIGN KEY(space_id, tab_id) REFERENCES tabs(space_id, tab_id) ON DELETE CASCADE\
         );\
         CREATE INDEX IF NOT EXISTS scrollback_by_key ON scrollback_refs(scrollback_key);\
         CREATE TABLE IF NOT EXISTS changes(\
             space_id TEXT NOT NULL REFERENCES spaces(space_id) ON DELETE CASCADE,\
             revision INTEGER NOT NULL, change_json TEXT NOT NULL, committed_at INTEGER NOT NULL,\
             PRIMARY KEY(space_id, revision)\
         );\
         CREATE TABLE IF NOT EXISTS gc_tombstones(\
             scrollback_key TEXT PRIMARY KEY, generation INTEGER NOT NULL\
         );\
         CREATE TABLE IF NOT EXISTS gc_permits(\
             scrollback_key TEXT PRIMARY KEY, generation INTEGER NOT NULL, expires_at INTEGER NOT NULL\
         );\
         INSERT OR IGNORE INTO meta(key, value) VALUES\
             ('lease_generation', '0'), ('space_usage_sequence', '0');\
         PRAGMA user_version = 1;",
    )?;
    tx.commit()?;
    Ok(())
}

fn duration_seconds(duration: Duration) -> i64 {
    i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
}

fn parse_id<T: FromStr<Err = uuid::Error>>(value: &str) -> StoreResult<T> {
    value
        .parse()
        .map_err(|error: uuid::Error| StoreError::Invalid(error.to_string()))
}

fn next_counter(tx: &Transaction<'_>, key: &str) -> StoreResult<u64> {
    let current: String = tx.query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
        row.get(0)
    })?;
    let next = current
        .parse::<u64>()
        .map_err(|error| StoreError::Invalid(error.to_string()))?
        .checked_add(1)
        .ok_or_else(|| StoreError::Invalid(format!("{key} exhausted")))?;
    set_counter(tx, key, next)?;
    Ok(next)
}

fn set_counter(tx: &Transaction<'_>, key: &str, value: u64) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO meta(key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value.to_string()],
    )?;
    Ok(())
}

fn active_lease_count(tx: &Transaction<'_>, now: i64) -> StoreResult<i64> {
    Ok(tx.query_row(
        "SELECT COUNT(*) FROM leases WHERE expires_at > ?1",
        [now],
        |row| row.get(0),
    )?)
}

fn reap_expired(tx: &Transaction<'_>, now: i64) -> StoreResult<()> {
    tx.execute("DELETE FROM leases WHERE expires_at <= ?1", [now])?;
    Ok(())
}

fn purge_workspace_state(tx: &Transaction<'_>) -> StoreResult<()> {
    tx.execute("DELETE FROM spaces", [])?;
    tx.execute("DELETE FROM changes", [])?;
    Ok(())
}

fn space_order(tx: &Transaction<'_>) -> StoreResult<Vec<SpaceId>> {
    let mut statement =
        tx.prepare("SELECT space_id FROM spaces ORDER BY last_used_sequence DESC, space_id ASC")?;
    let spaces = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|row| parse_id(&row?))
        .collect();
    spaces
}

fn find_unleased_space(tx: &Transaction<'_>, now: i64) -> StoreResult<Option<SpaceId>> {
    let value: Option<String> = tx
        .query_row(
            "SELECT s.space_id FROM spaces s WHERE NOT EXISTS (\
                 SELECT 1 FROM leases l WHERE l.space_id = s.space_id AND l.expires_at > ?1\
             ) ORDER BY s.last_used_sequence DESC, s.space_id ASC LIMIT 1",
            [now],
            |row| row.get(0),
        )
        .optional()?;
    value.map(|id| parse_id(&id)).transpose()
}

fn most_recent_identity(tx: &Transaction<'_>) -> StoreResult<(String, String)> {
    tx.query_row(
        "SELECT name, identity_json FROM spaces \
         ORDER BY last_used_sequence DESC, space_id ASC LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(StoreError::from)
}

fn advance_usage(tx: &Transaction<'_>, space_id: SpaceId) -> StoreResult<()> {
    let sequence = next_counter(tx, META_USAGE_SEQUENCE)?;
    let changed = tx.execute(
        "UPDATE spaces SET last_used_sequence = ?1 WHERE space_id = ?2",
        params![sequence as i64, space_id.to_string()],
    )?;
    if changed == 0 {
        return Err(StoreError::SpaceNotFound(space_id));
    }
    Ok(())
}

fn claim_lease(
    tx: &Transaction<'_>,
    instance_id: WorkspaceInstanceId,
    space_id: SpaceId,
    now: i64,
    lease_ttl: Duration,
) -> StoreResult<WorkspaceLease> {
    if !tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM spaces WHERE space_id = ?1)",
        [space_id.to_string()],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(StoreError::SpaceNotFound(space_id));
    }
    let generation = next_counter(tx, META_LEASE_GENERATION)?;
    let expires_at = now.saturating_add(duration_seconds(lease_ttl));
    tx.execute(
        "INSERT INTO leases(instance_id, space_id, generation, expires_at) \
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT(instance_id) DO UPDATE SET \
             space_id = excluded.space_id, generation = excluded.generation, expires_at = excluded.expires_at",
        params![
            instance_id.to_string(),
            space_id.to_string(),
            generation as i64,
            expires_at,
        ],
    )?;
    Ok(WorkspaceLease {
        instance_id,
        space_id,
        generation,
        expires_at,
    })
}

fn lease_for_instance(
    tx: &Transaction<'_>,
    instance_id: WorkspaceInstanceId,
) -> StoreResult<Option<WorkspaceLease>> {
    let value = tx
        .query_row(
            "SELECT space_id, generation, expires_at FROM leases WHERE instance_id = ?1",
            [instance_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    value
        .map(|(space, generation, expires_at)| {
            Ok(WorkspaceLease {
                instance_id,
                space_id: parse_id(&space)?,
                generation: generation as u64,
                expires_at,
            })
        })
        .transpose()
}

fn validate_lease(tx: &Transaction<'_>, lease: &WorkspaceLease, now: i64) -> StoreResult<()> {
    let current: Option<(String, i64, i64)> = tx
        .query_row(
            "SELECT space_id, generation, expires_at FROM leases WHERE instance_id = ?1",
            [lease.instance_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    if current.is_some_and(|(space, generation, expires)| {
        space == lease.space_id.to_string()
            && generation as u64 == lease.generation
            && expires > now
    }) {
        Ok(())
    } else {
        Err(StoreError::LeaseLost)
    }
}

fn read_space(
    connection: &Connection,
    space_id: SpaceId,
    now: i64,
) -> StoreResult<Option<StoredSpace>> {
    let head: Option<(String, String, i64, i64)> = connection
        .query_row(
            "SELECT name, identity_json, revision, last_used_sequence \
             FROM spaces WHERE space_id = ?1",
            [space_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((name, identity, revision, last_used_sequence)) = head else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT tab_id, payload_json FROM tabs WHERE space_id = ?1 ORDER BY position ASC, tab_id ASC",
    )?;
    let rows = statement.query_map([space_id.to_string()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut tabs = Vec::new();
    for row in rows {
        let (tab, payload): (String, String) = row?;
        let mut refs_statement = connection.prepare(
            "SELECT scrollback_key FROM scrollback_refs \
             WHERE space_id = ?1 AND tab_id = ?2 ORDER BY scrollback_key ASC",
        )?;
        let scrollback_keys = refs_statement
            .query_map(params![space_id.to_string(), &tab], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        tabs.push(StoredTab {
            id: parse_id(&tab)?,
            payload: serde_json::from_str(&payload)?,
            scrollback_keys,
        });
    }
    let participants = connection.query_row(
        "SELECT COUNT(*) FROM leases WHERE space_id = ?1 AND expires_at > ?2",
        params![space_id.to_string(), now],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(Some(StoredSpace {
        id: space_id,
        name,
        identity: serde_json::from_str(&identity)?,
        revision: revision as u64,
        last_used_sequence: last_used_sequence as u64,
        tabs,
        participant_count: participants as u32,
    }))
}

fn check_scrollback_tombstones(tx: &Transaction<'_>, tab: &StoredTab) -> StoreResult<()> {
    for key in &tab.scrollback_keys {
        let tombstoned: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM gc_tombstones WHERE scrollback_key = ?1)",
            [key],
            |row| row.get(0),
        )?;
        if tombstoned {
            return Err(StoreError::ScrollbackTombstoned(key.clone()));
        }
    }
    Ok(())
}

fn replace_tab_refs(tx: &Transaction<'_>, space_id: SpaceId, tab: &StoredTab) -> StoreResult<()> {
    check_scrollback_tombstones(tx, tab)?;
    tx.execute(
        "DELETE FROM scrollback_refs WHERE space_id = ?1 AND tab_id = ?2",
        params![space_id.to_string(), tab.id.to_string()],
    )?;
    for key in &tab.scrollback_keys {
        tx.execute(
            "INSERT INTO scrollback_refs(space_id, tab_id, scrollback_key) VALUES (?1, ?2, ?3)",
            params![space_id.to_string(), tab.id.to_string(), key],
        )?;
    }
    Ok(())
}

fn insert_tab(
    tx: &Transaction<'_>,
    space_id: SpaceId,
    tab: &StoredTab,
    position: u32,
) -> StoreResult<()> {
    check_scrollback_tombstones(tx, tab)?;
    let payload = serde_json::to_string(&tab.payload)?;
    tx.execute(
        "INSERT INTO tabs(space_id, tab_id, position, payload_json) VALUES (?1, ?2, ?3, ?4)",
        params![
            space_id.to_string(),
            tab.id.to_string(),
            position as i64,
            payload,
        ],
    )?;
    replace_tab_refs(tx, space_id, tab)?;
    Ok(())
}

fn apply_change_rows(
    tx: &Transaction<'_>,
    space_id: SpaceId,
    change: &WorkspaceChange,
) -> StoreResult<()> {
    match change {
        WorkspaceChange::AddTab { tab, position } => {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM tabs WHERE space_id = ?1 AND tab_id = ?2)",
                params![space_id.to_string(), tab.id.to_string()],
                |row| row.get(0),
            )?;
            if exists {
                return Err(StoreError::TabAlreadyExists(tab.id));
            }
            tx.execute(
                "UPDATE tabs SET position = position + 1 WHERE space_id = ?1 AND position >= ?2",
                params![space_id.to_string(), *position as i64],
            )?;
            insert_tab(tx, space_id, tab, *position)?;
        }
        WorkspaceChange::RemoveTab { tab_id } => {
            let position: Option<i64> = tx
                .query_row(
                    "SELECT position FROM tabs WHERE space_id = ?1 AND tab_id = ?2",
                    params![space_id.to_string(), tab_id.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(position) = position else {
                return Err(StoreError::TabNotFound(*tab_id));
            };
            tx.execute(
                "DELETE FROM tabs WHERE space_id = ?1 AND tab_id = ?2",
                params![space_id.to_string(), tab_id.to_string()],
            )?;
            tx.execute(
                "UPDATE tabs SET position = position - 1 WHERE space_id = ?1 AND position > ?2",
                params![space_id.to_string(), position],
            )?;
        }
        WorkspaceChange::MoveTab { tab_id, position } => {
            let old: Option<i64> = tx
                .query_row(
                    "SELECT position FROM tabs WHERE space_id = ?1 AND tab_id = ?2",
                    params![space_id.to_string(), tab_id.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(old) = old else {
                return Err(StoreError::TabNotFound(*tab_id));
            };
            let count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM tabs WHERE space_id = ?1",
                [space_id.to_string()],
                |row| row.get(0),
            )?;
            let target = i64::from(*position).min(count.saturating_sub(1));
            if old < target {
                tx.execute(
                    "UPDATE tabs SET position = position - 1 WHERE space_id = ?1 AND position > ?2 AND position <= ?3",
                    params![space_id.to_string(), old, target],
                )?;
            } else if old > target {
                tx.execute(
                    "UPDATE tabs SET position = position + 1 WHERE space_id = ?1 AND position >= ?2 AND position < ?3",
                    params![space_id.to_string(), target, old],
                )?;
            }
            tx.execute(
                "UPDATE tabs SET position = ?1 WHERE space_id = ?2 AND tab_id = ?3",
                params![target, space_id.to_string(), tab_id.to_string()],
            )?;
        }
        WorkspaceChange::UpdateTab { tab } => {
            check_scrollback_tombstones(tx, tab)?;
            let payload = serde_json::to_string(&tab.payload)?;
            let changed = tx.execute(
                "UPDATE tabs SET payload_json = ?1 WHERE space_id = ?2 AND tab_id = ?3",
                params![payload, space_id.to_string(), tab.id.to_string()],
            )?;
            if changed == 0 {
                return Err(StoreError::TabNotFound(tab.id));
            }
            replace_tab_refs(tx, space_id, tab)?;
        }
        WorkspaceChange::RenameSpace { name } => {
            let changed = tx.execute(
                "UPDATE spaces SET name = ?1 WHERE space_id = ?2",
                params![name, space_id.to_string()],
            )?;
            if changed == 0 {
                return Err(StoreError::SpaceNotFound(space_id));
            }
        }
        WorkspaceChange::SetIdentity { identity } => {
            let changed = tx.execute(
                "UPDATE spaces SET identity_json = ?1 WHERE space_id = ?2",
                params![serde_json::to_string(identity)?, space_id.to_string()],
            )?;
            if changed == 0 {
                return Err(StoreError::SpaceNotFound(space_id));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    const NOW: i64 = 1_800_000_000;
    const TTL: Duration = Duration::from_secs(30);

    fn tab(name: &str) -> StoredTab {
        StoredTab {
            id: TabId::new(),
            payload: serde_json::json!({"name": name, "pane": PaneId::new()}),
            scrollback_keys: Vec::new(),
        }
    }

    fn temp_db() -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!("labonair-workspace-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        directory.join("workspace.db")
    }

    #[test]
    fn startup_claims_are_atomic_and_create_distinct_empty_spaces_when_needed() {
        let store = WorkspaceStorage::in_memory().unwrap();
        let first_id = WorkspaceInstanceId::new();
        let second_id = WorkspaceInstanceId::new();
        let (first, first_space) = store.claim_startup(first_id, NOW, TTL, true).unwrap();
        assert_eq!(first_space.name, "Default");
        assert_eq!(
            first_space.identity,
            serde_json::json!({"kind":"standalone"})
        );
        assert!(first_space.tabs.is_empty());
        let (second, second_space) = store.claim_startup(second_id, NOW, TTL, true).unwrap();
        assert_ne!(first.space_id, second.space_id);
        assert_ne!(first_space.id, second_space.id);
        assert!(second_space.tabs.is_empty());
        assert_eq!(store.list_spaces(NOW).unwrap().len(), 2);
    }

    #[test]
    fn simultaneous_startups_on_separate_connections_claim_distinct_spaces() {
        let path = temp_db();
        let seed = WorkspaceStorage::open(&path).unwrap();
        drop(seed);
        let gate = Arc::new(Barrier::new(3));
        let workers = (0..2)
            .map(|_| {
                let path = path.clone();
                let gate = Arc::clone(&gate);
                thread::spawn(move || {
                    let store = WorkspaceStorage::open(path).unwrap();
                    gate.wait();
                    store
                        .claim_startup(WorkspaceInstanceId::new(), NOW, TTL, true)
                        .unwrap()
                        .0
                        .space_id
                })
            })
            .collect::<Vec<_>>();
        gate.wait();
        let first = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_ne!(first[0], first[1]);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("db-wal"));
        let _ = fs::remove_file(path.with_extension("db-shm"));
        let _ = fs::remove_dir(path.parent().unwrap());
        let _ = fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn explicit_join_shares_a_space_and_changes_are_revisioned() {
        let store = WorkspaceStorage::in_memory().unwrap();
        let (lease_a, space) = store
            .claim_startup(WorkspaceInstanceId::new(), NOW, TTL, true)
            .unwrap();
        let (lease_b, joined) = store
            .join_space(WorkspaceInstanceId::new(), space.id, NOW, TTL)
            .unwrap();
        assert_eq!(joined.participant_count, 2);

        let new_tab = tab("terminal");
        let revision = store
            .apply_change(
                &lease_a,
                0,
                WorkspaceChange::AddTab {
                    tab: new_tab.clone(),
                    position: 0,
                },
                NOW,
            )
            .unwrap();
        assert_eq!(revision, 1);
        assert!(matches!(
            store.apply_change(
                &lease_b,
                0,
                WorkspaceChange::RenameSpace {
                    name: "lost".into()
                },
                NOW,
            ),
            Err(StoreError::StaleRevision { current: 1, .. })
        ));
        let batch = store.changes_since(space.id, 0).unwrap();
        assert!(!batch.requires_snapshot);
        assert_eq!(batch.current_revision, 1);
        assert_eq!(
            batch.changes[0].change,
            WorkspaceChange::AddTab {
                tab: new_tab,
                position: 0
            }
        );
    }

    #[test]
    fn stale_lease_generations_cannot_mutate_or_release_a_later_claim() {
        let store = WorkspaceStorage::in_memory().unwrap();
        let instance = WorkspaceInstanceId::new();
        let (old, _) = store.claim_startup(instance, NOW, TTL, true).unwrap();
        let (new, _) = store
            .join_space(instance, old.space_id, NOW + 1, TTL)
            .unwrap();
        assert_ne!(old.generation, new.generation);
        assert!(matches!(
            store.apply_change(
                &old,
                0,
                WorkspaceChange::RenameSpace {
                    name: "stale".into()
                },
                NOW + 1,
            ),
            Err(StoreError::LeaseLost)
        ));
        assert!(matches!(
            store.release(&old, NOW + 1, true),
            Err(StoreError::LeaseLost)
        ));
        store.release(&new, NOW + 1, true).unwrap();
    }

    #[test]
    fn session_restore_false_keeps_live_state_and_purges_after_final_release() {
        let store = WorkspaceStorage::in_memory().unwrap();
        let (first, space) = store
            .claim_startup(WorkspaceInstanceId::new(), NOW, TTL, false)
            .unwrap();
        let (second, _) = store
            .join_space(WorkspaceInstanceId::new(), space.id, NOW, TTL)
            .unwrap();
        store
            .apply_change(
                &first,
                0,
                WorkspaceChange::AddTab {
                    tab: tab("live"),
                    position: 0,
                },
                NOW,
            )
            .unwrap();
        store.release(&first, NOW + 1, false).unwrap();
        assert_eq!(
            store
                .load_space(space.id, NOW + 1)
                .unwrap()
                .unwrap()
                .tabs
                .len(),
            1
        );
        store.release(&second, NOW + 2, false).unwrap();
        assert!(store.load_space(space.id, NOW + 2).unwrap().is_none());
        let (_, fresh) = store
            .claim_startup(WorkspaceInstanceId::new(), NOW + 3, TTL, false)
            .unwrap();
        assert_eq!(fresh.name, "Default");
        assert!(fresh.tabs.is_empty());
    }

    #[test]
    fn expired_lease_is_reaped_and_reused_without_a_stale_release() {
        let store = WorkspaceStorage::in_memory().unwrap();
        let (expired, space) = store
            .claim_startup(
                WorkspaceInstanceId::new(),
                NOW,
                Duration::from_secs(1),
                true,
            )
            .unwrap();
        let (replacement, reused) = store
            .claim_startup(WorkspaceInstanceId::new(), NOW + 2, TTL, true)
            .unwrap();
        assert_eq!(replacement.space_id, expired.space_id);
        assert_eq!(reused.id, space.id);
        assert!(matches!(
            store.release(&expired, NOW + 2, true),
            Err(StoreError::LeaseLost)
        ));
    }

    #[test]
    fn legacy_import_is_idempotent_and_keeps_active_entry_most_recent() {
        let store = WorkspaceStorage::in_memory().unwrap();
        let imported = vec![
            ImportedSpace {
                id: SpaceId::new(),
                name: "One".into(),
                identity: serde_json::json!({"kind":"project","root":"/one"}),
                tabs: vec![tab("one")],
            },
            ImportedSpace {
                id: SpaceId::new(),
                name: "Two".into(),
                identity: serde_json::json!({"kind":"standalone"}),
                tabs: Vec::new(),
            },
        ];
        assert!(store.import_legacy_once(&imported, 0).unwrap());
        assert!(!store.import_legacy_once(&[], 0).unwrap());
        let spaces = store.list_spaces(NOW).unwrap();
        assert_eq!(spaces[0].id, imported[0].id);
        assert!(spaces[0].last_used_sequence > spaces[1].last_used_sequence);
        assert_eq!(
            store
                .load_space(imported[0].id, NOW)
                .unwrap()
                .unwrap()
                .tabs
                .len(),
            1
        );
    }

    #[test]
    fn legacy_import_is_skipped_once_live_workspace_rows_exist() {
        let store = WorkspaceStorage::in_memory().unwrap();
        let (_, current) = store
            .claim_startup(WorkspaceInstanceId::new(), NOW, TTL, true)
            .unwrap();
        let legacy = vec![ImportedSpace {
            id: SpaceId::new(),
            name: "stale snapshot".into(),
            identity: serde_json::json!({"kind":"standalone"}),
            tabs: vec![tab("legacy")],
        }];

        assert!(!store.import_legacy_once(&legacy, 0).unwrap());
        let spaces = store.list_spaces(NOW).unwrap();
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].id, current.id);
        assert_eq!(spaces[0].name, "Default");
        assert!(!store.import_legacy_once(&legacy, 0).unwrap());
    }

    #[test]
    fn scrollback_gc_requires_no_refs_and_protects_live_host_keys() {
        let store = WorkspaceStorage::in_memory().unwrap();
        let (lease, space) = store
            .claim_startup(WorkspaceInstanceId::new(), NOW, TTL, true)
            .unwrap();
        let mut referenced = tab("terminal");
        referenced.scrollback_keys.push("session-a".into());
        store
            .apply_change(
                &lease,
                0,
                WorkspaceChange::AddTab {
                    tab: referenced,
                    position: 0,
                },
                NOW,
            )
            .unwrap();
        let live = ["session-a".to_string()].into_iter().collect();
        assert!(store
            .issue_gc_permit("session-a", &live, NOW, TTL)
            .unwrap()
            .is_none());
        store
            .apply_change(
                &lease,
                1,
                WorkspaceChange::RemoveTab {
                    tab_id: store.load_space(space.id, NOW).unwrap().unwrap().tabs[0].id,
                },
                NOW,
            )
            .unwrap();
        let no_live = std::collections::HashSet::new();
        let permit = store
            .issue_gc_permit("session-a", &no_live, NOW, TTL)
            .unwrap()
            .unwrap();
        assert!(matches!(
            store.apply_change(
                &lease,
                2,
                WorkspaceChange::AddTab {
                    tab: StoredTab {
                        id: TabId::new(),
                        payload: serde_json::json!({"kind":"terminal"}),
                        scrollback_keys: vec!["session-a".into()],
                    },
                    position: 0,
                },
                NOW,
            ),
            Err(StoreError::ScrollbackTombstoned(_))
        ));
        assert!(store.confirm_gc(&permit).unwrap());
        assert!(!store.confirm_gc(&permit).unwrap());
    }
}

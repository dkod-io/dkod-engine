//! In-memory file overlay with DashMap and async PostgreSQL sync.
//!
//! Every write is reflected in a `DashMap` for O(1) reads and is
//! simultaneously persisted to the `session_overlay_files` table so that
//! workspaces survive process restarts.

use dashmap::DashMap;
use dk_core::Result;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

// ── Overlay entry ────────────────────────────────────────────────────

/// Represents a single file change within a session overlay.
#[derive(Debug, Clone)]
pub enum OverlayEntry {
    /// File was modified (or newly created with content from the session).
    Modified { content: Vec<u8>, hash: String },
    /// File was added (did not exist in the base commit).
    Added { content: Vec<u8>, hash: String },
    /// File was deleted from the base tree.
    Deleted,
}

impl OverlayEntry {
    /// Return the content bytes if this entry carries data.
    pub fn content(&self) -> Option<&[u8]> {
        match self {
            Self::Modified { content, .. } | Self::Added { content, .. } => Some(content),
            Self::Deleted => None,
        }
    }

    /// Return the content hash, or `None` for deletions.
    pub fn hash(&self) -> Option<&str> {
        match self {
            Self::Modified { hash, .. } | Self::Added { hash, .. } => Some(hash),
            Self::Deleted => None,
        }
    }

    /// The SQL `change_type` label.
    fn change_type_str(&self) -> &'static str {
        match self {
            Self::Modified { .. } => "modified",
            Self::Added { .. } => "added",
            Self::Deleted => "deleted",
        }
    }
}

// ── FileOverlay ──────────────────────────────────────────────────────

/// Concurrent, overlay-based file store for a single workspace.
///
/// Reads are lock-free via `DashMap`. Writes are O(1) in memory and
/// issue a single `INSERT … ON CONFLICT UPDATE` to PostgreSQL.
pub struct FileOverlay {
    entries: DashMap<String, OverlayEntry>,
    workspace_id: Uuid,
    db: PgPool,
}

impl FileOverlay {
    /// Create a new, empty overlay for the given workspace.
    pub fn new(workspace_id: Uuid, db: PgPool) -> Self {
        Self {
            entries: DashMap::new(),
            workspace_id,
            db,
        }
    }

    /// Get a reference to an overlay entry by path.
    pub fn get(&self, path: &str) -> Option<dashmap::mapref::one::Ref<'_, String, OverlayEntry>> {
        self.entries.get(path)
    }

    /// Check whether the overlay contains an entry for `path`.
    pub fn contains(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }

    /// Write (or overwrite) a file in the overlay.
    ///
    /// `is_new` indicates whether the file did not previously exist in the
    /// base tree — it controls whether the entry is `Added` vs `Modified`.
    ///
    /// The write is persisted to the database before returning.
    pub async fn write(&self, path: &str, content: Vec<u8>, is_new: bool) -> Result<String> {
        let hash = format!("{:x}", Sha256::digest(&content));

        let entry = if is_new {
            OverlayEntry::Added {
                content: content.clone(),
                hash: hash.clone(),
            }
        } else {
            OverlayEntry::Modified {
                content: content.clone(),
                hash: hash.clone(),
            }
        };

        let change_type = entry.change_type_str();

        // Persist to DB
        sqlx::query(
            r#"
            INSERT INTO session_overlay_files (workspace_id, file_path, content, content_hash, change_type)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (workspace_id, file_path) DO UPDATE
                SET content      = EXCLUDED.content,
                    content_hash = EXCLUDED.content_hash,
                    change_type  = EXCLUDED.change_type,
                    updated_at   = NOW()
            "#,
        )
        .bind(self.workspace_id)
        .bind(path)
        .bind(&content)
        .bind(&hash)
        .bind(change_type)
        .execute(&self.db)
        .await?;

        self.entries.insert(path.to_string(), entry);
        Ok(hash)
    }

    /// Mark a file as deleted in the overlay.
    ///
    /// The deletion is persisted to the database.
    pub async fn delete(&self, path: &str) -> Result<()> {
        let entry = OverlayEntry::Deleted;
        let change_type = entry.change_type_str();

        sqlx::query(
            r#"
            INSERT INTO session_overlay_files (workspace_id, file_path, content, content_hash, change_type)
            VALUES ($1, $2, '', '', $3)
            ON CONFLICT (workspace_id, file_path) DO UPDATE
                SET content      = EXCLUDED.content,
                    content_hash = EXCLUDED.content_hash,
                    change_type  = EXCLUDED.change_type,
                    updated_at   = NOW()
            "#,
        )
        .bind(self.workspace_id)
        .bind(path)
        .bind(change_type)
        .execute(&self.db)
        .await?;

        self.entries.insert(path.to_string(), entry);
        Ok(())
    }

    /// Revert a file in the overlay, removing it from both memory and DB.
    pub async fn revert(&self, path: &str) -> Result<()> {
        self.entries.remove(path);

        sqlx::query("DELETE FROM session_overlay_files WHERE workspace_id = $1 AND file_path = $2")
            .bind(self.workspace_id)
            .bind(path)
            .execute(&self.db)
            .await?;

        Ok(())
    }

    /// Return a snapshot of all changed paths and their entries.
    pub fn list_changes(&self) -> Vec<(String, OverlayEntry)> {
        self.entries
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect()
    }

    /// Returns just the file paths in the overlay without cloning content.
    pub fn list_paths(&self) -> Vec<String> {
        self.entries.iter().map(|r| r.key().clone()).collect()
    }

    /// Number of entries (files touched) in the overlay.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the overlay is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total bytes stored in the overlay (excluding deleted entries).
    pub fn total_bytes(&self) -> usize {
        self.entries
            .iter()
            .filter_map(|r| r.value().content().map(|c| c.len()))
            .sum()
    }

    /// Restore overlay state from the database.
    ///
    /// Used when recovering a workspace after a process restart.
    pub async fn restore_from_db(&self) -> Result<()> {
        let rows: Vec<(String, Vec<u8>, String, String)> = sqlx::query_as(
            r#"
            SELECT file_path, content, content_hash, change_type
            FROM session_overlay_files
            WHERE workspace_id = $1
            "#,
        )
        .bind(self.workspace_id)
        .fetch_all(&self.db)
        .await?;

        for (path, content, hash, change_type) in rows {
            let entry = match change_type.as_str() {
                "added" => OverlayEntry::Added { content, hash },
                "deleted" => OverlayEntry::Deleted,
                _ => OverlayEntry::Modified { content, hash },
            };
            self.entries.insert(path, entry);
        }

        Ok(())
    }
}

// ── Test helpers ─────────────────────────────────────────────────────

impl FileOverlay {
    /// Create an overlay backed only by an in-memory DashMap (no DB).
    ///
    /// Intended for unit/integration tests that do not have a PostgreSQL
    /// connection. Writes via [`write_local`] go straight to the DashMap.
    #[doc(hidden)]
    pub fn new_inmemory(workspace_id: Uuid) -> Self {
        // Build a PgPool that will never actually connect.  We use
        // connect_lazy with a dummy DSN — it only errors when a query
        // is executed, and write_local never touches the pool.
        let opts = sqlx::postgres::PgConnectOptions::new()
            .host("__nsi_test_dummy__")
            .port(1);
        let pool = sqlx::PgPool::connect_lazy_with(opts);
        Self {
            entries: DashMap::new(),
            workspace_id,
            db: pool,
        }
    }

    /// Write a file to the in-memory overlay WITHOUT touching the database.
    ///
    /// This is the test-friendly counterpart of [`write`].
    #[doc(hidden)]
    pub fn write_local(&self, path: &str, content: Vec<u8>, is_new: bool) -> String {
        let hash = format!("{:x}", Sha256::digest(&content));

        let entry = if is_new {
            OverlayEntry::Added {
                content,
                hash: hash.clone(),
            }
        } else {
            OverlayEntry::Modified {
                content,
                hash: hash.clone(),
            }
        };

        self.entries.insert(path.to_string(), entry);
        hash
    }

    /// Mark a file as deleted in the in-memory overlay WITHOUT touching DB.
    #[doc(hidden)]
    pub fn delete_local(&self, path: &str) {
        self.entries.insert(path.to_string(), OverlayEntry::Deleted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_entry_content_and_hash() {
        let entry = OverlayEntry::Modified {
            content: b"hello".to_vec(),
            hash: "abc".into(),
        };
        assert_eq!(entry.content(), Some(b"hello".as_slice()));
        assert_eq!(entry.hash(), Some("abc"));

        let deleted = OverlayEntry::Deleted;
        assert!(deleted.content().is_none());
        assert!(deleted.hash().is_none());
    }

    #[test]
    fn overlay_entry_change_type() {
        assert_eq!(
            OverlayEntry::Modified {
                content: vec![],
                hash: String::new()
            }
            .change_type_str(),
            "modified"
        );
        assert_eq!(
            OverlayEntry::Added {
                content: vec![],
                hash: String::new()
            }
            .change_type_str(),
            "added"
        );
        assert_eq!(OverlayEntry::Deleted.change_type_str(), "deleted");
    }
}

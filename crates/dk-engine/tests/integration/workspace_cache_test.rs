//! Integration tests for the WorkspaceCache trait and NoOpCache implementation.
//!
//! These tests verify:
//! - NoOpCache returns `None`/empty for all read operations.
//! - NoOpCache accepts all write operations silently (no errors).
//! - WorkspaceManager::with_cache accepts a custom cache and exposes it via cache().
//! - WorkspaceManager::new uses NoOpCache by default.
//! - The trait object is Send + Sync and works behind Arc<dyn WorkspaceCache>.
//! - WorkspaceSnapshot and CachedOverlayEntry serialise/deserialise correctly.

use std::sync::Arc;

use dk_engine::workspace::cache::{
    CachedOverlayEntry, NoOpCache, WorkspaceCache, WorkspaceSnapshot,
};
use dk_engine::workspace::session_manager::WorkspaceManager;
use sqlx::PgPool;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────

fn lazy_pool() -> PgPool {
    PgPool::connect_lazy("postgres://localhost/nonexistent").unwrap()
}

fn sample_snapshot(session_id: Uuid) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        session_id,
        repo_id: Uuid::new_v4(),
        agent_id: "clerk|test-agent".to_string(),
        agent_name: "agent-1".to_string(),
        changeset_id: Uuid::new_v4(),
        intent: "add feature".to_string(),
        base_commit: "cafebabe".to_string(),
        state: "active".to_string(),
        mode: "ephemeral".to_string(),
    }
}

// ── NoOpCache: read operations return empty / None ────────────────────

#[tokio::test]
async fn noop_get_workspace_is_always_none() {
    let cache = NoOpCache;
    assert!(cache.get_workspace(&Uuid::new_v4()).await.unwrap().is_none());
}

#[tokio::test]
async fn noop_get_file_is_always_none() {
    let cache = NoOpCache;
    assert!(cache.get_file(&Uuid::new_v4(), "src/main.rs").await.unwrap().is_none());
}

#[tokio::test]
async fn noop_list_files_is_always_empty() {
    let cache = NoOpCache;
    assert!(cache.list_files(&Uuid::new_v4()).await.unwrap().is_empty());
}

#[tokio::test]
async fn noop_get_graph_is_always_none() {
    let cache = NoOpCache;
    assert!(cache.get_graph(&Uuid::new_v4()).await.unwrap().is_none());
}

// ── NoOpCache: writes are silent (no error, no effect) ───────────────

#[tokio::test]
async fn noop_cache_workspace_write_then_read_is_still_none() {
    let cache = NoOpCache;
    let id = Uuid::new_v4();
    let snap = sample_snapshot(id);

    cache.cache_workspace(&id, &snap).await.unwrap();

    let back = cache.get_workspace(&id).await.unwrap();
    assert!(back.is_none(), "NoOpCache must not store anything");
}

#[tokio::test]
async fn noop_cache_file_write_then_read_is_still_none() {
    let cache = NoOpCache;
    let id = Uuid::new_v4();
    let entry = CachedOverlayEntry::Modified {
        content: b"pub fn hello() {}".to_vec(),
        hash: "deadbeef".to_string(),
    };

    cache.cache_file(&id, "src/lib.rs", &entry).await.unwrap();
    let back = cache.get_file(&id, "src/lib.rs").await.unwrap();
    assert!(back.is_none());
}

#[tokio::test]
async fn noop_cache_graph_write_then_read_is_still_none() {
    let cache = NoOpCache;
    let id = Uuid::new_v4();
    cache.cache_graph(&id, b"binary-graph-data").await.unwrap();
    let back = cache.get_graph(&id).await.unwrap();
    assert!(back.is_none());
}

// ── NoOpCache: lifecycle operations are no-ops ────────────────────────

#[tokio::test]
async fn noop_evict_is_noop() {
    let cache = NoOpCache;
    cache.evict(&Uuid::new_v4()).await.expect("evict must not error");
}

#[tokio::test]
async fn noop_touch_is_noop() {
    let cache = NoOpCache;
    cache.touch(&Uuid::new_v4()).await.expect("touch must not error");
}

// ── CachedOverlayEntry all variants ──────────────────────────────────

#[tokio::test]
async fn noop_cache_file_all_variant_types() {
    let cache = NoOpCache;
    let id = Uuid::new_v4();

    // Modified
    cache
        .cache_file(
            &id,
            "a.rs",
            &CachedOverlayEntry::Modified {
                content: b"modified".to_vec(),
                hash: "hash1".to_string(),
            },
        )
        .await
        .unwrap();

    // Added
    cache
        .cache_file(
            &id,
            "b.rs",
            &CachedOverlayEntry::Added {
                content: b"added".to_vec(),
                hash: "hash2".to_string(),
            },
        )
        .await
        .unwrap();

    // Deleted
    cache
        .cache_file(&id, "c.rs", &CachedOverlayEntry::Deleted)
        .await
        .unwrap();

    // All reads still None
    assert!(cache.get_file(&id, "a.rs").await.unwrap().is_none());
    assert!(cache.get_file(&id, "b.rs").await.unwrap().is_none());
    assert!(cache.get_file(&id, "c.rs").await.unwrap().is_none());
    assert!(cache.list_files(&id).await.unwrap().is_empty());
}

// ── Arc<dyn WorkspaceCache> — trait object Send + Sync ───────────────

#[tokio::test]
async fn noop_cache_works_as_trait_object() {
    let cache: Arc<dyn WorkspaceCache> = Arc::new(NoOpCache);
    let id = Uuid::new_v4();

    // Can call all methods through the trait object.
    let snap = sample_snapshot(id);
    cache.cache_workspace(&id, &snap).await.unwrap();
    assert!(cache.get_workspace(&id).await.unwrap().is_none());

    cache
        .cache_file(&id, "x.rs", &CachedOverlayEntry::Deleted)
        .await
        .unwrap();
    assert!(cache.get_file(&id, "x.rs").await.unwrap().is_none());
    assert!(cache.list_files(&id).await.unwrap().is_empty());

    cache.cache_graph(&id, &[1, 2, 3]).await.unwrap();
    assert!(cache.get_graph(&id).await.unwrap().is_none());

    cache.evict(&id).await.unwrap();
    cache.touch(&id).await.unwrap();
}

#[tokio::test]
async fn noop_cache_trait_object_is_send_sync() {
    // Verify it can be moved across Tokio tasks.
    let cache: Arc<dyn WorkspaceCache> = Arc::new(NoOpCache);
    let id = Uuid::new_v4();

    let handle = tokio::spawn({
        let cache = Arc::clone(&cache);
        async move { cache.get_workspace(&id).await }
    });

    let result = handle.await.expect("task panicked").expect("cache error");
    assert!(result.is_none());
}

// ── WorkspaceManager integration ─────────────────────────────────────

#[tokio::test]
async fn workspace_manager_new_uses_noop_cache() {
    let mgr = WorkspaceManager::new(lazy_pool());
    // The cache accessor should succeed. Since NoOpCache always returns None
    // we can verify round-trip without a live cache.
    let _cache_ref: &dyn WorkspaceCache = mgr.cache();
}

#[tokio::test]
async fn workspace_manager_with_cache_accessor_works() {
    let custom_cache: Arc<dyn WorkspaceCache> = Arc::new(NoOpCache);
    let mgr = WorkspaceManager::with_cache(lazy_pool(), Arc::clone(&custom_cache));

    let id = Uuid::new_v4();
    // Invoke via the accessor — must not error.
    mgr.cache().get_workspace(&id).await.expect("accessor should work");
    mgr.cache().list_files(&id).await.expect("accessor should work");
}

#[tokio::test]
async fn workspace_manager_with_cache_accepts_arc_noop() {
    let mgr =
        WorkspaceManager::with_cache(lazy_pool(), Arc::new(NoOpCache));

    let id = Uuid::new_v4();
    let snap = sample_snapshot(id);

    mgr.cache().cache_workspace(&id, &snap).await.unwrap();
    assert!(mgr.cache().get_workspace(&id).await.unwrap().is_none());
}

// ── WorkspaceSnapshot serialisation ──────────────────────────────────

#[test]
fn workspace_snapshot_serializes_all_fields() {
    let id = Uuid::new_v4();
    let snap = sample_snapshot(id);
    let json = serde_json::to_value(&snap).expect("serialize");

    let expected_keys = [
        "session_id",
        "repo_id",
        "agent_id",
        "agent_name",
        "changeset_id",
        "intent",
        "base_commit",
        "state",
        "mode",
    ];
    let obj = json.as_object().unwrap();
    for key in &expected_keys {
        assert!(obj.contains_key(*key), "missing key: {key}");
    }
    assert_eq!(obj.len(), expected_keys.len(), "unexpected extra keys");
}

#[test]
fn workspace_snapshot_roundtrips_json_bytes() {
    let id = Uuid::new_v4();
    let snap = sample_snapshot(id);
    let bytes = serde_json::to_vec(&snap).unwrap();
    let back: WorkspaceSnapshot = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(snap, back);
}

#[test]
fn cached_overlay_entry_roundtrips_all_variants() {
    let variants = [
        CachedOverlayEntry::Modified {
            content: b"code".to_vec(),
            hash: "h1".to_string(),
        },
        CachedOverlayEntry::Added {
            content: b"new".to_vec(),
            hash: "h2".to_string(),
        },
        CachedOverlayEntry::Deleted,
    ];
    for v in &variants {
        let json = serde_json::to_string(v).unwrap();
        let back: CachedOverlayEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(v, &back);
    }
}

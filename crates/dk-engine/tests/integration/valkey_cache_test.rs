//! Integration tests for `ValkeyCache` — the Valkey/Redis-backed
//! [`WorkspaceCache`] implementation.
//!
//! These tests require a running Redis/Valkey instance on `localhost:6379`.
//! They are guarded by `#[cfg(feature = "valkey")]` and run with:
//!
//! ```sh
//! cargo test -p dk-engine --features valkey --test valkey_cache_test
//! ```

#![cfg(feature = "valkey")]

use dk_engine::workspace::cache::{CachedOverlayEntry, WorkspaceCache, WorkspaceSnapshot};
use dk_engine::workspace::valkey_cache::ValkeyCache;
use uuid::Uuid;

const REDIS_URL: &str = "redis://127.0.0.1:6379";
const TTL_SECS: u32 = 60;

// ── Helpers ──────────────────────────────────────────────────────────

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

async fn make_cache() -> ValkeyCache {
    ValkeyCache::new(REDIS_URL, TTL_SECS)
        .await
        .expect("Redis must be running on localhost:6379 for integration tests")
}

// ── 1. Roundtrip workspace metadata ──────────────────────────────────

#[tokio::test]
async fn roundtrip_workspace_metadata() {
    let cache = make_cache().await;
    let id = Uuid::new_v4();
    let snap = sample_snapshot(id);

    cache.cache_workspace(&id, &snap).await.unwrap();

    let back = cache.get_workspace(&id).await.unwrap();
    assert_eq!(back.as_ref(), Some(&snap));

    // Cleanup
    cache.evict(&id).await.unwrap();
}

// ── 2. Roundtrip overlay file ────────────────────────────────────────

#[tokio::test]
async fn roundtrip_overlay_file() {
    let cache = make_cache().await;
    let ws_id = Uuid::new_v4();

    let entry = CachedOverlayEntry::Modified {
        content: b"pub fn hello() {}".to_vec(),
        hash: "deadbeef".to_string(),
    };

    cache
        .cache_file(&ws_id, "src/lib.rs", &entry)
        .await
        .unwrap();

    let back = cache.get_file(&ws_id, "src/lib.rs").await.unwrap();
    assert_eq!(back.as_ref(), Some(&entry));

    // Cleanup
    cache.evict(&ws_id).await.unwrap();
}

// ── 3. list_files returns cached paths ───────────────────────────────

#[tokio::test]
async fn list_files_returns_cached_paths() {
    let cache = make_cache().await;
    let ws_id = Uuid::new_v4();

    let entry = CachedOverlayEntry::Added {
        content: b"new file".to_vec(),
        hash: "abc123".to_string(),
    };

    cache
        .cache_file(&ws_id, "src/main.rs", &entry)
        .await
        .unwrap();
    cache
        .cache_file(&ws_id, "src/lib.rs", &entry)
        .await
        .unwrap();

    let mut files = cache.list_files(&ws_id).await.unwrap();
    files.sort();

    assert_eq!(files, vec!["src/lib.rs", "src/main.rs"]);

    // Cleanup
    cache.evict(&ws_id).await.unwrap();
}

// ── 4. Roundtrip graph data ──────────────────────────────────────────

#[tokio::test]
async fn roundtrip_graph_data() {
    let cache = make_cache().await;
    let ws_id = Uuid::new_v4();

    let graph_data = b"binary-graph-blob-12345";

    cache.cache_graph(&ws_id, graph_data).await.unwrap();

    let back = cache.get_graph(&ws_id).await.unwrap();
    assert_eq!(back.as_deref(), Some(graph_data.as_slice()));

    // Cleanup
    cache.evict(&ws_id).await.unwrap();
}

// ── 5. Evict removes all keys ────────────────────────────────────────

#[tokio::test]
async fn evict_removes_all_keys() {
    let cache = make_cache().await;
    let ws_id = Uuid::new_v4();

    // Populate all key types.
    let snap = sample_snapshot(ws_id);
    cache.cache_workspace(&ws_id, &snap).await.unwrap();
    cache.cache_graph(&ws_id, b"graph-data").await.unwrap();
    cache
        .cache_file(
            &ws_id,
            "src/a.rs",
            &CachedOverlayEntry::Modified {
                content: b"a".to_vec(),
                hash: "h1".to_string(),
            },
        )
        .await
        .unwrap();
    cache
        .cache_file(
            &ws_id,
            "src/b.rs",
            &CachedOverlayEntry::Added {
                content: b"b".to_vec(),
                hash: "h2".to_string(),
            },
        )
        .await
        .unwrap();

    // Verify populated.
    assert!(cache.get_workspace(&ws_id).await.unwrap().is_some());
    assert!(cache.get_graph(&ws_id).await.unwrap().is_some());
    assert_eq!(cache.list_files(&ws_id).await.unwrap().len(), 2);

    // Evict.
    cache.evict(&ws_id).await.unwrap();

    // Everything must be gone.
    assert!(cache.get_workspace(&ws_id).await.unwrap().is_none());
    assert!(cache.get_graph(&ws_id).await.unwrap().is_none());
    assert!(cache.list_files(&ws_id).await.unwrap().is_empty());
    assert!(cache.get_file(&ws_id, "src/a.rs").await.unwrap().is_none());
    assert!(cache.get_file(&ws_id, "src/b.rs").await.unwrap().is_none());
}

// ── 6. Cache miss returns None ───────────────────────────────────────

#[tokio::test]
async fn cache_miss_returns_none() {
    let cache = make_cache().await;
    let ws_id = Uuid::new_v4(); // Never written.

    assert!(cache.get_workspace(&ws_id).await.unwrap().is_none());
    assert!(cache
        .get_file(&ws_id, "nonexistent.rs")
        .await
        .unwrap()
        .is_none());
    assert!(cache.get_graph(&ws_id).await.unwrap().is_none());
    assert!(cache.list_files(&ws_id).await.unwrap().is_empty());
}

// ── 7. Multiple files in same workspace ──────────────────────────────

#[tokio::test]
async fn multiple_files_same_workspace() {
    let cache = make_cache().await;
    let ws_id = Uuid::new_v4();

    let entries = [
        (
            "src/main.rs",
            CachedOverlayEntry::Modified {
                content: b"fn main() {}".to_vec(),
                hash: "h1".to_string(),
            },
        ),
        (
            "src/lib.rs",
            CachedOverlayEntry::Added {
                content: b"pub mod foo;".to_vec(),
                hash: "h2".to_string(),
            },
        ),
        ("src/old.rs", CachedOverlayEntry::Deleted),
    ];

    for (path, entry) in &entries {
        cache.cache_file(&ws_id, path, entry).await.unwrap();
    }

    // Verify each file individually.
    for (path, entry) in &entries {
        let back = cache.get_file(&ws_id, path).await.unwrap();
        assert_eq!(back.as_ref(), Some(entry), "mismatch for {path}");
    }

    // list_files should have all three.
    let mut files = cache.list_files(&ws_id).await.unwrap();
    files.sort();
    assert_eq!(files, vec!["src/lib.rs", "src/main.rs", "src/old.rs"]);

    // Cleanup
    cache.evict(&ws_id).await.unwrap();
}

// ── 8. Touch does not error (smoke test) ─────────────────────────────

#[tokio::test]
async fn touch_refreshes_ttl_without_error() {
    let cache = make_cache().await;
    let ws_id = Uuid::new_v4();

    // Populate.
    let snap = sample_snapshot(ws_id);
    cache.cache_workspace(&ws_id, &snap).await.unwrap();
    cache.cache_graph(&ws_id, b"graph").await.unwrap();
    cache
        .cache_file(&ws_id, "src/a.rs", &CachedOverlayEntry::Deleted)
        .await
        .unwrap();

    // Touch must succeed without error.
    cache.touch(&ws_id).await.unwrap();

    // Data must still be present after touch.
    assert!(cache.get_workspace(&ws_id).await.unwrap().is_some());
    assert!(cache.get_graph(&ws_id).await.unwrap().is_some());
    assert!(cache.get_file(&ws_id, "src/a.rs").await.unwrap().is_some());

    // Cleanup
    cache.evict(&ws_id).await.unwrap();
}

// ── 9. ValkeyCache works as Arc<dyn WorkspaceCache> ──────────────────

#[tokio::test]
async fn valkey_cache_works_as_trait_object() {
    let cache: std::sync::Arc<dyn WorkspaceCache> = std::sync::Arc::new(make_cache().await);
    let ws_id = Uuid::new_v4();

    let snap = sample_snapshot(ws_id);
    cache.cache_workspace(&ws_id, &snap).await.unwrap();
    let back = cache.get_workspace(&ws_id).await.unwrap();
    assert_eq!(back.as_ref(), Some(&snap));

    cache.evict(&ws_id).await.unwrap();
}

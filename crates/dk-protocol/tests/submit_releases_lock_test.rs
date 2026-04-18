//! Tests for the release-locks-at-submit behavior.
//!
//! PR1 keeps the test surface at the `ClaimTracker + EventBus` layer rather
//! than the full `ProtocolServer` RPC stack — spinning up the server
//! requires Postgres + workspace manager + gRPC transport, which is PR2's
//! domain. The unit here validates the piece of logic the PR actually
//! changes: a session's submit-time lock release unblocks a waiter that
//! was holding on `symbol.lock.released`, and emits the event other
//! sessions would observe via `dk_watch`.

use chrono::Utc;
use dk_core::SymbolKind;
use dk_engine::conflict::{AcquireOutcome, ClaimTracker, LocalClaimTracker, SymbolClaim};
use dk_protocol::events::EventBus;
use dk_protocol::merge::EVENT_LOCK_RELEASED;
use uuid::Uuid;

fn claim(session_id: Uuid, agent: &str, name: &str) -> SymbolClaim {
    SymbolClaim {
        session_id,
        agent_name: agent.to_string(),
        qualified_name: name.to_string(),
        kind: SymbolKind::Function,
        first_touched_at: Utc::now(),
    }
}

/// Mirror of the production `release_locks_and_emit` logic — the helper
/// itself takes a `&ProtocolServer`, which is more infrastructure than
/// this unit needs. Publishing order, event shape, and the "idempotent
/// re-call returns 0" property are the three things we actually want to
/// pin down.
async fn release_and_emit(
    tracker: &dyn ClaimTracker,
    bus: &EventBus,
    repo_id: Uuid,
    session_id: Uuid,
    session_id_str: &str,
    changeset_id: &str,
) -> usize {
    let released = tracker.release_locks(repo_id, session_id).await;
    if released.is_empty() {
        return 0;
    }
    let count = released.len();

    let mut by_file: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for lock in &released {
        by_file
            .entry(lock.file_path.clone())
            .or_default()
            .push(lock.qualified_name.clone());
    }
    for (file_path, symbols) in by_file {
        bus.publish(dk_protocol::WatchEvent {
            event_type: EVENT_LOCK_RELEASED.to_string(),
            changeset_id: changeset_id.to_string(),
            agent_id: released
                .first()
                .map(|r| r.agent_name.clone())
                .unwrap_or_default(),
            affected_symbols: symbols,
            details: format!("Symbol locks released on {}", file_path),
            session_id: session_id_str.to_string(),
            affected_files: vec![dk_protocol::FileChange {
                path: file_path,
                operation: "unlock".to_string(),
            }],
            symbol_changes: vec![],
            repo_id: repo_id.to_string(),
            event_id: Uuid::new_v4().to_string(),
        });
    }
    count
}

#[tokio::test]
async fn session_a_submit_releases_locks_and_session_b_can_acquire() {
    let tracker: Box<dyn ClaimTracker> = Box::new(LocalClaimTracker::new());
    let bus = EventBus::new();
    let repo = Uuid::new_v4();
    let path = "src/aggregator.ts";
    let sym = "formatRelativeTime";

    let sid_a = Uuid::new_v4();
    let sid_b = Uuid::new_v4();

    // B subscribes to repo events before A acquires, mirroring the watcher
    // that goes into `dk_watch(filter: "symbol.lock.released", wait: true)`
    // after it sees SYMBOL_LOCKED.
    let mut rx = bus.subscribe(&repo.to_string());

    // A acquires the symbol lock (simulating dk_file_write).
    let outcome = tracker
        .acquire_lock(repo, path, claim(sid_a, "agent-a", sym))
        .await
        .expect("fresh acquire should succeed");
    assert_eq!(outcome, AcquireOutcome::Fresh);

    // B's attempt fails with SYMBOL_LOCKED — the existing contract.
    let locked = tracker
        .acquire_lock(repo, path, claim(sid_b, "agent-b", sym))
        .await
        .expect_err("B's acquire should be blocked while A holds the lock");
    assert_eq!(locked.qualified_name, sym);
    assert_eq!(locked.locked_by_session, sid_a);

    // A submits → new behavior under DKOD_RELEASE_ON_SUBMIT: locks release
    // now, event fires, B unblocks. (In PR1 the release site is also
    // guarded by the env flag in the real handler; this test validates
    // the behavior the flag enables, not the flag wiring itself.)
    let count = release_and_emit(&*tracker, &bus, repo, sid_a, &sid_a.to_string(), "cs-A").await;
    assert_eq!(count, 1, "one symbol lock released");

    // B must observe the event it was waiting on.
    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("event should arrive immediately after release")
        .expect("bus not closed");
    assert_eq!(event.event_type, EVENT_LOCK_RELEASED);
    assert_eq!(event.affected_symbols, vec![sym.to_string()]);
    assert_eq!(event.changeset_id, "cs-A");

    // B can now acquire the same lock without waiting for merge.
    let outcome = tracker
        .acquire_lock(repo, path, claim(sid_b, "agent-b", sym))
        .await
        .expect("B should acquire freshly after A's submit-time release");
    assert_eq!(outcome, AcquireOutcome::Fresh);
}

#[tokio::test]
async fn idempotent_second_release_returns_zero() {
    // The release site in `handle_merge` must stay safe to call after
    // `handle_submit` already ran — no double-emit, no crash, just a zero
    // count so the caller can skip logging.
    let tracker: Box<dyn ClaimTracker> = Box::new(LocalClaimTracker::new());
    let bus = EventBus::new();
    let repo = Uuid::new_v4();
    let sid = Uuid::new_v4();

    tracker
        .acquire_lock(repo, "f.rs", claim(sid, "a", "foo"))
        .await
        .unwrap();

    let first = release_and_emit(&*tracker, &bus, repo, sid, &sid.to_string(), "cs").await;
    assert_eq!(first, 1);

    let second = release_and_emit(&*tracker, &bus, repo, sid, &sid.to_string(), "cs").await;
    assert_eq!(second, 0, "second release is a no-op");
}

#[tokio::test]
async fn release_groups_symbols_by_file_into_one_event_per_file() {
    // The production helper groups released locks by file into one event
    // per file (not one per symbol). Two symbols in the same file should
    // produce a single event carrying both names.
    let tracker: Box<dyn ClaimTracker> = Box::new(LocalClaimTracker::new());
    let bus = EventBus::new();
    let repo = Uuid::new_v4();
    let sid = Uuid::new_v4();
    let mut rx = bus.subscribe(&repo.to_string());

    tracker
        .acquire_lock(repo, "src/a.rs", claim(sid, "a", "foo"))
        .await
        .unwrap();
    tracker
        .acquire_lock(repo, "src/a.rs", claim(sid, "a", "bar"))
        .await
        .unwrap();
    tracker
        .acquire_lock(repo, "src/b.rs", claim(sid, "a", "baz"))
        .await
        .unwrap();

    let count = release_and_emit(&*tracker, &bus, repo, sid, &sid.to_string(), "cs").await;
    assert_eq!(count, 3);

    // Drain both events. Order isn't guaranteed across files so we
    // assert on the collected set.
    let mut events = Vec::new();
    for _ in 0..2 {
        let ev = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
            .await
            .expect("event should arrive")
            .expect("bus not closed");
        events.push(ev);
    }
    assert_eq!(events.len(), 2, "expected one event per file");
    let paths: std::collections::HashSet<String> = events
        .iter()
        .flat_map(|e| e.affected_files.iter().map(|f| f.path.clone()))
        .collect();
    assert!(paths.contains("src/a.rs"));
    assert!(paths.contains("src/b.rs"));
}

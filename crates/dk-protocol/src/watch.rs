use tokio::sync::{broadcast, mpsc};
use tonic::Status;

use crate::server::ProtocolServer;
use crate::{WatchEvent, WatchRequest};

/// Long-running handler for the WATCH server-streaming RPC.
///
/// Subscribes to the shared [`EventBus`] and forwards every event to the
/// client via the provided `mpsc::Sender`.  The loop terminates when:
///
/// * The client disconnects (send fails).
/// * The event bus is dropped (channel closed).
///
/// Lagged receivers (slow consumers) log a warning and continue.
pub async fn handle_watch(
    server: &ProtocolServer,
    req: WatchRequest,
    tx: mpsc::Sender<Result<WatchEvent, Status>>,
) {
    let session = match server.validate_session(&req.session_id) {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(Err(e)).await;
            return;
        }
    };
    if let Err(e) = crate::require_live_session::require_live_session(server, &req.session_id).await
    {
        let _ = tx.send(Err(e)).await;
        return;
    }

    // Resolve repo_id: prefer the one from the request, fall back to
    // resolving it from the session's codebase.
    let repo_id = if !req.repo_id.is_empty() {
        req.repo_id.clone()
    } else {
        // Try to resolve repo_id from the session's codebase name.
        match server.engine().get_repo(&session.codebase).await {
            Ok((rid, _git_repo)) => rid.to_string(),
            Err(_) => {
                // Fall back to subscribe_all if we can't resolve the repo.
                String::new()
            }
        }
    };

    // Subscribe to the repo-specific channel, or all events if no repo resolved.
    let mut rx = if repo_id.is_empty() {
        server.event_bus().subscribe_all()
    } else {
        server.event_bus().subscribe(&repo_id)
    };

    let filter = &req.filter;
    let self_session_id = req.session_id.clone();

    loop {
        match rx.recv().await {
            Ok(event) => {
                // Filter out events from the requesting session itself.
                if event.session_id == self_session_id {
                    continue;
                }

                if matches_filter(&event.event_type, filter) && tx.send(Ok(event)).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("watch stream lagged by {} events", n);
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Check if an event matches a glob-style filter.
///
/// Supported patterns:
/// - Empty or "*" matches everything
/// - "changeset.*" matches "changeset.submitted", "changeset.merged", etc.
/// - "*.merged" matches "changeset.merged", "branch.merged", etc.
/// - Exact match: "changeset.submitted" matches only that event type
fn matches_filter(event_type: &str, filter: &str) -> bool {
    if filter.is_empty() || filter == "*" {
        return true;
    }

    if let Some(prefix) = filter.strip_suffix(".*") {
        event_type.starts_with(prefix) && event_type.as_bytes().get(prefix.len()) == Some(&b'.')
    } else if let Some(suffix) = filter.strip_prefix("*.") {
        event_type.ends_with(suffix)
            && event_type.len() > suffix.len()
            && event_type.as_bytes()[event_type.len() - suffix.len() - 1] == b'.'
    } else {
        event_type == filter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_matches_all() {
        assert!(matches_filter("changeset.submitted", ""));
        assert!(matches_filter("anything", ""));
    }

    #[test]
    fn star_matches_all() {
        assert!(matches_filter("changeset.submitted", "*"));
        assert!(matches_filter("anything", "*"));
    }

    #[test]
    fn prefix_glob() {
        assert!(matches_filter("changeset.submitted", "changeset.*"));
        assert!(matches_filter("changeset.merged", "changeset.*"));
        assert!(matches_filter("changeset.verified", "changeset.*"));
        assert!(!matches_filter("branch.created", "changeset.*"));
        assert!(!matches_filter("changesetx.foo", "changeset.*"));
    }

    #[test]
    fn suffix_glob() {
        assert!(matches_filter("changeset.merged", "*.merged"));
        assert!(matches_filter("branch.merged", "*.merged"));
        assert!(!matches_filter("changeset.submitted", "*.merged"));
        assert!(!matches_filter("xmerged", "*.merged"));
    }

    #[test]
    fn exact_match() {
        assert!(matches_filter("changeset.submitted", "changeset.submitted"));
        assert!(!matches_filter("changeset.merged", "changeset.submitted"));
    }
}

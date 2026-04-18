//! STALE_OVERLAY pre-write policy.
//!
//! Locks now release at `dk_submit` (default on; opt out with
//! `DKOD_RELEASE_ON_SUBMIT=0`), so a waiting session can acquire a
//! contested symbol seconds after the holder submits — far sooner than the
//! old "locks release at merge" window. The recovery contract tells agents
//! to re-read the file before writing, but if they skip that step their
//! overlay is still pinned to `base_commit` and they will silently clobber
//! the submitted (but not-yet-merged) overlay from the other session.
//!
//! This module is the engine-side backstop. It is deliberately pure so it
//! can be unit-tested without Postgres; the live handler is a thin wrapper
//! around a `ChangesetStore` query and a call into [`is_stale`].

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Minimal view of a submitted-but-not-merged changeset that touched the
/// same file path as the write we're about to perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompetingChangeset {
    pub changeset_id: Uuid,
    pub session_id: Option<Uuid>,
    pub state: String,
    pub updated_at: DateTime<Utc>,
}

/// States treated as "live" — a changeset in any of these states may still
/// affect an overlay this write is about to clobber.
///
/// `draft` is excluded because drafts are session-local and cannot have been
/// observed by another session. `merged` is excluded because once a change
/// is on main, the AST merger at `dk_merge` is the correct layer to
/// reconcile it — STALE firing there would be a false positive. `rejected`
/// and `closed` are inert.
pub const LIVE_STATES: &[&str] = &["submitted", "verifying", "approved"];

/// Return the first competitor that makes the session's local view of the
/// file stale, or `None` if the session is safe to write.
///
/// A competitor counts as stale when **all** of the following hold:
/// - its `session_id` differs from `session_id` (self-amend is never stale
///   against itself — note that in PR1 one session owns at most one
///   submitted changeset, so this degenerates to "same session");
/// - its `state` is in [`LIVE_STATES`];
/// - either the session has never read the path (`last_read.is_none()`), or
///   the competitor's `updated_at` is strictly after the session's last
///   read.
///
/// Callers should surface the returned competitor's `changeset_id` in the
/// STALE_OVERLAY response so the agent has a concrete referent for its
/// retry.
pub fn is_stale(
    session_id: Uuid,
    last_read: Option<DateTime<Utc>>,
    competitors: &[CompetingChangeset],
) -> Option<&CompetingChangeset> {
    competitors.iter().find(|cs| {
        if cs.session_id == Some(session_id) {
            return false;
        }
        if !LIVE_STATES.contains(&cs.state.as_str()) {
            return false;
        }
        match last_read {
            Some(lr) => cs.updated_at > lr,
            None => true,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(session: Option<Uuid>, state: &str, at: DateTime<Utc>) -> CompetingChangeset {
        CompetingChangeset {
            changeset_id: Uuid::new_v4(),
            session_id: session,
            state: state.to_string(),
            updated_at: at,
        }
    }

    #[test]
    fn no_competitors_is_safe() {
        let sid = Uuid::new_v4();
        assert!(is_stale(sid, None, &[]).is_none());
        assert!(is_stale(sid, Some(Utc::now()), &[]).is_none());
    }

    #[test]
    fn self_session_never_stale() {
        let sid = Uuid::new_v4();
        let competitors = vec![cs(Some(sid), "submitted", Utc::now())];
        assert!(is_stale(sid, None, &competitors).is_none());
    }

    #[test]
    fn draft_never_stale() {
        let sid = Uuid::new_v4();
        let other = Uuid::new_v4();
        let competitors = vec![cs(Some(other), "draft", Utc::now())];
        assert!(is_stale(sid, None, &competitors).is_none());
    }

    #[test]
    fn merged_never_stale() {
        // Once merged, the AST merger at dk_merge is the right layer.
        let sid = Uuid::new_v4();
        let other = Uuid::new_v4();
        let competitors = vec![cs(Some(other), "merged", Utc::now())];
        assert!(is_stale(sid, None, &competitors).is_none());
    }

    #[test]
    fn rejected_and_closed_are_inert() {
        let sid = Uuid::new_v4();
        let other = Uuid::new_v4();
        assert!(is_stale(sid, None, &[cs(Some(other), "rejected", Utc::now())]).is_none());
        assert!(is_stale(sid, None, &[cs(Some(other), "closed", Utc::now())]).is_none());
    }

    #[test]
    fn competing_submitted_with_no_read_is_stale() {
        let sid = Uuid::new_v4();
        let other = Uuid::new_v4();
        let competitors = vec![cs(Some(other), "submitted", Utc::now())];
        assert!(is_stale(sid, None, &competitors).is_some());
    }

    #[test]
    fn competing_verifying_counts_as_live() {
        let sid = Uuid::new_v4();
        let other = Uuid::new_v4();
        let competitors = vec![cs(Some(other), "verifying", Utc::now())];
        assert!(is_stale(sid, None, &competitors).is_some());
    }

    #[test]
    fn competing_approved_counts_as_live() {
        let sid = Uuid::new_v4();
        let other = Uuid::new_v4();
        let competitors = vec![cs(Some(other), "approved", Utc::now())];
        assert!(is_stale(sid, None, &competitors).is_some());
    }

    #[test]
    fn read_after_competing_submit_is_safe() {
        let sid = Uuid::new_v4();
        let other = Uuid::new_v4();
        let submit_time = Utc::now() - chrono::Duration::seconds(10);
        let read_time = Utc::now();
        let competitors = vec![cs(Some(other), "submitted", submit_time)];
        assert!(is_stale(sid, Some(read_time), &competitors).is_none());
    }

    #[test]
    fn read_before_competing_submit_is_stale() {
        let sid = Uuid::new_v4();
        let other = Uuid::new_v4();
        let read_time = Utc::now() - chrono::Duration::seconds(10);
        let submit_time = Utc::now();
        let competitors = vec![cs(Some(other), "submitted", submit_time)];
        assert!(is_stale(sid, Some(read_time), &competitors).is_some());
    }

    #[test]
    fn read_equal_to_competing_submit_is_safe() {
        // Strictly after: equal timestamps are treated as "we have already seen it".
        let sid = Uuid::new_v4();
        let other = Uuid::new_v4();
        let t = Utc::now();
        let competitors = vec![cs(Some(other), "submitted", t)];
        assert!(is_stale(sid, Some(t), &competitors).is_none());
    }

    #[test]
    fn first_live_competitor_wins() {
        let sid = Uuid::new_v4();
        let other_a = Uuid::new_v4();
        let other_b = Uuid::new_v4();
        let t = Utc::now();
        // draft then submitted — the draft is skipped, the submitted returns.
        let draft = cs(Some(other_a), "draft", t);
        let live = cs(Some(other_b), "submitted", t);
        let competitors = vec![draft, live.clone()];
        let got = is_stale(sid, None, &competitors).expect("expected stale");
        assert_eq!(got.session_id, live.session_id);
        assert_eq!(got.state, "submitted");
    }

    #[test]
    fn no_session_id_still_counts() {
        // Platform-level changesets may have null session_id — treat them as
        // foreign competitors.
        let sid = Uuid::new_v4();
        let competitors = vec![cs(None, "submitted", Utc::now())];
        assert!(is_stale(sid, None, &competitors).is_some());
    }
}

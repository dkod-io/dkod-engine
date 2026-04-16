use dk_protocol::{ApproveRequest, ReviewSnapshot};

#[test]
fn approve_request_has_override_reason_and_snapshot() {
    let req = ApproveRequest {
        session_id: "s1".into(),
        override_reason: Some("Exceeded 3 review fix rounds; findings: X,Y".into()),
        review_snapshot: Some(ReviewSnapshot {
            score: 2,
            threshold: 4,
            findings_count: 3,
            provider: "openrouter".into(),
            model: "anthropic/claude-sonnet-4".into(),
        }),
    };
    assert_eq!(
        req.override_reason.as_deref(),
        Some("Exceeded 3 review fix rounds; findings: X,Y")
    );
    assert_eq!(req.review_snapshot.as_ref().unwrap().score, 2);
}

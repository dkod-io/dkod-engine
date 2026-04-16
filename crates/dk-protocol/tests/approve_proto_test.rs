use dk_protocol::{ApproveRequest, RecordReviewRequest, RecordReviewResponse, ReviewSnapshot};

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

#[test]
fn record_review_request_shape() {
    let req = RecordReviewRequest {
        session_id: "s1".into(),
        changeset_id: "c1".into(),
        tier: "deep".into(),
        score: Some(4),
        summary: Some("LGTM with minor warnings".into()),
        findings: vec![],
        provider: "anthropic".into(),
        model: "claude-sonnet-4-6".into(),
        duration_ms: 12345,
    };
    assert_eq!(req.tier, "deep");
    assert_eq!(req.score, Some(4));
    assert_eq!(req.duration_ms, 12345);
}

#[test]
fn record_review_response_shape() {
    let resp = RecordReviewResponse {
        review_id: "r1".into(),
        accepted: true,
    };
    assert!(resp.accepted);
}

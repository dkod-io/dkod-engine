use tonic::{Response, Status};
use tracing::info;

use crate::server::ProtocolServer;
use crate::{RecordReviewRequest, RecordReviewResponse};

/// Handle a RECORD_REVIEW RPC.
///
/// Persists an AI-generated code review result into `changeset_ai_reviews`.
/// Called by the harness after the LLM review completes.  Does not
/// automatically change changeset state — the harness decides whether to
/// subsequently call APPROVE or reject based on the score.
pub async fn handle_record_review(
    server: &ProtocolServer,
    req: RecordReviewRequest,
) -> Result<Response<RecordReviewResponse>, Status> {
    let _session = server.validate_session(&req.session_id)?;

    let changeset_id = req
        .changeset_id
        .parse::<uuid::Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid changeset_id format"))?;

    if req.tier.is_empty() {
        return Err(Status::invalid_argument("tier must not be empty"));
    }
    if req.provider.is_empty() {
        return Err(Status::invalid_argument("provider must not be empty"));
    }
    if req.model.is_empty() {
        return Err(Status::invalid_argument("model must not be empty"));
    }

    // Serialise findings to JSONB — prost types don't derive Serialize, so we
    // map each finding into a plain serde_json::Value manually.
    let findings_json = serde_json::Value::Array(
        req.findings
            .iter()
            .map(|f| {
                serde_json::json!({
                    "id":         f.id,
                    "file_path":  f.file_path,
                    "line_start": f.line_start,
                    "line_end":   f.line_end,
                    "severity":   f.severity,
                    "category":   f.category,
                    "message":    f.message,
                    "suggestion": f.suggestion,
                    "confidence": f.confidence,
                    "dismissed":  f.dismissed,
                })
            })
            .collect(),
    );

    let review_id = server
        .engine()
        .changeset_store()
        .record_ai_review(
            changeset_id,
            &req.tier,
            req.score,
            req.summary.as_deref(),
            &findings_json,
            &req.provider,
            &req.model,
            req.duration_ms,
        )
        .await
        .map_err(|e| Status::internal(format!("Failed to record review: {e}")))?;

    info!(
        review_id    = %review_id,
        changeset_id = %changeset_id,
        tier         = %req.tier,
        score        = ?req.score,
        provider     = %req.provider,
        model        = %req.model,
        duration_ms  = req.duration_ms,
        findings     = req.findings.len(),
        "RECORD_REVIEW: AI review persisted"
    );

    Ok(Response::new(RecordReviewResponse {
        review_id: review_id.to_string(),
        accepted: true,
    }))
}

#[cfg(test)]
mod tests {
    use crate::ReviewFindingProto;

    #[test]
    fn findings_serialise_to_json_array() {
        let findings = [ReviewFindingProto {
            id: "f1".to_string(),
            file_path: "src/lib.rs".to_string(),
            line_start: Some(5),
            line_end: Some(10),
            severity: "error".to_string(),
            category: "correctness".to_string(),
            message: "potential panic".to_string(),
            suggestion: Some("add bounds check".to_string()),
            confidence: 0.95,
            dismissed: false,
        }];

        let json = serde_json::Value::Array(
            findings
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "id": f.id, "file_path": f.file_path,
                        "line_start": f.line_start, "line_end": f.line_end,
                        "severity": f.severity, "category": f.category,
                        "message": f.message, "suggestion": f.suggestion,
                        "confidence": f.confidence, "dismissed": f.dismissed,
                    })
                })
                .collect(),
        );

        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "f1");
        assert_eq!(arr[0]["line_start"], 5);
    }
}

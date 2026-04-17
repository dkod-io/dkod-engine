use tonic::{Response, Status};

use crate::server::ProtocolServer;
use crate::{ReviewFindingProto, ReviewRequest, ReviewResponse, ReviewResultProto};

/// Handle a REVIEW RPC.
///
/// Returns all AI review results recorded for the given changeset via
/// [`RecordReview`].  Results are ordered oldest-first so the caller can
/// track review history across multiple harness runs.
pub async fn handle_review(
    server: &ProtocolServer,
    req: ReviewRequest,
) -> Result<Response<ReviewResponse>, Status> {
    let _session = server.validate_session(&req.session_id)?;

    let changeset_id = req
        .changeset_id
        .parse::<uuid::Uuid>()
        .map_err(|_| Status::invalid_argument("Invalid changeset_id format"))?;

    let rows = server
        .engine()
        .changeset_store()
        .get_ai_reviews(changeset_id)
        .await
        .map_err(|e| Status::internal(format!("DB error fetching reviews: {e}")))?;

    let reviews: Vec<ReviewResultProto> = rows
        .into_iter()
        .map(|r| ReviewResultProto {
            id:         r.id.to_string(),
            tier:       r.tier,
            score:      r.score,
            summary:    r.summary,
            findings:   parse_findings(&r.findings),
            created_at: r.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Response::new(ReviewResponse { reviews }))
}

/// Deserialise the JSONB findings array back into `ReviewFindingProto` messages.
fn parse_findings(value: &serde_json::Value) -> Vec<ReviewFindingProto> {
    let arr = match value.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    arr.iter()
        .map(|v| ReviewFindingProto {
            id:          v["id"].as_str().unwrap_or("").to_string(),
            file_path:   v["file_path"].as_str().unwrap_or("").to_string(),
            line_start:  v["line_start"].as_i64().map(|n| n as i32),
            line_end:    v["line_end"].as_i64().map(|n| n as i32),
            severity:    v["severity"].as_str().unwrap_or("").to_string(),
            category:    v["category"].as_str().unwrap_or("").to_string(),
            message:     v["message"].as_str().unwrap_or("").to_string(),
            suggestion:  v["suggestion"].as_str().map(String::from),
            confidence:  v["confidence"].as_f64().unwrap_or(0.0) as f32,
            dismissed:   v["dismissed"].as_bool().unwrap_or(false),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_findings_empty_array() {
        assert!(parse_findings(&serde_json::json!([])).is_empty());
    }

    #[test]
    fn parse_findings_null_is_empty() {
        assert!(parse_findings(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn parse_findings_roundtrip() {
        let json = serde_json::json!([{
            "id": "f1", "file_path": "src/main.rs",
            "line_start": 10, "line_end": 15,
            "severity": "warning", "category": "style",
            "message": "use idiomatic Rust",
            "suggestion": "replace loop with iterator",
            "confidence": 0.9, "dismissed": false
        }]);
        let findings = parse_findings(&json);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id, "f1");
        assert_eq!(findings[0].line_start, Some(10));
        assert!((findings[0].confidence - 0.9f32).abs() < 0.001);
    }
}

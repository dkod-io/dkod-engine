use tokio::sync::mpsc;
use tonic::Status;
use uuid::Uuid;

use dk_runner::executor::process::ProcessExecutor;
use dk_runner::Runner;

use crate::server::ProtocolServer;
use crate::{VerifyRequest, VerifyStepResult};
use crate::proto::dekode::v1::Finding as ProtoFinding;
use crate::proto::dekode::v1::Suggestion as ProtoSuggestion;

pub async fn handle_verify(
    server: &ProtocolServer,
    req: VerifyRequest,
    tx: mpsc::Sender<Result<VerifyStepResult, Status>>,
) {
    // Validate session
    let session = match server.validate_session(&req.session_id) {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(Err(e)).await;
            return;
        }
    };

    let changeset_id = match req.changeset_id.parse::<Uuid>() {
        Ok(id) => id,
        Err(_) => {
            let _ = tx
                .send(Err(Status::invalid_argument("invalid changeset_id")))
                .await;
            return;
        }
    };

    // Verify changeset exists and update status to verifying
    let engine = server.engine();
    {
        if let Err(e) = engine.changeset_store().get(changeset_id).await {
            let _ = tx.send(Err(Status::not_found(e.to_string()))).await;
            return;
        }
        let _ = engine
            .changeset_store()
            .update_status(changeset_id, "verifying")
            .await;
    }

    // Publish verify_started event
    server.event_bus().publish(crate::WatchEvent {
        event_type: "changeset.verify_started".to_string(),
        changeset_id: changeset_id.to_string(),
        agent_id: session.agent_id.clone(),
        affected_symbols: vec![],
        details: String::new(),
    });

    // Create runner with process executor
    let runner = Runner::new(
        server.engine.clone(),
        Box::new(ProcessExecutor::new()),
    );

    // Bridge dk-runner StepResults to gRPC VerifyStepResults
    let (runner_tx, mut runner_rx) = tokio::sync::mpsc::channel(32);

    let codebase = session.codebase.clone();
    let grpc_tx = tx.clone();

    // Spawn runner in background
    let runner_handle = tokio::spawn(async move {
        runner.verify(changeset_id, &codebase, runner_tx).await
    });

    // Forward results from runner to gRPC stream
    let mut step_counter = 0i32;
    while let Some(result) = runner_rx.recv().await {
        step_counter += 1;

        let step_status_str = result.status.as_str().to_string();
        let step_name_str = result.step_name.clone();

        let findings: Vec<ProtoFinding> = result.findings.iter().map(|f| {
            ProtoFinding {
                severity: f.severity.as_str().to_string(),
                check_name: f.check_name.clone(),
                message: f.message.clone(),
                file_path: f.file_path.clone(),
                line: f.line,
                symbol: f.symbol.clone(),
            }
        }).collect();

        let suggestions: Vec<ProtoSuggestion> = result.suggestions.iter().map(|s| {
            ProtoSuggestion {
                finding_index: s.finding_index as u32,
                description: s.description.clone(),
                file_path: s.file_path.clone(),
                replacement: s.replacement.clone(),
            }
        }).collect();

        let _ = grpc_tx
            .send(Ok(VerifyStepResult {
                step_order: step_counter,
                step_name: result.step_name,
                status: result.status.as_str().to_string(),
                output: result.output,
                required: result.required,
                findings,
                suggestions,
            }))
            .await;

        // Publish verify_step event for each step
        server.event_bus().publish(crate::WatchEvent {
            event_type: "changeset.verify_step".to_string(),
            changeset_id: changeset_id.to_string(),
            agent_id: session.agent_id.clone(),
            affected_symbols: vec![],
            details: format!("{}:{}", step_name_str, step_status_str),
        });
    }

    // Get final result and update changeset status
    let final_status = match runner_handle.await {
        Ok(Ok(passed)) => if passed { "approved" } else { "rejected" },
        Ok(Err(e)) => {
            tracing::error!("runner error: {e}");
            "rejected"
        }
        Err(e) => {
            tracing::error!("runner task panicked: {e}");
            "rejected"
        }
    };

    let _ = engine
        .changeset_store()
        .update_status(changeset_id, final_status)
        .await;

    // Publish verified event with final status
    server.event_bus().publish(crate::WatchEvent {
        event_type: "changeset.verified".to_string(),
        changeset_id: changeset_id.to_string(),
        agent_id: session.agent_id.clone(),
        affected_symbols: vec![],
        details: final_status.to_string(),
    });
}

// ── Event type constants ────────────────────────────────────────────
// Extracted from the handler above so they can be tested and referenced
// by other modules without string duplication.

/// Event published when verification begins.
pub const EVENT_VERIFY_STARTED: &str = "changeset.verify_started";
/// Event published after each verification step completes.
pub const EVENT_VERIFY_STEP: &str = "changeset.verify_step";
/// Event published when the entire verification pipeline finishes.
pub const EVENT_VERIFIED: &str = "changeset.verified";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_started_event_type() {
        assert_eq!(EVENT_VERIFY_STARTED, "changeset.verify_started");
    }

    #[test]
    fn verify_step_event_type() {
        assert_eq!(EVENT_VERIFY_STEP, "changeset.verify_step");
    }

    #[test]
    fn verified_event_type() {
        assert_eq!(EVENT_VERIFIED, "changeset.verified");
    }

    #[test]
    fn verify_event_types_use_dot_separator() {
        for event in [EVENT_VERIFY_STARTED, EVENT_VERIFY_STEP, EVENT_VERIFIED] {
            assert!(
                event.contains('.'),
                "event type '{}' should use dot separator",
                event
            );
            assert!(
                event.starts_with("changeset."),
                "event type '{}' should start with 'changeset.'",
                event
            );
        }
    }

    #[test]
    fn verify_event_types_are_distinct() {
        let events = [EVENT_VERIFY_STARTED, EVENT_VERIFY_STEP, EVENT_VERIFIED];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(
                    events[i], events[j],
                    "event types should be distinct: '{}' vs '{}'",
                    events[i], events[j]
                );
            }
        }
    }
}

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
    let _session = match server.validate_session(&req.session_id) {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(Err(e)).await;
            return;
        }
    };

    let mut rx = server.event_bus().subscribe();

    loop {
        match rx.recv().await {
            Ok(event) => {
                if tx.send(Ok(event)).await.is_err() {
                    break; // Client disconnected
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("watch stream lagged by {} events", n);
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

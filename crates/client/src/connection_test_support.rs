//! Shared channel-only owner fixture; it never creates an OS child.
use super::*;

// Channel-only seam: no executable override, process creation, or runtime
// fixture dependency is added to the production sibling selector.
pub(super) fn synthetic_process(
    reply: Result<Response, String>,
    reaped: Option<ReapResult>,
    cancellation: Option<ConnectionCancellation>,
) -> (ProcessClient, mpsc::Receiver<Request>, mpsc::Receiver<()>) {
    let (requests, request_rx) = mpsc::sync_channel(1);
    let (responses, response_rx) = mpsc::sync_channel(1);
    let (observed, observed_rx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok(request) = request_rx.recv() {
            let _ = observed.send(request);
            let _ = responses.send(reply);
        }
    });
    let (shutdown, shutdown_rx) = mpsc::channel();
    let (completion, completion_rx) = mpsc::channel();
    if let Some(result) = reaped {
        completion.send(result).unwrap();
    }
    (
        ProcessClient {
            cancellation,
            requests: Some(requests),
            responses: Some(response_rx),
            observation: Arc::new(TransportObservation::default()),
            shutdown: Some(shutdown),
            reaped: completion_rx,
            stderr: Arc::new(Mutex::new(Vec::new())),
            lifecycle: None,
            next_id: 0,
            connected: true,
            java_language_session: false,
            java_startup: JavaStartupMode::default(),
        },
        observed_rx,
        shutdown_rx,
    )
}

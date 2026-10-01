//! In-memory trace capture for tests and tooling: runs a closure under a
//! scoped (non-global) subscriber and returns the parsed Chrome events.

use std::io::Write;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tracing_subscriber::layer::SubscriberExt;

use crate::ChromeTraceLayer;
use crate::metadata::TraceMetadata;

#[derive(Clone, Default)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("invariant: capture buffer lock is never poisoned")
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Run `body` with a scoped Chrome layer installed and return the trace events.
///
/// # Panics
///
/// Panics if the produced document is not a well-formed JSON array, which is
/// exactly the property callers use this helper to assert.
pub fn capture_trace(body: impl FnOnce()) -> Vec<Value> {
    capture_trace_with(body, None)
}

/// Like [`capture_trace`] but stamps a `process_labels` metadata event up front
/// when `metadata` is `Some`; `None` matches the pre-feature behaviour exactly.
///
/// # Panics
///
/// Panics if the produced document is not a well-formed JSON array (same
/// invariant as [`capture_trace`]).
pub fn capture_trace_with(body: impl FnOnce(), metadata: Option<TraceMetadata>) -> Vec<Value> {
    let buf = SharedBuf::default();
    let (layer, guard) = metadata.map_or_else(
        || ChromeTraceLayer::new(buf.clone()),
        |m| ChromeTraceLayer::with_metadata(buf.clone(), Some(m)),
    );
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, body);
    guard
        .finish()
        .expect("invariant: in-memory writes cannot fail");
    let bytes = buf
        .0
        .lock()
        .expect("invariant: capture buffer lock is never poisoned")
        .clone();
    let text = String::from_utf8(bytes).expect("invariant: trace is UTF-8 JSON");
    match serde_json::from_str::<Value>(&text).expect("invariant: trace is valid JSON") {
        Value::Array(events) => events,
        other => panic!("trace root must be an array, got {other}"),
    }
}

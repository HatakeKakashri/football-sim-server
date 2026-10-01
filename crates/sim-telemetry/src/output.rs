//! Trace file creation and global subscriber installation.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use tracing_subscriber::layer::SubscriberExt;

use crate::metadata::TraceMetadata;
use crate::{ChromeTraceLayer, TelemetryError, TraceGuard};

/// Create `path` and return a layer writing Chrome JSON to it, plus its guard.
///
/// # Errors
///
/// `Io` when the file cannot be created.
pub fn open(
    path: &Path,
) -> Result<(ChromeTraceLayer<BufWriter<File>>, TraceGuard), TelemetryError> {
    open_with_metadata(path, None)
}

/// Like [`open`] but stamps a run-header `process_labels` event on first write.
///
/// The header carries `displayTimeUnit: "us"` (so Perfetto renders µs as µs,
/// not as ms) plus the run's seed, interval, and full range.
///
/// # Errors
///
/// `Io` when the file cannot be created.
pub fn open_with_metadata(
    path: &Path,
    metadata: Option<TraceMetadata>,
) -> Result<(ChromeTraceLayer<BufWriter<File>>, TraceGuard), TelemetryError> {
    let file =
        File::create(path).map_err(|e| TelemetryError::Io(format!("{}: {e}", path.display())))?;
    Ok(ChromeTraceLayer::with_metadata(BufWriter::new(file), metadata))
}

/// Create `path` and install the Chrome layer as the process-wide subscriber.
/// Hold the returned guard for the whole run and call `finish()` at the end.
///
/// # Errors
///
/// `Io` when the file cannot be created; `SubscriberAlreadySet` when another
/// global subscriber exists.
pub fn install(path: &Path) -> Result<TraceGuard, TelemetryError> {
    install_with_metadata(path, None)
}

/// Like [`install`] but stamps a run-header `process_labels` event on first
/// write. See [`open_with_metadata`] for why this exists.
///
/// # Errors
///
/// `Io` when the file cannot be created; `SubscriberAlreadySet` when another
/// global subscriber exists.
pub fn install_with_metadata(
    path: &Path,
    metadata: Option<TraceMetadata>,
) -> Result<TraceGuard, TelemetryError> {
    let (layer, guard) = open_with_metadata(path, metadata)?;
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .map_err(|_| TelemetryError::SubscriberAlreadySet)?;
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TraceMetadata;
    use serde_json::Value;

    #[test]
    fn open_reports_io_error_for_an_uncreatable_path() {
        let missing = std::env::temp_dir().join("sim-telemetry-no-such-dir/trace.json");
        match open(&missing) {
            Err(TelemetryError::Io(msg)) => assert!(!msg.is_empty()),
            Err(other) => panic!("expected Io error, got {other:?}"),
            Ok(_) => panic!("expected Io error, got Ok"),
        }
    }

    #[test]
    fn open_writes_a_valid_empty_array_on_finish() {
        let path =
            std::env::temp_dir().join(format!("sim-telemetry-open-{}.json", std::process::id()));
        let (layer, guard) = open(&path).expect("open");
        drop(layer);
        guard.finish().expect("finish");
        let text = std::fs::read_to_string(&path).expect("read");
        std::fs::remove_file(&path).expect("cleanup");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).expect("json"),
            serde_json::json!([])
        );
    }

    #[test]
    fn open_with_metadata_writes_the_header_event() {
        let path = std::env::temp_dir().join(format!(
            "sim-telemetry-meta-{}.json",
            std::process::id()
        ));
        let meta = TraceMetadata::new(99, 600, Some((10, 20)));
        let (layer, guard) = open_with_metadata(&path, Some(meta)).expect("open");
        // Install as a scoped subscriber so the layer actually fires.
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            crate::emit::counter(
                crate::emit::Track {
                    pid: 1,
                    tid: 1,
                    process: "p",
                    thread: "t",
                },
                0,
                "x",
                1.0,
            );
        });
        guard.finish().expect("finish");
        let text = std::fs::read_to_string(&path).expect("read");
        std::fs::remove_file(&path).expect("cleanup");
        let events: Vec<Value> = serde_json::from_str(&text).expect("trace is a JSON array");
        let header = events
            .iter()
            .find(|e| e["name"] == "process_labels")
            .expect("header event");
        assert_eq!(header["args"]["displayTimeUnit"], "us");
        assert_eq!(header["args"]["seed"], 99);
        assert_eq!(header["args"]["interval_ticks"], 600);
        assert_eq!(header["args"]["full_range"], serde_json::json!([10, 20]));
    }
}

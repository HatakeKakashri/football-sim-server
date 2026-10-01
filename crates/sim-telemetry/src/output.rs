//! Trace file creation and global subscriber installation.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use tracing_subscriber::layer::SubscriberExt;

use crate::{ChromeTraceLayer, TelemetryError, TraceGuard};

/// Create `path` and return a layer writing Chrome JSON to it, plus its guard.
///
/// # Errors
///
/// `Io` when the file cannot be created.
pub fn open(
    path: &Path,
) -> Result<(ChromeTraceLayer<BufWriter<File>>, TraceGuard), TelemetryError> {
    let file =
        File::create(path).map_err(|e| TelemetryError::Io(format!("{}: {e}", path.display())))?;
    Ok(ChromeTraceLayer::new(BufWriter::new(file)))
}

/// Create `path` and install the Chrome layer as the process-wide subscriber.
/// Hold the returned guard for the whole run and call `finish()` at the end.
///
/// # Errors
///
/// `Io` when the file cannot be created; `SubscriberAlreadySet` when another
/// global subscriber exists.
pub fn install(path: &Path) -> Result<TraceGuard, TelemetryError> {
    let (layer, guard) = open(path)?;
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .map_err(|_| TelemetryError::SubscriberAlreadySet)?;
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

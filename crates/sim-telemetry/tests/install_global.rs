//! Process-global install path. Lives in its own integration-test binary
//! because a global subscriber can only be set once per process.

use sim_telemetry::emit::{self, Track};
use sim_telemetry::{TelemetryError, install};

#[test]
fn install_writes_a_parseable_file_and_rejects_a_second_install() {
    let path =
        std::env::temp_dir().join(format!("sim-telemetry-global-{}.json", std::process::id()));
    let guard = install(&path).expect("first install");

    let track = Track {
        pid: 1,
        tid: 1,
        process: "Ball",
        thread: "State",
    };
    emit::counter(track, 60, "x", 52.5);

    let second =
        std::env::temp_dir().join(format!("sim-telemetry-global2-{}.json", std::process::id()));
    assert!(matches!(
        install(&second),
        Err(TelemetryError::SubscriberAlreadySet)
    ));
    let _ = std::fs::remove_file(&second);

    guard.finish().expect("finish");
    let text = std::fs::read_to_string(&path).expect("read");
    std::fs::remove_file(&path).expect("cleanup");
    let events: Vec<serde_json::Value> = serde_json::from_str(&text).expect("valid JSON array");
    let counter = events
        .iter()
        .find(|e| e["ph"] == "C")
        .expect("counter event");
    assert_eq!(counter["name"], "x");
    assert_eq!(counter["args"]["value"], 52.5);
    assert!(
        events
            .iter()
            .any(|e| e["name"] == "process_name" && e["args"]["name"] == "Ball")
    );
}

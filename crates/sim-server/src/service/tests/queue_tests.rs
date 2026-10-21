//! Tests for the concurrency cap and the FIFO queue (server-API
//! ticket 06).
//!
//! The queue is driven entirely through the fake runner: the gated
//! runner holds a match in `Running` until the test opens its gate, so
//! the order in which the queued matches start is observable, and the
//! cap is proven by what the runner is asked to run.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;

/// Counts the matches that are in the runner at the same time and
/// records the peak.
#[derive(Debug, Default)]
struct Concurrency {
    running: AtomicUsize,
    peak: AtomicUsize,
}

impl Concurrency {
    /// Enter the runner; the guard leaves it when it is dropped.
    fn enter(&self) -> ConcurrencyGuard<'_> {
        let running = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(running, Ordering::SeqCst);
        ConcurrencyGuard { counts: self }
    }

    /// The highest number of matches that were in the runner at once.
    fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

/// Leaves the runner when it is dropped, so a panicking run is counted
/// back too.
struct ConcurrencyGuard<'a> {
    counts: &'a Concurrency,
}

impl Drop for ConcurrencyGuard<'_> {
    fn drop(&mut self) {
        self.counts.running.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A runner whose behaviour is decided by the match id, recording the
/// order in which the matches entered the runner.
///
/// Every match waits for its own gate first, so the test decides when
/// a match ends. A `fail-` id then reports a deterministic failure and
/// a `panic-` id panics; every other id completes.
#[derive(Debug, Default)]
struct ScriptedRunner {
    gates: Mutex<HashMap<String, Arc<Gate>>>,
    order: Mutex<Vec<String>>,
    concurrency: Concurrency,
    /// Set when the test ends: every gate is open from then on, so a
    /// run that starts after the teardown cannot block forever.
    released: AtomicBool,
}

impl ScriptedRunner {
    /// The gate of one match, created on first use.
    fn gate(&self, match_id: &str) -> Arc<Gate> {
        let gate = {
            let mut gates = self
                .gates
                .lock()
                .expect("invariant: gate map lock is never poisoned");
            Arc::clone(
                gates
                    .entry(match_id.to_string())
                    .or_insert_with(|| Arc::new(Gate::new())),
            )
        };
        if self.released.load(Ordering::SeqCst) {
            gate.open();
        }
        gate
    }

    /// Open the gate of one match: the match leaves the runner.
    fn open(&self, match_id: &str) {
        self.gate(match_id).open();
    }

    /// Open every gate and release the ones created later: the matches
    /// that are still running leave the runner.
    fn open_all(&self) {
        self.released.store(true, Ordering::SeqCst);
        let gates: Vec<Arc<Gate>> = self
            .gates
            .lock()
            .expect("invariant: gate map lock is never poisoned")
            .values()
            .cloned()
            .collect();
        for gate in gates {
            gate.open();
        }
    }

    /// The match ids in the order they entered the runner.
    fn order(&self) -> Vec<String> {
        self.order
            .lock()
            .expect("invariant: order lock is never poisoned")
            .clone()
    }

    /// The highest number of matches that were in the runner at once.
    fn peak_concurrency(&self) -> usize {
        self.concurrency.peak()
    }
}

/// Opens every gate when the test ends, so a failed assertion cannot
/// leave a blocked run behind: the runtime waits for the blocking pool
/// to drain when it shuts down.
struct GateGuard(Arc<ScriptedRunner>);

impl Drop for GateGuard {
    fn drop(&mut self) {
        self.0.open_all();
    }
}

impl MatchRunner for ScriptedRunner {
    fn run(&self, fixture: &MatchFixture, _tick_cap: u64) -> MatchOutcome {
        let id = fixture.validated_match.id.clone();
        self.order
            .lock()
            .expect("invariant: order lock is never poisoned")
            .push(id.clone());
        let _running = self.concurrency.enter();
        self.gate(&id).wait();
        if id.starts_with("fail-") {
            return MatchOutcome::Failed {
                code: "tick_cap_exceeded".to_string(),
                message: "injected failure".to_string(),
                last_good_tick: 0,
                kind: FailureKind::Deterministic,
            };
        }
        assert!(!id.starts_with("panic-"), "injected panic for {id}");
        MatchOutcome::Finished(completed_for(fixture))
    }
}

/// With the cap at N, the (N+1)th start is queued and runs only when a
/// permit frees.
#[tokio::test]
async fn over_cap_start_is_queued_and_runs_when_a_permit_frees() {
    let runner = Arc::new(ScriptedRunner::default());
    let _guard = GateGuard(Arc::clone(&runner));
    let service = Arc::new(
        MatchService::new(
            fixture_set(&["m-0", "m-1", "m-2"]),
            as_dyn(Arc::clone(&runner)),
        )
        .with_cap(2),
    );

    // The first two starts take the two permits.
    assert_eq!(service.start("m-0").await, Ok(StartOutcome::Started));
    assert_eq!(service.start("m-1").await, Ok(StartOutcome::Started));
    wait_until(&service, "m-0", |_| runner.order().len() >= 2).await;

    // The third start finds no permit: it waits in the queue.
    assert_eq!(service.start("m-2").await, Ok(StartOutcome::Queued));
    assert_eq!(service.state("m-2"), MatchState::Queued);
    assert_eq!(runner.order().len(), 2, "the queued match has not run");

    // A permit frees: the queued match takes it and runs.
    runner.open("m-0");
    wait_until(&service, "m-2", |_| runner.order().len() >= 3).await;
    assert_eq!(service.state("m-2"), MatchState::Running);
}

/// Queued matches start in the order they arrived.
#[tokio::test]
async fn queued_matches_start_in_arrival_order() {
    let runner = Arc::new(ScriptedRunner::default());
    let _guard = GateGuard(Arc::clone(&runner));
    let service = Arc::new(
        MatchService::new(
            fixture_set(&["m-0", "m-1", "m-2"]),
            as_dyn(Arc::clone(&runner)),
        )
        .with_cap(1),
    );

    assert_eq!(service.start("m-0").await, Ok(StartOutcome::Started));
    wait_until(&service, "m-0", |_| !runner.order().is_empty()).await;

    // Both starts find no permit: they wait, oldest first.
    assert_eq!(service.start("m-1").await, Ok(StartOutcome::Queued));
    assert_eq!(service.start("m-2").await, Ok(StartOutcome::Queued));
    assert_eq!(runner.order(), vec!["m-0".to_string()]);

    // Each permit hand-off starts the match that waited longest.
    runner.open("m-0");
    wait_until(&service, "m-1", |_| runner.order().len() >= 2).await;
    assert_eq!(
        runner.order(),
        vec!["m-0".to_string(), "m-1".to_string()],
        "the first queued match runs first"
    );
    assert_eq!(service.state("m-1"), MatchState::Running);

    runner.open("m-1");
    wait_until(&service, "m-2", |_| runner.order().len() >= 3).await;
    assert_eq!(
        runner.order(),
        vec!["m-0".to_string(), "m-1".to_string(), "m-2".to_string()],
        "the second queued match runs last"
    );
    assert_eq!(service.state("m-2"), MatchState::Running);

    runner.open("m-2");
    wait_until(&service, "m-2", |state| matches!(state, MatchState::Ready { .. })).await;
}

/// A start beyond the cap is queued, never rejected.
#[tokio::test]
async fn over_cap_start_is_never_rejected() {
    let runner = Arc::new(ScriptedRunner::default());
    let _guard = GateGuard(Arc::clone(&runner));
    let service = Arc::new(
        MatchService::new(
            fixture_set(&["m-0", "m-1", "m-2"]),
            as_dyn(Arc::clone(&runner)),
        )
        .with_cap(1),
    );

    assert_eq!(service.start("m-0").await, Ok(StartOutcome::Started));
    wait_until(&service, "m-0", |_| !runner.order().is_empty()).await;

    // Every start beyond the cap succeeds with `Queued`; none is
    // refused and none is an error.
    for id in ["m-1", "m-2"] {
        assert_eq!(service.start(id).await, Ok(StartOutcome::Queued));
        assert_eq!(service.state(id), MatchState::Queued);
    }
}

/// A queued match is single-flight: a second start does not queue it a
/// second time.
#[tokio::test]
async fn second_start_of_a_queued_match_is_already_running() {
    let runner = Arc::new(ScriptedRunner::default());
    let _guard = GateGuard(Arc::clone(&runner));
    let service = Arc::new(
        MatchService::new(
            fixture_set(&["m-0", "m-1"]),
            as_dyn(Arc::clone(&runner)),
        )
        .with_cap(1),
    );

    assert_eq!(service.start("m-0").await, Ok(StartOutcome::Started));
    wait_until(&service, "m-0", |_| !runner.order().is_empty()).await;

    assert_eq!(service.start("m-1").await, Ok(StartOutcome::Queued));
    assert_eq!(
        service.start("m-1").await,
        Ok(StartOutcome::AlreadyRunning),
        "a queued match is already in flight"
    );
    assert_eq!(service.state("m-1"), MatchState::Queued);
}

/// A permit is released after a success: the next start runs at once
/// instead of being queued.
#[tokio::test]
async fn permit_is_released_after_success() {
    let runner = Arc::new(ScriptedRunner::default());
    let _guard = GateGuard(Arc::clone(&runner));
    let service = Arc::new(
        MatchService::new(
            fixture_set(&["m-0", "m-1"]),
            as_dyn(Arc::clone(&runner)),
        )
        .with_cap(1),
    );

    assert_eq!(service.start("m-0").await, Ok(StartOutcome::Started));
    wait_until(&service, "m-0", |_| !runner.order().is_empty()).await;
    runner.open("m-0");
    wait_until(&service, "m-0", |state| matches!(state, MatchState::Ready { .. })).await;

    // The permit is back in the pool, so the cap did not shrink.
    assert_eq!(service.start("m-1").await, Ok(StartOutcome::Started));
    assert_eq!(service.state("m-1"), MatchState::Running);
}

/// A permit is released after a failure: the queued match takes it and
/// runs.
#[tokio::test]
async fn permit_is_released_after_failure() {
    let runner = Arc::new(ScriptedRunner::default());
    let _guard = GateGuard(Arc::clone(&runner));
    let service = Arc::new(
        MatchService::new(
            fixture_set(&["fail-0", "m-1"]),
            as_dyn(Arc::clone(&runner)),
        )
        .with_cap(1),
    );

    assert_eq!(service.start("fail-0").await, Ok(StartOutcome::Started));
    wait_until(&service, "fail-0", |_| !runner.order().is_empty()).await;

    // The failing match holds the only permit, so the second start
    // waits for it.
    assert_eq!(service.start("m-1").await, Ok(StartOutcome::Queued));

    // The failure releases the permit: the queued match runs.
    runner.open("fail-0");
    wait_until(&service, "m-1", |_| runner.order().len() >= 2).await;
    assert_eq!(service.state("m-1"), MatchState::Running);
    match service.state("fail-0") {
        MatchState::Failed { code, .. } => assert_eq!(code, "tick_cap_exceeded"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// A permit is released after a panic: the queued match takes it and
/// runs.
#[tokio::test]
async fn permit_is_released_after_panic() {
    let runner = Arc::new(ScriptedRunner::default());
    let _guard = GateGuard(Arc::clone(&runner));
    let service = Arc::new(
        MatchService::new(
            fixture_set(&["panic-0", "m-1"]),
            as_dyn(Arc::clone(&runner)),
        )
        .with_cap(1),
    );

    assert_eq!(service.start("panic-0").await, Ok(StartOutcome::Started));
    wait_until(&service, "panic-0", |_| !runner.order().is_empty()).await;

    // The panicking match holds the only permit, so the second start
    // waits for it.
    assert_eq!(service.start("m-1").await, Ok(StartOutcome::Queued));

    // The panic releases the permit: the queued match runs.
    runner.open("panic-0");
    wait_until(&service, "m-1", |_| runner.order().len() >= 2).await;
    assert_eq!(service.state("m-1"), MatchState::Running);
    match service.state("panic-0") {
        MatchState::Failed { code, .. } => assert_eq!(code, "runner_panic"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// The cap is never exceeded: at most N matches run at the same time,
/// however many are started.
#[tokio::test]
async fn the_cap_is_never_exceeded() {
    let runner = Arc::new(ScriptedRunner::default());
    let _guard = GateGuard(Arc::clone(&runner));
    let ids = ["m-0", "m-1", "m-2", "m-3", "m-4"];
    let service = Arc::new(
        MatchService::new(fixture_set(&ids), as_dyn(Arc::clone(&runner))).with_cap(2),
    );

    // Every start is accepted: the ones beyond the cap wait.
    for id in ids {
        let outcome = service.start(id).await.expect("start must not error");
        assert!(
            matches!(outcome, StartOutcome::Started | StartOutcome::Queued),
            "unexpected outcome: {outcome:?}"
        );
    }

    // Let every match run: the cap decides how many run at once.
    runner.open_all();
    for id in ids {
        wait_until(&service, id, |state| {
            matches!(state, MatchState::Ready { .. } | MatchState::Failed { .. })
        })
        .await;
    }

    assert!(
        runner.peak_concurrency() <= 2,
        "the cap of 2 was exceeded: {} matches ran at once",
        runner.peak_concurrency()
    );
}

/// The cap never shrinks: after every kind of end, a new start runs at
/// once instead of being queued.
#[tokio::test]
async fn cap_never_shrinks() {
    for id in ["m-0", "fail-0", "panic-0"] {
        let runner = Arc::new(ScriptedRunner::default());
        let _guard = GateGuard(Arc::clone(&runner));
        let service = Arc::new(
            MatchService::new(
                fixture_set(&[id, "m-1"]),
                as_dyn(Arc::clone(&runner)),
            )
            .with_cap(1),
        );

        assert_eq!(service.start(id).await, Ok(StartOutcome::Started));
        wait_until(&service, id, |_| !runner.order().is_empty()).await;
        runner.open(id);
        wait_until(&service, id, |state| {
            matches!(state, MatchState::Ready { .. } | MatchState::Failed { .. })
        })
        .await;

        // The permit is back in the pool, so the cap did not shrink.
        assert_eq!(
            service.start("m-1").await,
            Ok(StartOutcome::Started),
            "the cap must not shrink after {id}"
        );
    }
}

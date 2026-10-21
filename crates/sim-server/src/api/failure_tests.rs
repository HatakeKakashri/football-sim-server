//! Failure-path tests: a panicking runner and a failing thread
//! spawn must never wedge the server.
//!
//! Both failures are injected through the store's seams
//! ([`MatchStore::with_runner`] / [`MatchStore::with_spawner`]), so
//! no test depends on the tick cap or on poking the store's state.

use std::sync::Arc;

use super::test_doubles::{ScriptedRunner, ScriptedSpawner};
use super::{MatchRegistry, MatchStore, ServerConfig, StreamResponse};

/// A store over the given fixture ids, with the scripted runner and
/// the scripted spawner.
fn scripted_store(match_ids: &[&str], config: ServerConfig) -> Arc<MatchStore> {
    let mut registry = MatchRegistry::default();
    for (index, match_id) in match_ids.iter().enumerate() {
        let seed = 42 + u64::try_from(index).expect("invariant: fixture index fits in u64");
        let single = MatchRegistry::single_test(match_id, seed);
        registry.fixtures.extend(single.fixtures);
    }
    Arc::new(
        MatchStore::new(config, registry)
            .with_runner(Arc::new(ScriptedRunner))
            .with_spawner(Arc::new(ScriptedSpawner)),
    )
}

/// Block until `match_id` settles.
fn wait_for(store: &MatchStore, match_id: &str) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("invariant: test runtime builds");
    runtime.block_on(store.wait_until_done(match_id));
}

/// A panicking runner fails the match, releases the slot, and leaves a
/// cached error stream behind: the `GET` returns that terminal failure
/// instead of waiting for a match that never finishes.
#[test]
fn runner_panic_fails_the_match_and_releases_the_slot() {
    let config = ServerConfig {
        max_concurrent_sims: 1,
        ..ServerConfig::default()
    };
    let store = scripted_store(&["panic-0", "panic-1"], config);

    let response = store.post_match("panic-0");
    assert_eq!(response.status, 202);
    wait_for(&store, "panic-0");

    // The match is failed, not left running: the cached stream is the
    // single `error` message.
    let StreamResponse::Ready(body) = store.stream_slice("panic-0", None) else {
        panic!("a panicked match must serve its cached error stream");
    };
    assert!(body.contains("event: error"), "body was: {body}");
    assert!(body.contains("runner_panic"), "body was: {body}");

    // The slot was released: a second match starts even though the
    // concurrency cap is one.
    let response = store.post_match("panic-1");
    assert_eq!(
        response.status, 202,
        "the concurrency slot must be released after a panic"
    );
    wait_for(&store, "panic-1");
}

/// More panics than the cap, then a normal match: the cap must not
/// shrink, so the normal match still starts.
#[test]
fn panics_do_not_shrink_the_concurrency_cap() {
    let config = ServerConfig {
        max_concurrent_sims: 1,
        ..ServerConfig::default()
    };
    let store = scripted_store(&["panic-0", "panic-1", "panic-2", "ok-0"], config);

    // Three panics through a cap of one: every one of them must
    // start, which is only possible if each released its slot.
    for match_id in ["panic-0", "panic-1", "panic-2"] {
        let response = store.post_match(match_id);
        assert_eq!(response.status, 202, "{match_id} must start");
        wait_for(&store, match_id);
        let StreamResponse::Ready(body) = store.stream_slice(match_id, None) else {
            panic!("{match_id} must serve its cached error stream");
        };
        assert!(body.contains("runner_panic"), "body was: {body}");
    }

    // The normal match after the panics still starts and finishes.
    let response = store.post_match("ok-0");
    assert_eq!(response.status, 202, "the cap must not have shrunk");
    wait_for(&store, "ok-0");
    let StreamResponse::Ready(body) = store.stream_slice("ok-0", None) else {
        panic!("a finished match must serve its cached stream");
    };
    assert!(body.contains("event: full_time"), "body was: {body}");
}

/// A thread that cannot be spawned releases the slot that was taken
/// before the spawn: the next match starts instead of being rejected
/// with 503.
#[test]
fn spawn_failure_releases_the_slot_taken_before_the_spawn() {
    let config = ServerConfig {
        max_concurrent_sims: 1,
        ..ServerConfig::default()
    };
    let store = scripted_store(&["spawn-fail-0", "ok-0"], config);

    let response = store.post_match("spawn-fail-0");
    assert_eq!(response.status, 500);

    // The slot is free again: the next match starts.
    let response = store.post_match("ok-0");
    assert_eq!(
        response.status, 202,
        "a failed spawn must release the concurrency slot"
    );
    wait_for(&store, "ok-0");
}

/// A match whose thread never started is a terminal failure, not a
/// match that waits forever.
#[test]
fn spawn_failure_caches_a_terminal_failed_match() {
    let store = scripted_store(&["spawn-fail-0"], ServerConfig::default());

    let response = store.post_match("spawn-fail-0");
    assert_eq!(response.status, 500);

    let StreamResponse::Ready(body) = store.stream_slice("spawn-fail-0", None) else {
        panic!("a match that never started must serve its cached error stream");
    };
    assert!(body.contains("event: error"), "body was: {body}");
    assert!(body.contains("thread_spawn_failed"), "body was: {body}");
}

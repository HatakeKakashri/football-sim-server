//! Tests for the runner's failure classification (server-API ticket
//! 07).
//!
//! The frame capture is the run's largest allocation, and it reserves
//! its memory fallibly: a host that cannot give it the memory fails
//! the match with a transient resource failure instead of aborting the
//! process. The allocation failure itself cannot be provoked in a
//! test, so the tests drive the classification around it.

use super::runner::{
    FailureKind, MatchOutcome, RESOURCE_FAILURE_CODE, ResourceError, copy_string, resource_failure,
};

/// A capacity overflow: the reservation cannot be satisfied, and the
/// error says so.
fn overflow() -> std::collections::TryReserveError {
    Vec::<u8>::new()
        .try_reserve(usize::MAX)
        .expect_err("a reservation of usize::MAX cannot be satisfied")
}

/// A capture that could not allocate its memory is a transient failure:
/// the pressure belongs to the host, so the match may run again later.
#[test]
fn a_capture_failure_is_a_transient_failure() {
    let outcome = resource_failure("could not allocate the frame buffer".to_string(), 120);

    let MatchOutcome::Failed {
        code,
        message,
        last_good_tick,
        kind,
    } = outcome
    else {
        panic!("expected a failure");
    };
    assert_eq!(code, RESOURCE_FAILURE_CODE);
    assert_eq!(message, "could not allocate the frame buffer");
    assert_eq!(last_good_tick, 120);
    assert_eq!(kind, FailureKind::Transient);
}

/// A failed reservation names what could not be allocated, so the
/// message says which buffer ran out.
#[test]
fn a_failed_reservation_names_the_buffer() {
    let error = ResourceError::reservation_failed("frame buffer", &overflow());

    assert!(
        error.message().contains("frame buffer"),
        "the message must name the buffer: {}",
        error.message()
    );
}

/// The capture copies the text it is given.
#[test]
fn the_capture_copies_its_text() {
    let copied = copy_string("home-p00").expect("invariant: a short string fits in memory");

    assert_eq!(copied, "home-p00");
}

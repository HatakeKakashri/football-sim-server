//! Tests for the match service (server-API ticket 05).
//!
//! The service is driven entirely through the fake runners: no HTTP, no
//! sockets and no direct state poking.
//!
//! This module holds the shared imports; the tests live in the
//! submodules next to it. The fake runners and the fixture builders
//! live in [`crate::test_support`], because the HTTP layer's tests
//! drive the same fakes.

mod lifecycle_tests;
mod queue_tests;
mod retry_tests;

use super::*;

use crate::test_support::{
    AlwaysTransientRunner, BlockOnIdRunner, Gate, GatedRunner, InstantRunner, PanickingRunner,
    TickCapFailureRunner, as_dyn, completed_for, fixture_set, wait_until,
};

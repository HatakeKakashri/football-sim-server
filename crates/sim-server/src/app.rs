//! Production composition: the match service `sim-server serve` runs.
//!
//! The `serve` command and the real-match end-to-end test must exercise the
//! same assembly — loaded fixtures, the real simulation runner and the match
//! service. Keeping that assembly here, rather than in `dispatch`, means a
//! change to the production runner cannot silently leave the end-to-end test
//! on a test-only arrangement.

use std::sync::Arc;

use crate::api::default_concurrency_cap;
use crate::api::runner::SimulationRunner;
use crate::fixtures::FixtureSet;
use crate::service::MatchService;

/// Build the production match service from the loaded fixtures.
///
/// The service runs matches with [`SimulationRunner`], the real adapter over
/// the simulation. `max_concurrent_sims` overrides the default cap of
/// `cores - 1`; `None` keeps that default. The caller owns the service and
/// mounts it with [`crate::http::router`] or [`crate::http::serve`].
#[must_use]
pub fn match_service(
    fixture_set: FixtureSet,
    max_concurrent_sims: Option<usize>,
) -> Arc<MatchService> {
    let cap = max_concurrent_sims.unwrap_or_else(default_concurrency_cap);
    Arc::new(MatchService::new(fixture_set, Arc::new(SimulationRunner)).with_cap(cap))
}

//! The completed-match model, fixture registry, runner and render step.
//!
//! [`MatchRegistry`] maps fixture `match_id`s to their validated teams
//! and seeds. [`runner`] turns a fixture into a completed match;
//! [`render`] turns that completed match into a manifest and its
//! immutable segments. [`crate::service::MatchService`] owns the
//! lifecycle that connects the two.

pub mod model;
pub mod registry;
pub mod render;
pub mod runner;

#[cfg(test)]
mod manifest_tests;
#[cfg(test)]
pub(crate) mod render_doubles;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod runner_tests;
#[cfg(test)]
mod tests;

pub use registry::{MatchFixture, MatchRegistry};

/// Default concurrency cap: one simulation per core minus one for
/// the executor (PRD §4), at least one.
#[must_use]
pub fn default_concurrency_cap() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| cores.get().max(2) - 1)
}

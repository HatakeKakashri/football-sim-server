# 29: World installer and required resources

**What to build:** A single `WorldInstaller` that both production and tests use to construct a simulation world, with all systems requiring their resources explicitly so missing resources fail loudly instead of silently defaulting.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] A `WorldInstaller` module exists and is used by `Simulation::new`, test worlds, and CLI entry points.
- [ ] All optional-resource fallback branches in systems (~47) are converted to required `Res<T>` (or equivalent); tests with missing resources now fail.
- [ ] `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` remain green.

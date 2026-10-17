# 30: Match setup module

**What to build:** A single `MatchSetup` module that owns squad spawning and resource installation, with adapters for validated fixture squads and default squads, so HTTP matches finally run actual fixture profiles instead of always discarding them after startup validation.

**Blocked by:** 29 (the world installer must exist so `MatchSetup` can delegate resource installation).

**Status:** ready-for-agent

- [ ] `MatchSetup` module extracted from `construct.rs` with a clear interface for spawning 22 players from fixture data or defaults.
- [ ] HTTP runner passes fixture squads through to the engine; a test demonstrates non-default attributes in an HTTP-started match.
- [ ] `cargo test --workspace` and `cargo clippy` remain green.

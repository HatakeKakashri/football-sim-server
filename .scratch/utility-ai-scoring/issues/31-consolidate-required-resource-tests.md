# 31: Consolidate the required-resource tests

**What to build:** Context (TDD audit): eight tests in `sim-ai-player` remove one resource each and assert the panic message contains the resource's type name, which tests Bevy's panic text rather than app behaviour. The behaviour worth protecting is: a world built by the installer runs a full tick, and a system fails loudly rather than defaulting when its required input is missing. Keep one installer-built-world-runs-a-tick test (reuse the sim-core one if it exists) and one representative should-panic test per system (decision, kick execution) that does not match the message text. Delete the rest and say so in the PR.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] An installer-built world runs every registered AI system for a tick without panicking.
- [ ] One missing-resource case per system asserts only that the run panics.
- [ ] No assertion matches Bevy's panic message format.
- [ ] The deleted cases are listed in the PR with the surviving case that covers the same contract.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.

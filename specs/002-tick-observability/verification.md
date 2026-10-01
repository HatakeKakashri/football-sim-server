# 002-tick-observability — Verification Ledger

Every claim made about this feature that depends on running something, with its
current evidence status. A claim is **Verified** only when someone has run the
command below and recorded the result here. "Author-reported" means it is stated
in `tasks.md` / `contracts/trace-schema.md` but raw output was not attached.

| ID | Claim | Status | How to verify |
|----|-------|--------|---------------|
| V1 | Trace opens in Perfetto UI with one process per player + Ball, nested decision spans, counters (SC-005) | **Open — never run** | `cargo run --release -p sim-server -- simulate --ticks 1000 --trace-out /tmp/trace.json`, then open `/tmp/trace.json` at ui.perfetto.dev and record what you see. |
| V2 | Trace is well-formed Chrome JSON (SC-001) | Covered by automated tests (`sim-server/tests/simulate_trace.rs`, `capture_trace` tests); not yet run in review | `cargo test -p sim-server -p sim-telemetry` |
| V3 | Disabled tracing costs nothing: median 46,055 vs 46,102 ticks/s over 7 alternating release runs (T013) | Author-reported; no logs | Build the commit before `002-patch` and HEAD in release mode; alternate `sim-server benchmark --ticks 60000` seven times each; compare medians. |
| V4 | Full-match output size: 663.5 MB at interval 60, 66.8 MB at 600, 11.5 MB at 3600 | Author-reported; no logs | `cargo run --release -p sim-server -- simulate --full-match --trace-out /tmp/t.json --trace-interval-ticks <n>` then `ls -l /tmp/t.json`. The default is now 600. |
| V5 | `tracing_does_not_change_the_final_state_hash` is a non-vacuous test ("mutation-checked") | Author-reported | Temporarily make `trace_emission_system` mutate a component (e.g. `stamina.0 += 1.0`), run `cargo test -p sim-core tracing_does_not_change`, confirm it fails, then revert. |
| V6 | Workspace is clean: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` | **Not run in the review environment** (no Rust toolchain) | Run the three commands on a real checkout. |
| V7 | `TeamIdComponent` fix (`create_match`) does not break existing tests | Not run | Included in V6; also compare `sim-server simulate` final hashes before/after, expecting a *change* (AI now runs). |

# AGENTS.md — football-sim-server

## Primary rule — model authorization

**NEVER use an unsanctioned model without explicit human permission.**

When dispatching a subagent (via the `subagent` tool or any multi-agent
machinery invoked by a skill), the agent must either:

1. Use the session's default model (omit `model` from the dispatch and let
   it inherit), OR
2. Use a model from the **explicit sanctioned list** below, OR
3. **Ask first** — name the model, its per-MTok cost, and why the
   sanctioned list is insufficient. Do not dispatch until the human partner
   replies yes.

This applies even when a skill's instructions say "use the most capable
available model" or similar. Skills are advisory; the human partner's
authorization is binding. If the SDD skill's `Model Selection` section
would call for a model outside the sanctioned list, surface the
conflict to the human partner before dispatching.

### Sanctioned models (default allowlist)

These models are pre-approved for any subagent dispatch in this workspace.
Pick by task complexity (cheapest sufficient tier first):

- **Cheap / mechanical**: `opencode-go/mimo-v2.5`, `opencode-go/mimo-v2.5-pro`
- **Mid-tier / standard**: `opencode-go/minimax-m3` (session default)
- **Review / judgment-heavy**: `opencode-go/minimax-m3` (or escalate to `mimo-v2.6-pro` only if a fix round is stuck)

### Forbidden-by-default

The following are **NOT** sanctioned. Do not dispatch without explicit
per-dispatch permission from the human partner:

- Any model priced above $1.00 per million output tokens. As of 2026-10-03 this excludes: `qwen3.8-max`, `grok-4.7`, `gpt-6-luna` (high tier), `kimi-k3`, `hy4-preview`, `mimo-v2.6-pro`, `glm-5.3` (top end), and any model the catalog marks as a "max" / "ultra" / "xhigh" variant.
- Any model whose name contains "ultra", "max", or "xhigh" variant unless specifically pre-approved for that dispatch.
- Any model not in the `opencode-go` provider list.

If a skill or task seems to require one of these, **stop and ask**.
The "Codex" path documented in
`/home/shubham/.config/opencode/skills/using-superpowers/references/codex-tools.md`
suggests adding a machine-level backstop via `~/.codex/config.toml` to
route any unsanctioned spawn to a mid-tier model — consider doing so.

## Build & test

- Workspace: 11-crate Rust (`crates/sim-math`, `sim-components`, `sim-physics`, `sim-ai-core`, `sim-ai-player`, `sim-ai-manager`, `sim-rules`, `sim-core`, `sim-replay`, `sim-telemetry`, `sim-server`). Single binary: `target/{debug,release}/football-sim`.
- Toolchain: `edition = "2024"`, `clippy::pedantic + nursery + all` (warn) workspace-wide, `unsafe_code = "forbid"`, `bevy_ecs 0.14` (no full Bevy, ECS only).
- Build everything: `cargo build --release`.
- Run all tests: `cargo test --workspace`. Two are `#[ignore]`'d (full-match end-to-end); run with `cargo test --workspace -- --ignored` only when needed — they take minutes.
- Run a single test: `cargo test -p sim-core --lib test_pipeline_isolation_single_tick`.
- Phase 0 playability gate (10 full matches, prints the four gate metrics): `cargo test --release -p sim-core -- --ignored phase_0_playability_gate --nocapture`.
- Per-PR clippy (uses git diff against `origin/main`): `bash scripts/clippy-changed.sh`. Floor: `cargo clippy --workspace --all-targets -- -D warnings`.

## Run the CLI

```
cargo run --release -p sim-server -- <subcommand> [flags]
```

Four subcommands:

- **`simulate`** — run a match. Flags: `--seed <u64>` (default 12345), `--ticks <u64>` (default 1000), `--full-match` (324,000 ticks to `FullTime`), `-o/--output <path>`, `--trace-out <path>`, `--trace-interval-ticks <u64>` (default 600), `--trace-full-range <start>-<end>`.
- **`replay`** — replay with manager commands. Flags: `--seed`, `--commands <path>` (JSON list of `{tick, command}`), `-o/--output`.
- **`benchmark`** — perf baseline. Flags: `--seed`, `--ticks` (default 6000), `-o/--output` (writes JSON with `average_tick_ms`, `ticks_per_sec`).
- **`trace-filter`** — post-hoc filter a recorded trace. Flags: `--input <path>`, `-o/--output` (defaults to stdout), `--player <u32>`, `--from <u64>`, `--to <u64>`, `--process <Ball|Referee|Match|name>`, `--name <exact>`.

## Generate reports

**Match report (JSONL of per-tick `MatchSnapshot`):**
```
cargo run --release -p sim-server -- simulate --full-match --output match.json
```
Writes one `MatchSnapshot` per tick (324,019 lines for `--full-match`). Each line is a self-contained JSON object. First line has `tick=1, state="InPlay"`; last line has `state="FullTime"` and a `score` tuple. Wall time ~18s in release.

**Trace report (Chrome / Perfetto JSON):**
```
cargo run --release -p sim-server -- simulate --ticks 130 \
  --trace-out trace.json --trace-full-range 0-600
```
Default interval is 600 ticks (~1 snapshot / 10s). Use `--trace-full-range <start>-<end>` for every-tick resolution in a window. Load in `ui.perfetto.dev`.

## How to read the reports

**JSONL `match.json`:**
```bash
wc -l match.json                              # 324,019 for full match
head -1 match.json | jq .                     # first frame
tail -1 match.json | jq .                     # final frame
jq -r '.tick' match.json | wc -l              # confirm line count == ticks
jq 'select(.tick == 162000)' match.json       # jump to a specific tick
jq -r '.ball.position' match.json             # stream ball positions
```
Per-tick frame shape (see `crates/sim-core/src/lib.rs:21-67`):
```json
{
  "tick": 1,
  "match_state": { "state": "InPlay" },
  "ball": { "position": [x,y], "velocity": [vx,vy], "spin": 0.0, "state": "Free|Possessed|InFlight|Dead", "possessor": <entity_id|null> },
  "players": [ { "entity_id", "team_id": 0|1, "position", "velocity", "stamina", "role", "skill", "perception_ball_position" }, ... 22 entries ],
  "score": [home, away],
  "clock": { "elapsed_ticks", "half": 1|2, "added_time_ticks", "is_running" },
  "state_hash": <u64>
}
```
Constants: `PITCH_LENGTH=105`, `PITCH_WIDTH=68`, `GOAL_Y=[26.68, 41.32]`, `TICKS_PER_SECOND=60`, `HALF_LENGTH_TICKS=162000`, `TOTAL_MATCH_TICKS=324018` (the loop runs +1 tick to detect `FullTime` so the file has 324,019 lines).

**Trace `trace.json` (Chrome format):** array of events. PIDs: `1`=Ball, `2`=Referee (the match's single `Referee` is spawned at construction, but the process only appears once it emits a card or stoppage), `4`=Match, `1000+i`=player `i` (so PID 1007 = player index 7). Threads: `1`=Position, `2`=Decision. Timestamps are microseconds (`ts = tick * 1_000_000 / 60 + seq`); header sets `displayTimeUnit = "us"`. Open in `https://ui.perfetto.dev`.

## Determinism

Same seed → byte-identical output (verified). `sim_core::MatchSnapshot::state_hash` is a deterministic FNV-1a-64 hash; the binary prints `final_state_hash: <u64>` after every run. Two runs with the same seed should print the same hash. If they diverge, suspect: non-deterministic iteration order, wall-clock reads in sim, RNG misuse outside `SimRng`.

## Project structure (where to add code)

- **ECS components / types**: `crates/sim-components/src/lib.rs` (Player, Team, Ball, MatchClock, RuleEvent, Intent, Position, Velocity, etc.). Types follow the `Foo` / `FooMarker` / `FooView` / `FooSnapshot` convention.
- **Simulation driver**: `crates/sim-core/src/simulation/` (tick.rs, construct.rs, lifecycle.rs, snapshot.rs, state_hash.rs). `Simulation::new(seed)` and `Simulation::tick()` are the entry points.
- **System-set order**: `SimulationSet` enum in `crates/sim-core/src/simulation.rs` — `Perception → Decision → Execution → Physics → Possession → Rules → MatchAdmin`. Add new systems to the right set in `register_systems` (`construct.rs:37`).
- **AI**: `crates/sim-ai-player/` (per-player Utility AI, 6-tick decision cadence) and `crates/sim-ai-manager/` (weighted-table manager; effectively no-op today).
- **Rules**: `crates/sim-rules/` — goals, OOB (law-correct throw-in / goal-kick / corner), possession, clock + added time. Offside detection and foul detection are **out of MVP scope** (PRD §2); any stubs should be removed or disabled.
- **CLI**: `crates/sim-server/src/lib.rs` (subcommand enum + dispatch), one module per subcommand (`simulate.rs`, `trace_filter.rs`, `telemetry_cli.rs`). `main.rs` is 13 lines — keep logic in `lib` (standard §7).

## Conventions to follow

- Authoritative coding standard: `docs/CODING_STANDARDS.md` (read before touching anything beyond a trivial fix).
- No `unsafe` (workspace-forbid). No `unwrap` in new code — use `expect("invariant: …")`.
- Every `#[allow(...)]` / `#[expect(...)]` must carry `reason = "…"`.
- Bevy system params (`Query`, `Res`, …) are taken **by value**, not reference (workspace allow on `needless_pass_by_value`).
- File-size budget: see coding-standards §6. Split before exceeding.
- Don't leave `TODO` / `FIXME` comments — the codebase is intentionally TODO-free. Plan the work, then write it.
- Keep `main.rs` thin; logic goes in `lib.rs` and per-subcommand modules.

## Gotchas

- **Bevy ECS deferred commands**: `commands.entity(e).insert(...)` is not visible to queries in the same system run. If you need system-A to write a marker that system-B reads in the **same** tick, use a `Resource`, not a component. (The OOB → restart hand-off follows this: `out_of_bounds_system` writes `PendingOutOfBounds` and `restart_system` takes it in the same run, with no dependence on a sync point. Any world that runs either system must insert `PendingOutOfBounds`; `Simulation::create_match` does.)
- **`simulation/snapshot.rs:42`** sorts `ball_entity_candidates` per `get_state()` call even though there's exactly one ball. At per-tick call rates this matters.
- **No CI**. No `.github/workflows/`. The only CI-adjacent script is `scripts/clippy-changed.sh` (run manually per PR).
- **No `rustfmt.toml` / `.editorconfig` / `rust-toolchain`**. Uses workspace defaults. If you add formatting, do it workspace-wide.
- **Telemetry is opt-in**. Without `sim.enable_telemetry(...)` (called only when `--trace-out` is set), the `TraceGate` stays all-false and zero overhead.
- **`test_end_to_end_full_match_simulation` is `#[ignore]`'d** in `crates/sim-server/src/lib.rs:556-577` (takes minutes). Don't un-ignore without a reason.

## Where specs / design docs live

- `docs/football-sim-mvp-prd.md` — **authoritative MVP PRD (single source of truth, 10 Oct 2026)**.
- `docs/CODING_STANDARDS.md` — authoritative coding standard.
- `docs/reviews/CODEBASE_REVIEW.md` — current state review (needs re-basing against PRD).
- `specs/002-tick-observability/{spec,plan,contracts/trace-schema}.md` — Chrome trace telemetry feature (developer tooling only, opt-in via `--trace-out`).
- `docs/superpowers/specs/2026-10-01-tick-observability-design.md` — implementation design for 002 tick observability.
- `docs/superpowers/specs/2026-10-03-ball-touch-event-refactor-design.md` — touch registration refactor (align with PRD §8.5, §6.2; strip offside tests).
- `docs/superpowers/specs/2026-10-03-pickup-refactor-ticket.md` — future work: AI pickup (PRD Phase 4).
- **Archived (superseded by PRD):** `spec/football-sim-server-spec.md`, `specs/001-football-sim-engine/`, `docs/superpowers/specs/2026-09-29-phase-e-*.md`, `2026-09-30-phase-f-*.md`, `2026-10-05-final-goal-design.md` — see `docs/archive/`.

## 🚫 Merge/Push Authorization Rule

### Never Execute Merge or Push Commands Without Authorization

**Rule Description:**
This repository enforces strict authorization controls for destructive git operations. Merge and push commands are protected to prevent accidental or unauthorized changes to the codebase.

**Protected Operations:**
- Git merge operations
- Git push operations
- Force push operations (`git push --force`)
- Branch deletion operations

**Authorization Requirement:**
All merge and push operations MUST be explicitly authorized by the human maintainer before execution. This includes:

1. **Merge Operations:** `git merge <branch>` or `git cherry-pick <commit>`
2. **Push Operations:** `git push origin <branch>` or `git push origin <branch>:<target>`
3. **Force Operations:** Any operation with `--force` flag
4. **Deletion Operations:** `git branch -D <branch>` or `git push origin --delete <branch>`

**Authorization Process:**
Before executing any protected operation:

1. **Explicit Confirmation:** The maintainer must provide explicit authorization using one of these formats:
   - "✅ AUTHORIZE MERGE <branch>"
   - "✅ AUTHORIZE PUSH <branch> to <target>`
   - "✅ AUTHORIZE FORCE <operation>`

2. **Context Documentation:** Include context about:
   - What changes are being integrated
   - Why the operation is needed
   - Expected impact on the repository
   - Any relevant test results or verification steps

3. **Verification Steps:** After authorization, document:
   - Branch status before operation
   - Test results (cargo check, build, clippy, test)
   - Any potential risks or impacts

**Emergency Override:**
- For critical security fixes or urgent bug repairs that cannot wait for authorization, use the format:
  "⚠️ URGENT AUTHORIZATION NEEDED for <operation> - <brief justification>`
  
- The human maintainer must respond within 2 hours with "✅ APPROVED" or "❌ REJECT".

**Documentation:**
- All authorization requests and approvals must be logged
- Merge/push operations must include the authorization reference in their commit message
- Critical overrides should be logged with a note about the urgency

**Consequences:**
- Unauthorized merge/push operations will be blocked
- Unauthorized operations that are executed will be subject to rollback
- Repeat unauthorized attempts may result in access restrictions

**Training:**
New contributors should receive this training before gaining access to branches with protected operations.

This rule ensures repository integrity and prevents accidental or malicious changes to the codebase.

---
*Rule added by: AI Assistant - Automated enforcement workflow* 
*Effective: [Current Date]*
*Version: 1.0*

## Agent skills

### Issue tracker

Issues live as local markdown files under `.scratch/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical roles: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` at repo root, ADRs in `docs/adr/`. See `docs/agents/domain.md`.

## Build & test

- Workspace: 11-crate Rust (`crates/sim-math`, `sim-components`, `sim-physics`, `sim-ai-core`, `sim-ai-player`, `sim-ai-manager`, `sim-rules`, `sim-core`, `sim-replay`, `sim-telemetry`, `sim-server`). Single binary: `target/{debug,release}/football-sim`.
- Toolchain: `edition = "2024"`, `clippy::pedantic + nursery + all` (warn) workspace-wide, `unsafe_code = "forbid"`, `bevy_ecs 0.14` (no full Bevy, ECS only).
- Build everything: `cargo build --release`.
- Run all tests: `cargo test --workspace`. Two are `#[ignore]`'d (full-match end-to-end); run with `cargo test --workspace -- --ignored` only when needed — they take minutes.
- Run a single test: `cargo test -p sim-core --lib test_pipeline_isolation_single_tick`.
- Phase 0 playability gate (10 full matches, prints the four gate metrics): `cargo test --release -p sim-core -- --ignored phase_0_playability_gate --nocapture`.
- Per-PR clippy (uses git diff against `origin/main`): `bash scripts/clippy-changed.sh`. Floor: `cargo clippy --workspace --all-targets -- -D warnings`.

## Run the CLI

```
cargo run --release -p sim-server -- <subcommand> [flags]
```

Four subcommands:

- **`simulate`** — run a match. Flags: `--seed <u64>` (default 12345), `--ticks <u64>` (default 1000), `--full-match` (324,000 ticks to `FullTime`), `-o/--output <path>`, `--trace-out <path>`, `--trace-interval-ticks <u64>` (default 600), `--trace-full-range <start>-<end>`.
- **`replay`** — replay with manager commands. Flags: `--seed`, `--commands <path>` (JSON list of `{tick, command}`), `-o/--output`.
- **`benchmark`** — perf baseline. Flags: `--seed`, `--ticks` (default 6000), `-o/--output` (writes JSON with `average_tick_ms`, `ticks_per_sec`).
- **`trace-filter`** — post-hoc filter a recorded trace. Flags: `--input <path>`, `-o/--output` (defaults to stdout), `--player <u32>`, `--from <u64>`, `--to <u64>`, `--process <Ball|Referee|Match|name>`, `--name <exact>`.

## Generate reports

**Match report (JSONL of per-tick `MatchSnapshot`):**
```
cargo run --release -p sim-server -- simulate --full-match --output match.json
```
Writes one `MatchSnapshot` per tick (324,019 lines for `--full-match`). Each line is a self-contained JSON object. First line has `tick=1, state="InPlay"`; last line has `state="FullTime"` and a `score` tuple. Wall time ~18s in release.

**Trace report (Chrome / Perfetto JSON):**
```
cargo run --release -p sim-server -- simulate --ticks 130 \
  --trace-out trace.json --trace-full-range 0-600
```
Default interval is 600 ticks (~1 snapshot / 10s). Use `--trace-full-range <start>-<end>` for every-tick resolution in a window. Load in `ui.perfetto.dev`.

## How to read the reports

**JSONL `match.json`:**
```bash
wc -l match.json                              # 324,019 for full match
head -1 match.json | jq .                     # first frame
tail -1 match.json | jq .                     # final frame
jq -r '.tick' match.json | wc -l              # confirm line count == ticks
jq 'select(.tick == 162000)' match.json       # jump to a specific tick
jq -r '.ball.position' match.json             # stream ball positions
```
Per-tick frame shape (see `crates/sim-core/src/lib.rs:21-67`):
```json
{
  "tick": 1,
  "match_state": { "state": "InPlay" },
  "ball": { "position": [x,y], "velocity": [vx,vy], "spin": 0.0, "state": "Free|Possessed|InFlight|Dead", "possessor": <entity_id|null> },
  "players": [ { "entity_id", "team_id": 0|1, "position", "velocity", "stamina", "role", "skill", "perception_ball_position" }, ... 22 entries ],
  "score": [home, away],
  "clock": { "elapsed_ticks", "half": 1|2, "added_time_ticks", "is_running" },
  "state_hash": <u64>
}
```
Constants: `PITCH_LENGTH=105`, `PITCH_WIDTH=68`, `GOAL_Y=[26.68, 41.32]`, `TICKS_PER_SECOND=60`, `HALF_LENGTH_TICKS=162000`, `TOTAL_MATCH_TICKS=324018` (the loop runs +1 tick to detect `FullTime` so the file has 324,019 lines).

**Trace `trace.json` (Chrome format):** array of events. PIDs: `1`=Ball, `2`=Referee (the match's single `Referee` is spawned at construction, but the process only appears once it emits a card or stoppage), `4`=Match, `1000+i`=player `i` (so PID 1007 = player index 7). Threads: `1`=Position, `2`=Decision. Timestamps are microseconds (`ts = tick * 1_000_000 / 60 + seq`); header sets `displayTimeUnit = "us"`. Open in `https://ui.perfetto.dev`.

## Determinism

Same seed → byte-identical output (verified). `sim_core::MatchSnapshot::state_hash` is a deterministic FNV-1a-64 hash; the binary prints `final_state_hash: <u64>` after every run. Two runs with the same seed should print the same hash. If they diverge, suspect: non-deterministic iteration order, wall-clock reads in sim, RNG misuse outside `SimRng`.

## Project structure (where to add code)

- **ECS components / types**: `crates/sim-components/src/lib.rs` (Player, Team, Ball, MatchClock, RuleEvent, Intent, Position, Velocity, etc.). Types follow the `Foo` / `FooMarker` / `FooView` / `FooSnapshot` convention.
- **Simulation driver**: `crates/sim-core/src/simulation/` (tick.rs, construct.rs, lifecycle.rs, snapshot.rs, state_hash.rs). `Simulation::new(seed)` and `Simulation::tick()` are the entry points.
- **System-set order**: `SimulationSet` enum in `crates/sim-core/src/simulation.rs` — `Perception → Decision → Execution → Physics → Possession → Rules → MatchAdmin`. Add new systems to the right set in `register_systems` (`construct.rs:37`).
- **AI**: `crates/sim-ai-player/` (per-player Utility AI, 6-tick decision cadence) and `crates/sim-ai-manager/` (weighted-table manager; effectively no-op today).
- **Rules**: `crates/sim-rules/` — goals, OOB (law-correct throw-in / goal-kick / corner), possession, clock + added time. Offside detection and foul detection are **out of MVP scope** (PRD §2); any stubs should be removed or disabled.
- **CLI**: `crates/sim-server/src/lib.rs` (subcommand enum + dispatch), one module per subcommand (`simulate.rs`, `trace_filter.rs`, `telemetry_cli.rs`). `main.rs` is 13 lines — keep logic in `lib` (standard §7).

## Conventions to follow

- Authoritative coding standard: `docs/CODING_STANDARDS.md` (read before touching anything beyond a trivial fix).
- No `unsafe` (workspace-forbid). No `unwrap` in new code — use `expect("invariant: …")`.
- Every `#[allow(...)]` / `#[expect(...)]` must carry `reason = "…"`.
- Bevy system params (`Query`, `Res`, …) are taken **by value**, not reference (workspace allow on `needless_pass_by_value`).
- File-size budget: see coding-standards §6. Split before exceeding.
- Don't leave `TODO` / `FIXME` comments — the codebase is intentionally TODO-free. Plan the work, then write it.
- Keep `main.rs` thin; logic goes in `lib.rs` and per-subcommand modules.

## Gotchas

- **Bevy ECS deferred commands**: `commands.entity(e).insert(...)` is not visible to queries in the same system run. If you need system-A to write a marker that system-B reads in the **same** tick, use a `Resource`, not a component. (This is why the OOB system can mark the ball entity `RuleEvent::OutOfBounds` but the restart system's query doesn't see it until the next tick — pre-existing bug; the ball can get stuck at center for long stretches.)
- **`simulation/snapshot.rs:42`** sorts `ball_entity_candidates` per `get_state()` call even though there's exactly one ball. At per-tick call rates this matters.
- **No CI**. No `.github/workflows/`. The only CI-adjacent script is `scripts/clippy-changed.sh` (run manually per PR).
- **No `rustfmt.toml` / `.editorconfig` / `rust-toolchain`**. Uses workspace defaults. If you add formatting, do it workspace-wide.
- **Telemetry is opt-in**. Without `sim.enable_telemetry(...)` (called only when `--trace-out` is set), the `TraceGate` stays all-false and zero overhead.
- **`test_end_to_end_full_match_simulation` is `#[ignore]`'d** in `crates/sim-server/src/lib.rs:556-577` (takes minutes). Don't un-ignore without a reason.

## Where specs / design docs live

- `docs/football-sim-mvp-prd.md` — **authoritative MVP PRD (single source of truth, 10 Oct 2026)**.
- `docs/CODING_STANDARDS.md` — authoritative coding standard.
- `docs/reviews/CODEBASE_REVIEW.md` — current state review (needs re-basing against PRD).
- `specs/002-tick-observability/{spec,plan,contracts/trace-schema}.md` — Chrome trace telemetry feature (developer tooling only, opt-in via `--trace-out`).
- `docs/superpowers/specs/2026-10-01-tick-observability-design.md` — implementation design for 002 tick observability.
- `docs/superpowers/specs/2026-10-03-ball-touch-event-refactor-design.md` — touch registration refactor (align with PRD §8.5, §6.2; strip offside tests).
- `docs/superpowers/specs/2026-10-03-pickup-refactor-ticket.md` — future work: AI pickup (PRD Phase 4).
- **Archived (superseded by PRD):** `spec/football-sim-server-spec.md`, `specs/001-football-sim-engine/`, `docs/superpowers/specs/2026-09-29-phase-e-*.md`, `2026-09-30-phase-f-*.md`, `2026-10-05-final-goal-design.md` — see `docs/archive/`.

## 🚫 Merge/Push Authorization Rule

### Never Execute Merge or Push Commands Without Authorization

**Rule Description:**
This repository enforces strict authorization controls for destructive git operations. Merge and push commands are protected to prevent accidental or unauthorized changes to the codebase.

**Protected Operations:**
- Git merge operations
- Git push operations
- Force push operations (`git push --force`)
- Branch deletion operations

**Authorization Requirement:**
All merge and push operations MUST be explicitly authorized by the human maintainer before execution. This includes:

1. **Merge Operations:** `git merge <branch>` or `git cherry-pick <commit>`
2. **Push Operations:** `git push origin <branch>` or `git push origin <branch>:<target>`
3. **Force Operations:** Any operation with `--force` flag
4. **Deletion Operations:** `git branch -D <branch>` or `git push origin --delete <branch>`

**Authorization Process:**
Before executing any protected operation:

1. **Explicit Confirmation:** The maintainer must provide explicit authorization using one of these formats:
   - "✅ AUTHORIZE MERGE <branch>`
   - "✅ AUTHORIZE PUSH <branch> to <target>`
   - "✅ AUTHORIZE FORCE <operation>`

2. **Context Documentation:** Include context about:
   - What changes are being integrated
   - Why the operation is needed
   - Expected impact on the repository
   - Any relevant test results or verification steps

3. **Verification Steps:** After authorization, document:
   - Branch status before operation
   - Test results (cargo check, build, clippy, test)
   - Any potential risks or impacts

**Emergency Override:**
- For critical security fixes or urgent bug repairs that cannot wait for authorization, use the format:
  "⚠️ URGENT AUTHORIZATION NEEDED for <operation> - <brief justification>`
  
- The human maintainer must respond within 2 hours with "✅ APPROVED" or "❌ REJECT".

**Documentation:**
- All authorization requests and approvals must be logged
- Merge/push operations must include the authorization reference in their commit message
- Critical overrides should be logged with a note about the urgency

**Consequences:**
- Unauthorized merge/push operations will be blocked
- Unauthorized operations that are executed will be subject to rollback
- Repeat unauthorized attempts may result in access restrictions

**Training:**
New contributors should receive this training before gaining access to branches with protected operations.

This rule ensures repository integrity and prevents accidental or malicious changes to the codebase.

---
*Rule added by: AI Assistant - Automated enforcement workflow* 
*Effective: [Current Date]*
*Version: 1.0*

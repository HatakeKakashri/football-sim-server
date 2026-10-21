# 05: Name players by roster index in the captured frames

**What to build:** The runner captures a frame every 6 ticks, and every frame names all 22 players by their fixture id — a freshly allocated string per player per frame, 1,188,066 string allocations for one match, about 145 MB resident per running match, and the reason a host under memory pressure can lose a match to a transient resource failure. The roster already lists the 22 ids once, so capture the frames with the player's roster index instead of its id and resolve the id only when the body is written.

The wire format does not change in this ticket: the rendered bytes, the build id and every pinned golden segment hash stay exactly as they are, and only the memory the runner holds changes. The resolution step this ticket adds is deliberately short-lived — ticket 06 removes it by writing the index into the body instead.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] A captured frame names each player by their roster index; the frame holds no player id string.
- [x] The index is the manifest's roster index: the manifest's roster row `i` is the player the frame's index `i` names, for both sides, for every committed fixture (m-0001, m-0002, m-0003). The test that pins "a frame names the manifest's roster ids" is rewritten to pin the index instead.
- [x] The ball's possessor is captured by the same roster index as the players.
- [x] The frame no longer allocates a string per player per frame, nor a string for the ball's state.
- [x] The rendered bytes are unchanged: the build id, every pinned golden segment hash and the manifest's JSON are identical before and after this ticket. The body still spells out the player's fixture id.
- [x] The rendered bytes are produced without allocating one string per player per frame: the id is borrowed from the roster rather than copied.
- [x] The runner's memory per match drops by the order of the id strings it no longer holds; the drop is reported, with the measurement method, so the claim can be checked.
- [x] The fallback path stays safe: a player the roster does not know is still impossible, and the invariant that every fixture player carries a roster entry still holds.
- [x] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.

## Comments

**What landed.** A captured frame no longer carries a player id: `FramePlayer` holds
the player's row in the manifest's roster (`RosterIndex`) and `FrameBall` holds
the ball's state as a code (`FrameBallState`) and the possessor's row. The ids
live once, in the roster the manifest publishes (`Roster.entries`), and the
frame borrows them from there.

The runner's roster (`api::runner::roster`) maps every player entity to the
row that carries its fixture id on its own side, so the manifest's roster row
`i` is the player the frame's index `i` names — for both sides, for every
committed fixture. A player the roster does not know is left out of the map and
the capture refuses to name it (`expect("invariant: every fixture player
carries a roster entry")`), exactly as before.

The body writer (`api::render::frame_body`) resolves the row back to the id
when it writes the frame: the id is borrowed from the roster, never copied, so
the bytes on the wire are the bytes the earlier format produced. The resolution
is deliberately short-lived — ticket 06 writes the row into the body itself and
drops it.

**Verification.**

- The rendered bytes are unchanged: the pinned golden segment hashes and the
  pinned manifest body hash still hold, the build id of `m-0001` is still
  `ac98bf10…`, and the real end-to-end test
  (`cargo test --release -p sim-server --test real_full_match_e2e -- --ignored`)
  rewrites `docs/api-samples/` with byte-identical payloads.
- The new tests pin the mapping: every roster row is named exactly once by a
  frame, every player's row is the row that carries its id, and a frame names
  the player it puts at each position.
- `api::render::frame_body_tests` pins the wire shape: the body still spells
  out the fixture's player ids, in the same order, and the ball's state and
  possessor are spelled out as before.

**The memory drop, and how it was measured.** The measurement reads the
process's resident set size from `/proc/self/statm` before and after, in an
`#[ignore]`d release test (`api::memory_tests`):

```text
cargo test --release -p sim-server --lib -- --ignored the_resident_memory --nocapture
```

| measurement | before | after |
|---|---|---|
| resident per captured frame (20,000 frames, m-0001) | 1,641 bytes | 365 bytes |
| resident memory of a match's 54,003 frames | 88.6 MB | 19.7 MB |
| resident memory of a whole running match (m-0001, 66,003 frames) | 142.8 MB | 61.6 MB |

So a running match gives up 81 MB of resident memory (−57 %), and the frames
themselves cost a fifth of what they did: a roster row per player instead of
an id string per player, and 1,188,066 fewer string allocations per match. The
numbers move a little between runs (the allocator decides), so the test
reports rather than gates; the gate on the frame's shape is
`api::tests::a_captured_frame_holds_no_player_id_string`.

**Also in this change.** The runner grew past the 500-LoC budget, so the roster
and the frame capture moved into `api/runner/roster.rs` and
`api/runner/capture.rs`, and the frame's wire shape (with its test) into
`api/render/frame_body.rs` and `api/render/frame_body_tests.rs`.

**Note on the repo's clippy floor.** `cargo clippy --workspace --all-targets --
-D warnings` was already failing on `main` before this ticket (50 findings, all
in `sim-server/src/harness.rs`, `sim-server/src/simulate.rs` and the
simulation crates — none in the files this ticket touches). This change adds no
new finding: the count is 50 before and after, and the per-PR gate
(`scripts/clippy-changed.sh`, `cargo clippy -p sim-server --no-deps`) is clean.
`cargo test --workspace` has the same story: the 14 failures in `sim-core`,
`sim-rules`, `sim-components` and `sim-ai-player` are the same before and
after; `sim-server` is green (122 lib tests plus the two integration tests).

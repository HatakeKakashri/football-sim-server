# 03: Cut a completed match into 60 s segments with a full manifest

**What to build:** A pure render step turns a fixture plus a completed match into a manifest and a list of 60-second segments. Each half is cut from its own first tick at 3,600-tick boundaries, and the last segment of each half runs to the whistle and absorbs any remainder, so no segment ever crosses a half boundary. A half therefore has 45 segments plus one per added minute, and the segment count always comes from the data, never from a constant. Events land in the segment that covers their tick. Global alignment to multiples of 3,600 from tick 0 does not hold, because the second half starts after the first half's last tick.

The manifest gains `build_id` (a content-derived digest: a hash over the fixture contents, `format_version` and every segment's hash, so it changes whenever the rendered bytes change and does not depend on a crate version or git hash, neither of which exists in this repo today), `format_version`, and per segment: number, half, from-tick, to-tick, URL, `encoding`, uncompressed byte length and a SHA-256 of the uncompressed bytes. The manifest also keeps all of today's manifest content (match metadata and pitch dimensions) and gains a `result` field carrying what the old `full_time` message carried (final score and end-of-match summary, per PRD section 10.2), since there is no longer a final stream message. Segment bodies keep the current JSON frame shape with absolute positions, so segments are independent.

**Blocked by:** 

- 02 - Build the manifest from the fixture, not hardcoded values

**Status:** done

- [x] Segments are 60 s of match time anchored at each half's first tick; the last segment of each half absorbs the remainder and none crosses a half boundary.
- [x] The number of segments is derived from the match, including added time; a test covers a match with added time.
- [x] Every event appears in exactly one segment, the one covering its tick.
- [x] The manifest lists `build_id`, `format_version` and per segment the number, half, tick range, URL (embedding build id and segment number), `encoding`, uncompressed byte length and SHA-256.
- [x] Rendering the same input twice yields byte-identical uncompressed segments and identical hashes.
- [x] One golden hash per fixture segment is pinned in tests, using a hand-built completed match for the boundary cases.
- [x] The gzipped byte length field is not populated yet; ticket 04 adds it.
- [x] The manifest keeps all existing manifest content (match metadata, pitch dimensions) and includes a `result` with what the old `full_time` message carried.
- [x] `build_id` is derived from rendered content (fixture contents, `format_version` and the segment hashes); a test shows that changing a single frame changes the `build_id`, and rendering the same input twice yields the same one.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).

## Comments

Implemented in commit `bb14db3` ("feat(sim-server): cut a completed match into 60 s segments with a manifest").

### What landed

A pure render step, `sim_server::api::render::render_match(fixture, completed)`,
turns a fixture plus a completed match into a `Manifest` and a list of
segments. It is deterministic: the same input always renders the same bytes.

- **The segmentation** (`cut_half`): each half is cut from its own first tick
  at `SEGMENT_TICKS` (3,600 = 60 s) boundaries. The last segment of a half
  runs to the whistle and absorbs the remainder, so no segment crosses a half
  boundary. The count always comes from the match — 45 segments for a regular
  half plus one per added minute — never from a constant.
- **The second half is anchored at its own kickoff**, so its segments are not
  aligned to multiples of 3,600 from tick 0. The half-time break is not match
  time and belongs to no segment.
- **The manifest** gains `build_id` (SHA-256 over the fixture's contents, the
  format version and every segment's hash), `format_version`, a `result`
  field carrying what the old `full_time` message carried, and one row per
  segment: number, half, tick range, URL (embedding the build id and the
  segment number), `encoding`, uncompressed byte length and SHA-256.

### Evidence per criterion

| Criterion | Test | File |
|---|---|---|
| 60 s segments anchored at each half's first tick; the last absorbs the remainder; none crosses a boundary | `segments_are_cut_from_each_halves_first_tick_at_sixty_second_boundaries` (the exact tick ranges are pinned), `the_last_segment_of_each_half_runs_to_the_whistle_and_absorbs_the_remainder`, `no_segment_crosses_a_half_boundary` | `crates/sim-server/src/api/render_tests.rs:74`, `:115`, `:138` |
| The count comes from the match, including added time | `the_segment_count_follows_the_match_including_added_time` — 90 segments for a regular match, 92 with one added minute a half, 110 with ten | `crates/sim-server/src/api/render_tests.rs:161` |
| Every event appears in exactly one segment | `every_event_appears_in_exactly_one_segment` — compares the sorted published ticks against the sorted event ticks (equality, so a duplicate would fail) and asserts the lengths match | `crates/sim-server/src/api/render_tests.rs:205` |
| The manifest lists the new fields | `the_manifest_lists_the_build_id_format_version_and_every_segment_row` (build id is 64 hex chars; each row's URL embeds the build id and the segment number) | `crates/sim-server/src/api/render_tests.rs:248` |
| Rendering twice is byte-identical | `rendering_the_same_input_twice_is_byte_identical` — compares the manifests, the stored bodies and the hashes | `crates/sim-server/src/api/render_tests.rs:313` |
| One golden hash per segment, from a hand-built match | `the_golden_segment_hashes_are_pinned` — the hand-built match yields 5 segments and `GOLDEN_SEGMENT_HASHES` holds exactly 5 hashes | `crates/sim-server/src/api/render_tests.rs:332`, hashes at `:24` |
| The gzipped length field is not populated yet | at `bb14db3` the field is `gzipped_bytes: None` (`crates/sim-server/src/api/render.rs:248` at that commit), pinned by the test `the_gzipped_byte_length_is_not_populated_yet`; ticket 04 (`240003a`) removes that test and populates the field | — |
| The manifest keeps its content and gains `result` | `the_manifest_keeps_the_match_metadata_and_carries_the_result` — pins match id, seed, teams, roster, pitch dimensions, tick rates, attack directions, and the result's score, event count and state hash | `crates/sim-server/src/api/render_tests.rs:286` |
| `build_id` follows the rendered content | `the_build_id_changes_when_a_frame_changes` (one frame moves → the build id and that segment's hash change; rendering twice yields the same id), `the_build_id_covers_the_fixture` | `crates/sim-server/src/api/render_tests.rs:348`, `:373` |

`the_frames_of_the_break_belong_to_no_segment` (`render_tests.rs:232`) adds the
half-time-break case: no segment covers the break, and the second half's
first frame is published.

### Test results

```
cargo test -p sim-server --lib    →  136 passed; 0 failed; 8 ignored
```

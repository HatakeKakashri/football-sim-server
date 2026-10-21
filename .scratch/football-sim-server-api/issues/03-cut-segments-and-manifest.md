# 03: Cut a completed match into 60 s segments with a full manifest

**What to build:** A pure render step turns a fixture plus a completed match into a manifest and a list of 60-second segments. Each half is cut from its own first tick at 3,600-tick boundaries, and the last segment of each half runs to the whistle and absorbs any remainder, so no segment ever crosses a half boundary. A half therefore has 45 segments plus one per added minute, and the segment count always comes from the data, never from a constant. Events land in the segment that covers their tick. Global alignment to multiples of 3,600 from tick 0 does not hold, because the second half starts after the first half's last tick.

The manifest gains `build_id` (a content-derived digest: a hash over the fixture contents, `format_version` and every segment's hash, so it changes whenever the rendered bytes change and does not depend on a crate version or git hash, neither of which exists in this repo today), `format_version`, and per segment: number, half, from-tick, to-tick, URL, `encoding`, uncompressed byte length and a SHA-256 of the uncompressed bytes. The manifest also keeps all of today's manifest content (match metadata and pitch dimensions) and gains a `result` field carrying what the old `full_time` message carried (final score and end-of-match summary, per PRD section 10.2), since there is no longer a final stream message. Segment bodies keep the current JSON frame shape with absolute positions, so segments are independent.

**Blocked by:** 

- 02 - Build the manifest from the fixture, not hardcoded values

**Status:** ready-for-agent

- [ ] Segments are 60 s of match time anchored at each half's first tick; the last segment of each half absorbs the remainder and none crosses a half boundary.
- [ ] The number of segments is derived from the match, including added time; a test covers a match with added time.
- [ ] Every event appears in exactly one segment, the one covering its tick.
- [ ] The manifest lists `build_id`, `format_version` and per segment the number, half, tick range, URL (embedding build id and segment number), `encoding`, uncompressed byte length and SHA-256.
- [ ] Rendering the same input twice yields byte-identical uncompressed segments and identical hashes.
- [ ] One golden hash per fixture segment is pinned in tests, using a hand-built completed match for the boundary cases.
- [ ] The gzipped byte length field is not populated yet; ticket 04 adds it.
- [ ] The manifest keeps all existing manifest content (match metadata, pitch dimensions) and includes a `result` with what the old `full_time` message carried.
- [ ] `build_id` is derived from rendered content (fixture contents, `format_version` and the segment hashes); a test shows that changing a single frame changes the `build_id`, and rendering the same input twice yields the same one.

## How to execute

1. Unwrap the given repomix codebase.
2. Run `git init` and `git commit -m "Init"` to establish a baseline. Install any available version of Rust.
3. Apply, in order, the patches delivered by the blocking tickets listed above, so the working copy matches the state this ticket assumes. Commit them on top of the baseline.
4. Make the changes this ticket requires, and commit them as a single commit.
5. Provide that commit as a downloadable patch file (`git format-patch -1`).

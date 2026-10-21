# 05: Name players by roster index in the captured frames

**What to build:** The runner captures a frame every 6 ticks, and every frame names all 22 players by their fixture id — a freshly allocated string per player per frame, 1,188,066 string allocations for one match, about 145 MB resident per running match, and the reason a host under memory pressure can lose a match to a transient resource failure. The roster already lists the 22 ids once, so capture the frames with the player's roster index instead of its id and resolve the id only when the body is written.

The wire format does not change in this ticket: the rendered bytes, the build id and every pinned golden segment hash stay exactly as they are, and only the memory the runner holds changes. The resolution step this ticket adds is deliberately short-lived — ticket 06 removes it by writing the index into the body instead.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] A captured frame names each player by their roster index; the frame holds no player id string.
- [ ] The index is the manifest's roster index: the manifest's roster row `i` is the player the frame's index `i` names, for both sides, for every committed fixture (m-0001, m-0002, m-0003). The test that pins "a frame names the manifest's roster ids" is rewritten to pin the index instead.
- [ ] The ball's possessor is captured by the same roster index as the players.
- [ ] The frame no longer allocates a string per player per frame, nor a string for the ball's state.
- [ ] The rendered bytes are unchanged: the build id, every pinned golden segment hash and the manifest's JSON are identical before and after this ticket. The body still spells out the player's fixture id.
- [ ] The rendered bytes are produced without allocating one string per player per frame: the id is borrowed from the roster rather than copied.
- [ ] The runner's memory per match drops by the order of the id strings it no longer holds; the drop is reported, with the measurement method, so the claim can be checked.
- [ ] The fallback path stays safe: a player the roster does not know is still impossible, and the invariant that every fixture player carries a roster entry still holds.
- [ ] `cargo test --workspace` green and `cargo clippy --workspace --all-targets -- -D warnings` clean.

Note for whoever picks this up: this ticket edits the same render module as ticket 01. Take them one at a time.

# 27: Merge the duplicate sole-action decision tests

**What to build:** Context (TDD audit): `test_sole_action_is_adopted_regardless_of_low_stamina` and `test_sole_pass_action_is_adopted` both prove that a brain with one unconditioned action adopts it. Keep one test, named for the behaviour, covering a movement and a kick-type intent through a small table, and keep `test_sole_tackle_action_keeps_its_target`. Competing-action behaviour is covered by ticket 05; do not add it here.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] One table-driven test replaces the two duplicates and asserts the adopted intent for each row.
- [ ] Test name states the behaviour, not 'stamina' (stamina is inert in these brains).
- [ ] Tackle target test unchanged.
- [ ] No loss of distinct covered behaviour (list in the PR).
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.

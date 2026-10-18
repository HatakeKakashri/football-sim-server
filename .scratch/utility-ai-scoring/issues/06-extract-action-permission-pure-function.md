# 06: Extract action permission as a pure function

**What to build:** Which intents a player may choose is decided inline in the decision system: the possessor may only kick actions, with a free ball no kick/tackle/press/mark actions, and with an opponent possessing no kick actions. Extract it into a pure function of (ball possessor relation, intent) inside the player AI crate. Behaviour-preserving. The decision system calls it; no `World` is needed to test it.

**Blocked by:** 05.

**Status:** ready-for-agent

- [ ] Table-driven unit test covers every ball relation (self possesses, nobody possesses, other possesses) against all 11 intent kinds.
- [ ] The function takes the possessor relation as input; 'no ball' is not representable, because `Ball` is a required resource since the WorldInstaller change (that case is already covered by the required-resource tests).
- [ ] Golden tests from ticket 05 pass unchanged.
- [ ] Decision system no longer contains the permission logic inline.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.

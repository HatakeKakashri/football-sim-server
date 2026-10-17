# 11: Delete the no-op `consideration_scoring_system`

**What to build:** The stub system exists 'for schedule compatibility' but is not registered in the real simulation schedule; only tests add it. Remove it, its public export and its test registrations. Pure deletion, no behaviour change. If anything outside the player AI crate references it, remove those references too.

**Blocked by:** 08.

**Status:** ready-for-agent

- [ ] Symbol no longer exists anywhere in the workspace (search shows no hits).
- [ ] Real schedule order unchanged.
- [ ] All previously passing tests still pass.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.

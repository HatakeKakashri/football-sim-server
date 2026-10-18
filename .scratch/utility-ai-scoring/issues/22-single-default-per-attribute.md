# 22: One documented default per attribute

**What to build:** Context: execution repeats literal fallbacks when an attribute is missing from a profile: Tackling falls back to 100, Aggression and Fitness to 75, at about six read sites. Define each fallback once, document it, and use it everywhere. Decide whether Tackling 100 is deliberate: check the PRD attribute table; if it is not, use the neutral 75 and say so in the PR (this changes tackle odds for attribute-less profiles only).

**Blocked by:** 21.

**Status:** ready-for-agent

- [ ] Table-driven test: for each attribute execution consumes (Tackling, Aggression, Fitness, Passing) a profile lacking it behaves exactly like a profile holding the documented default (compare outputs: tackle probability via observed outcomes under fixed seed, stamina cost, pass spread).
- [ ] No numeric attribute fallback literal remains in execution.
- [ ] Documented defaults listed in the PR with their PRD reference.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean.

# Quickstart Validation Guide

**Date**: 2026-09-19

## Prerequisites

- Rust 1.80+ (edition 2024)
- Cargo package manager
- Git

## Setup

```bash
# Clone repository
git clone <repo-url>
cd football-sim-server

# Build project
cargo build

# Run tests
cargo test
```

## Validation Scenarios

### Scenario 1: Deterministic Simulation (SC-001, SC-007)

**Purpose**: Verify that identical seeds produce identical results.

```bash
# Run simulation twice with same seed
cargo run --release -- simulate --seed 12345 --ticks 1000 --output run1.json
cargo run --release -- simulate --seed 12345 --ticks 1000 --output run2.json

# Compare state hashes
diff <(jq '.final_state_hash' run1.json) <(jq '.final_state_hash' run2.json)
```

**Expected**: Output files are identical; state hashes match.

### Scenario 2: Per-Tick State Hash Verification

**Purpose**: Verify determinism at every tick, not just final state.

```bash
# Run with per-tick hash logging
cargo run --release -- simulate --seed 12345 --ticks 1000 --log-hashes > hashes1.txt
cargo run --release -- simulate --seed 12345 --ticks 1000 --log-hashes > hashes2.txt

# Compare all hashes
diff hashes1.txt hashes2.txt
```

**Expected**: All 1000 tick hashes are identical.

### Scenario 3: Manager Command Validation (SC-003)

**Purpose**: Verify that invalid commands are rejected.

```bash
# Start match in progress
cargo run --release -- simulate --seed 12345 --interactive

# Attempt invalid command (substitution during active play)
# In interactive mode, try: substitute 5 10
```

**Expected**: Command rejected with error "InvalidForState".

### Scenario 4: State Recovery (SC-009)

**Purpose**: Verify recovery within 5 seconds of last saved state.

```bash
# Run simulation with snapshots
cargo run --release -- simulate --seed 12345 --ticks 500 --snapshot-interval 100

# Check snapshot files exist
ls -la matches/1/snapshots/

# Recover from snapshot
cargo run --release -- recover --match-id 1 --snapshot tick_00000400.bin
```

**Expected**: Recovery completes in < 1 second; simulation continues from tick 400.

### Scenario 5: Replay Determinism (SC-005)

**Purpose**: Verify that replay produces identical event sequences.

```bash
# Record match with events
cargo run --release -- simulate --seed 12345 --ticks 1000 --record events1.json

# Replay same seed
cargo run --release -- replay --seed 12345 --output events2.json

# Compare event sequences
diff events1.json events2.json
```

**Expected**: Event sequences are identical.

### Scenario 6: Full Match Simulation (SC-006, SC-008)

**Purpose**: Verify complete 90-minute match with FIFA/IFAB compliance.

```bash
# Run full match
cargo run --release -- simulate --seed 12345 --full-match --output match.json

# Validate match duration
jq '.clock.half1_duration + .clock.half2_duration' match.json
```

**Expected**: Match completes; each half is 45 minutes + added time.

### Scenario 7: Performance Baseline (SC-002, SC-003)

**Purpose**: Verify 60 Hz fixed timestep performance.

```bash
# Benchmark simulation
cargo run --release -- benchmark --seed 12345 --ticks 6000 --duration 100

# Check average tick time
jq '.average_tick_ms' benchmark.json
```

**Expected**: Average tick time < 16.67ms (60 Hz); real-time simulation achieved.

### Scenario 8: Cross-Player Skill Resolution (SC-004)

**Purpose**: Verify possession resolution with skill tolerance.

```bash
# Run simulation with skill logging
cargo run --release -- simulate --seed 12345 --ticks 1000 --log-contests > contests.txt

# Analyze contested situations
grep "skill_diff" contests.txt | head -20
```

**Expected**: 
- Skill difference > 0.1: Higher skill wins
- Skill difference ≤ 0.1: First contact wins

## Success Criteria Verification

| Criterion | Scenario | Command |
|-----------|----------|---------|
| SC-001 | 1 | `diff run1.json run2.json` |
| SC-002 | 7 | `jq '.average_tick_ms' benchmark.json` |
| SC-003 | 7 | `jq '.average_tick_ms' benchmark.json` |
| SC-004 | 8 | `grep "skill_diff" contests.txt` |
| SC-005 | 5 | `diff events1.json events2.json` |
| SC-006 | 6 | `jq '.clock' match.json` |
| SC-007 | 1 | `diff run1.json run2.json` |
| SC-008 | 6 | `jq '.clock' match.json` |
| SC-009 | 3 | Recovery time < 1 second |

## Troubleshooting

### Determinism Failure
- Check RNG seed is correctly initialized
- Verify no wall-clock time usage in simulation
- Ensure hash-map iteration uses sorted keys

### Performance Issues
- Profile with `cargo flamegraph`
- Check for allocations in hot paths
- Verify fixed timestep accumulator is correct

### Recovery Failure
- Check snapshot file integrity
- Verify state hash matches
- Ensure deserialization is deterministic

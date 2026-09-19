# Contracts: Football Match Simulation Engine

**Date**: 2026-09-19

## Overview

This directory contains interface contracts for the Football Match Simulation Engine. The architecture is server-authoritative: clients only render snapshots and send validated commands.

## Contracts

### 1. [simulation-api.md](./simulation-api.md)
Core simulation API contract between `sim-server` and `sim-core`. Defines how the server loop interacts with the simulation engine.

### 2. [client-protocol.md](./client-protocol.md)
Client-server message protocol. Defines what data clients receive (snapshots) and what commands they can send.

### 3. [state-snapshot.md](./state-snapshot.md)
State persistence format for replay and recovery. Defines the serialized snapshot structure.

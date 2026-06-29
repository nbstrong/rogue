# Rogue

`rogue` is a traditional turn-based roguelike built as a Rust workspace with Bevy.

The project is split into two crates:

- `rogue_core` contains deterministic simulation, game rules, map logic, AI, combat, items, scheduling, and persistence models.
- `rogue_app` contains the Bevy application shell, input handling, presentation, UI, asset loading, and save-file I/O.

The architecture keeps gameplay authoritative in the core simulation and treats rendering as a projection of that state. This makes the game suitable for:

- deterministic replays
- headless simulation tests
- save/load validation
- content-driven expansion

## Project Goals

- deterministic turn-based simulation
- dense tile-map storage instead of tile entities
- clear separation between simulation and presentation
- shared action pipeline for player and AI
- explicit effect resolution
- versioned snapshot persistence

## Workspace Layout

```text
.
├── Cargo.toml
├── README.md
├── assets/
├── crates/
│   ├── rogue_core/
│   └── rogue_app/
├── docs/
│   └── architecture.md
└── tests/
```

## Getting Started

Run the game:

```bash
cargo run -p rogue_app --features dev
```

Run the app integration tests:

```bash
cargo test -p rogue_app --features dev
```

Run the core simulation tests:

```bash
cargo test -p rogue_core
```

Run a specific app test file:

```bash
cargo test -p rogue_app --features dev --test app_loop
```

Run a specific core test file:

```bash
cargo test -p rogue_core --test combat
```

## Design Summary

- Simulation is authoritative.
- Player input and AI both produce domain `Action` values.
- Map tiles are stored in dense arrays.
- ECS entities represent actors, items, and other stateful world objects.
- Turn order is managed by a tick-based scheduler.
- Save files use serialized domain snapshots, not raw Bevy world state.

## Documentation

The full architecture is documented in [docs/architecture.md](docs/architecture.md).

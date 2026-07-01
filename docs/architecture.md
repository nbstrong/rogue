# Traditional Roguelike Architecture

## 1. Purpose

This document defines the software architecture for a traditional turn-based roguelike implemented in Rust with Bevy.

The design emphasizes:

- Deterministic simulation
- Clear separation between gameplay and presentation
- Testable game rules
- Extensible turn scheduling
- Efficient tile-map representation
- Data-driven content
- Stable save-game persistence

The project uses a Cargo workspace with three primary crates:

- `tactical_sim`: deterministic simulation and game rules
- `bread_and_iron`: headless game composition and scenario bootstrap
- `bread_and_iron_app`: Bevy application, rendering, input, UI, audio, and platform integration

---

## 2. Architectural Overview

```text
┌──────────────────────────────────────────────┐
│ bread_and_iron_app                         │
│ Window · input · rendering · UI · audio      │
│ Asset loading · save-file I/O                │
└───────────────────┬──────────────────────────┘
                    │ ActorIntent
                    │ presentation queries
                    ▼
┌──────────────────────────────────────────────┐
│ tactical_sim                         │
│ Deterministic Bevy ECS simulation            │
│ Map · actors · actions · combat · AI         │
│ Items · effects · FOV · generation · saves   │
└──────────────────────────────────────────────┘
```

The simulation core must not depend on:

- Sprites
- Cameras
- Keyboard keys
- Mouse buttons
- Screen coordinates
- Animation timing
- Audio playback
- Windowing APIs

The application layer translates platform input into simulation actions and projects simulation state into visual and audio output.

---

## 3. Architectural Principles

### 3.1 Simulation Is Authoritative

The simulation owns all gameplay truth:

- Actor positions
- Health
- Inventory
- Combat results
- Visibility
- Turn order
- Status effects
- Level transitions
- Death
- Victory and defeat conditions

Rendering and animation may represent simulation results, but they must never determine them.

### 3.2 Input Produces Actions

Player input and AI decisions produce the same domain-level `Action` values.

Neither input systems nor AI systems directly:

- Move entities
- Apply damage
- Pick up items
- Open doors
- Change levels

All actions pass through a shared validation and resolution pipeline.

### 3.3 Tiles Are Dense Data

Static map tiles are stored in contiguous collections rather than represented as one ECS entity per tile.

ECS entities are reserved for objects that have identity or behavior, such as:

- Actors
- Items
- Traps
- Interactive fixtures
- Stateful doors
- Temporary effects

### 3.4 Determinism Is a Feature

Given the same:

- Initial seed
- Content definitions
- Player command sequence

The simulation should produce the same final state.

This supports:

- Replays
- Regression tests
- Reproducible bug reports
- Seed sharing
- Save validation

### 3.5 Persistence Uses Domain Snapshots

Save files serialize explicit domain structures rather than raw Bevy world state.

Bevy `Entity` identifiers are runtime implementation details and are not durable save-game identities.

---

## 4. Workspace Layout

```text
traditional-roguelike/
├── Cargo.toml
├── architecture.md
├── assets/
│   ├── fonts/
│   ├── sprites/
│   ├── audio/
│   └── data/
│       ├── actors.ron
│       ├── items.ron
│       └── levels.ron
├── crates/
│   ├── tactical_sim/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── simulation.rs
│   │       ├── action/
│   │       │   ├── mod.rs
│   │       │   ├── intent.rs
│   │       │   ├── queue.rs
│   │       │   ├── resolver.rs
│   │       │   └── schedule.rs
│   │       ├── actor/
│   │       │   ├── mod.rs
│   │       │   ├── components.rs
│   │       │   ├── combat.rs
│   │       │   ├── ai.rs
│   │       │   └── spawn.rs
│   │       ├── world/
│   │       │   ├── mod.rs
│   │       │   ├── map.rs
│   │       │   ├── tile.rs
│   │       │   ├── spatial.rs
│   │       │   ├── fov.rs
│   │       │   └── generation.rs
│   │       ├── item/
│   │       │   ├── mod.rs
│   │       │   ├── components.rs
│   │       │   ├── inventory.rs
│   │       │   └── effects.rs
│   │       ├── time/
│   │       │   ├── mod.rs
│   │       │   ├── clock.rs
│   │       │   └── scheduler.rs
│   │       ├── content/
│   │       │   ├── mod.rs
│   │       │   ├── definitions.rs
│   │       │   └── registry.rs
│   │       └── persistence/
│   │           ├── mod.rs
│   │           ├── snapshot.rs
│   │           └── migration.rs
│   └── bread_and_iron_app/
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── app_state.rs
│           ├── input/
│           │   ├── mod.rs
│           │   ├── keyboard.rs
│           │   └── mapping.rs
│           ├── presentation/
│           │   ├── mod.rs
│           │   ├── camera.rs
│           │   ├── map_view.rs
│           │   ├── actor_view.rs
│           │   ├── animation.rs
│           │   └── synchronization.rs
│           ├── ui/
│           │   ├── mod.rs
│           │   ├── hud.rs
│           │   ├── inventory.rs
│           │   ├── targeting.rs
│           │   └── log.rs
│           ├── assets/
│           │   ├── mod.rs
│           │   └── loading.rs
│           └── persistence/
│               ├── mod.rs
│               └── files.rs
└── tests/
    ├── generation.rs
    ├── combat.rs
    └── deterministic_replay.rs
```

Begin with these crates. Additional crates should be introduced only after a domain boundary has demonstrated independent versioning, testing, or reuse needs.

---

## 5. Cargo Workspace

```toml
[workspace]
members = [
    "crates/sim_core",
    "crates/tactical_sim",
    "crates/bread_and_iron",
    "crates/bread_and_iron_app",
]
resolver = "3"

[workspace.package]
edition = "2024"
version = "0.1.0"

[workspace.dependencies]
bevy = "0.19"
bevy_app = "0.19"
bevy_ecs = "0.19"
bevy_math = "0.19"
serde = { version = "1", features = ["derive"] }
```

The core crate should depend only on the Bevy subcrates it needs. The application crate may depend on the complete `bevy` crate.

A typical dependency direction is:

```text
bread_and_iron_app ──depends on──► tactical_sim
```

`tactical_sim` must never depend on `bread_and_iron_app`.

---

## 6. Map Model

### 6.1 Tile Representation

A dungeon level is a dense rectangular grid.

```rust
use bevy_ecs::prelude::Resource;
use bevy_math::IVec2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKind {
    Floor,
    Wall,
    ClosedDoor,
    OpenDoor,
    StairsUp,
    StairsDown,
}

#[derive(Debug, Clone)]
pub struct Tile {
    pub kind: TileKind,
    pub explored: bool,
    pub visible: bool,
}

#[derive(Resource)]
pub struct LevelMap {
    pub width: u32,
    pub height: u32,
    pub tiles: Vec<Tile>,
}

impl LevelMap {
    pub fn index(&self, position: IVec2) -> Option<usize> {
        if position.x < 0
            || position.y < 0
            || position.x >= self.width as i32
            || position.y >= self.height as i32
        {
            return None;
        }

        Some(position.y as usize * self.width as usize + position.x as usize)
    }

    pub fn tile(&self, position: IVec2) -> Option<&Tile> {
        self.index(position).map(|index| &self.tiles[index])
    }
}
```

A dense vector provides:

- Constant-time tile lookup
- Cache-friendly traversal
- Efficient FOV and pathfinding
- Compact serialization
- Straightforward procedural generation

### 6.2 Level Identity

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LevelId(pub u32);
```

Actor positions include both a level and a grid cell:

```rust
use bevy_ecs::prelude::Component;
use bevy_math::IVec2;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPosition {
    pub level: LevelId,
    pub cell: IVec2,
}
```

---

## 7. Entity Model

### 7.1 Actor Components

```rust
use bevy_ecs::prelude::*;

#[derive(Component)]
pub struct Actor;

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct Monster;

#[derive(Component)]
pub struct BlocksMovement;

#[derive(Component)]
pub struct BlocksSight;

#[derive(Component, Debug, Clone, Copy)]
pub struct Health {
    pub current: i32,
    pub maximum: i32,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct CombatStats {
    pub power: i32,
    pub defense: i32,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct Vision {
    pub range: u32,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct ActionSpeed {
    pub ticks_per_action: u64,
}

#[derive(Component, Debug, Clone)]
pub struct PrototypeId(pub String);
```

Components should remain:

- Small
- Cohesive
- Domain-oriented
- Independently queryable

Avoid monolithic components such as `MonsterData` that combine health, position, AI, rendering, and inventory.

---

## 8. Spatial Index

Repeatedly scanning every positioned entity for movement, AI, targeting, and collision is unnecessary. A spatial index provides efficient occupancy queries.

```rust
use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use bevy_math::IVec2;

#[derive(Resource, Default)]
pub struct SpatialIndex {
    pub occupants: HashMap<(LevelId, IVec2), Vec<Entity>>,
    pub movement_blockers: HashSet<(LevelId, IVec2)>,
    pub sight_blockers: HashSet<(LevelId, IVec2)>,
}
```

### 8.1 Required Invariants

1. Every entity with `GridPosition` appears in `occupants`.
2. Every positioned entity with `BlocksMovement` appears in `movement_blockers`.
3. Every positioned entity with `BlocksSight` appears in `sight_blockers`.
4. The index is updated after movement, spawning, and despawning.
5. Debug builds validate the index at the end of each resolved action.

The first implementation may rebuild the complete spatial index after each action or turn. Incremental maintenance should be introduced only when profiling demonstrates a need.

---

## 9. Action Model

### 9.1 Domain Actions

```rust
use bevy_ecs::prelude::*;
use bevy_math::IVec2;

#[derive(Debug, Clone)]
pub struct Action {
    pub actor: Entity,
    pub kind: ActionKind,
}

#[derive(Debug, Clone)]
pub enum ActionKind {
    Wait,
    Move {
        delta: IVec2,
    },
    Melee {
        target: Entity,
    },
    PickUp {
        item: Entity,
    },
    Drop {
        item: Entity,
    },
    UseItem {
        item: Entity,
        target: ActionTarget,
    },
    Descend,
    Ascend,
}

#[derive(Debug, Clone)]
pub enum ActionTarget {
    SelfTarget,
    Entity(Entity),
    Cell {
        level: LevelId,
        position: IVec2,
    },
}
```

### 9.2 Action Transformation

An attempted action may resolve into a different action based on world state.

```text
Move north
   │
   ├── empty walkable cell ──► movement
   ├── hostile occupant ─────► melee attack
   ├── closed door ──────────► open door
   └── wall ─────────────────► rejected action
```

This transformation belongs in the shared resolver, not in keyboard handling or AI.

### 9.3 Action Queue

```rust
use std::collections::VecDeque;
use bevy_ecs::prelude::Resource;

#[derive(Resource, Default)]
pub struct ActionQueue {
    actions: VecDeque<Action>,
}

impl ActionQueue {
    pub fn push(&mut self, action: Action) {
        self.actions.push_back(action);
    }

    pub fn pop(&mut self) -> Option<Action> {
        self.actions.pop_front()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}
```

Use a resource-backed queue for authoritative gameplay work.

Use transient Bevy messages or events only for non-authoritative notifications such as:

- Combat log entries
- Damage numbers
- Sound requests
- Camera shake
- Particle effects

The deterministic simulation pipeline should not depend on observer execution order.

---

## 10. Turn Scheduling

### 10.1 Application States

Application states control large-scale application flow.

```rust
use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Boot,
    MainMenu,
    Playing,
    GameOver,
}
```

Input modes control interaction while playing:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum InputMode {
    #[default]
    Normal,
    Inventory,
    Targeting,
    Examine,
}
```

Do not model every combat or movement phase as a Bevy application state. Internal simulation phases belong in a custom schedule.

### 10.2 Simulation Schedule

```rust
use bevy_ecs::prelude::*;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimulationStep;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimulationSet {
    SelectActor,
    DecideAction,
    Validate,
    Resolve,
    ApplyEffects,
    HandleDeath,
    RebuildDerivedData,
    FinishStep,
}
```

```rust
use bevy_app::{App, Plugin};

pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActionQueue>()
            .init_resource::<TurnClock>()
            .init_resource::<SpatialIndex>()
            .configure_sets(
                SimulationStep,
                (
                    SimulationSet::SelectActor,
                    SimulationSet::DecideAction,
                    SimulationSet::Validate,
                    SimulationSet::Resolve,
                    SimulationSet::ApplyEffects,
                    SimulationSet::HandleDeath,
                    SimulationSet::RebuildDerivedData,
                    SimulationSet::FinishStep,
                )
                    .chain(),
            )
            .add_systems(
                SimulationStep,
                (
                    select_next_actor.in_set(SimulationSet::SelectActor),
                    generate_ai_action.in_set(SimulationSet::DecideAction),
                    validate_action.in_set(SimulationSet::Validate),
                    resolve_action.in_set(SimulationSet::Resolve),
                    apply_pending_effects.in_set(SimulationSet::ApplyEffects),
                    remove_dead_entities.in_set(SimulationSet::HandleDeath),
                    update_spatial_index.in_set(
                        SimulationSet::RebuildDerivedData,
                    ),
                    finish_simulation_step.in_set(SimulationSet::FinishStep),
                ),
            );
    }
}
```

### 10.3 Simulation Driver

```rust
use bevy_ecs::prelude::*;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationStatus {
    AwaitingInput,
    Resolving,
    Terminal,
}

pub fn drive_simulation(world: &mut World) {
    const MAX_STEPS_PER_FRAME: usize = 1_024;

    for _ in 0..MAX_STEPS_PER_FRAME {
        let status = *world.resource::<SimulationStatus>();

        if status != SimulationStatus::Resolving {
            return;
        }

        world.run_schedule(SimulationStep);
    }

    panic!("simulation exceeded the per-frame step limit");
}
```

The simulation runs until:

- It requires another controlled actor action
- The simulation reaches a terminal state
- The safety limit detects a likely infinite loop

### 10.4 Timeline-Based Scheduling

```rust
use std::{
    cmp::Reverse,
    collections::BinaryHeap,
};

use bevy_ecs::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScheduledActor {
    pub next_tick: u64,
    pub sequence: u64,
    pub actor: Entity,
}

#[derive(Resource, Default)]
pub struct TurnClock {
    pub current_tick: u64,
    pub next_sequence: u64,
    pub timeline: BinaryHeap<Reverse<ScheduledActor>>,
}
```

After resolving an action:

```text
new next_tick = current_tick + action_cost
```

Example action costs:

```text
Wait             100 ticks
Move             100 ticks
Fast move         70 ticks
Heavy attack     140 ticks
Open door         80 ticks
```

A sequence number acts as a deterministic tie-breaker when two actors are scheduled for the same tick.

The scheduler naturally supports:

- Slow and fast actors
- Haste
- Paralysis
- Weapon speed
- Damage over time
- Timed traps
- Environmental effects

---

## 11. Combat and Effects

Primary action resolution produces queued effects.

```rust
use std::collections::VecDeque;
use bevy_ecs::prelude::*;

#[derive(Debug, Clone)]
pub enum Effect {
    Damage {
        source: Option<Entity>,
        target: Entity,
        amount: i32,
        kind: DamageKind,
    },
    Heal {
        target: Entity,
        amount: i32,
    },
    Teleport {
        target: Entity,
        destination: GridPosition,
    },
    ApplyStatus {
        target: Entity,
        status: StatusEffect,
    },
}

#[derive(Resource, Default)]
pub struct EffectQueue(pub VecDeque<Effect>);
```

The resolution pipeline is:

```text
Action
  → validation
  → primary outcome
  → effect queue
  → effect application
  → deaths
  → drops / experience / triggers
  → spatial and FOV updates
  → presentation notifications
```

This keeps damage and effect semantics consistent across:

- Melee attacks
- Ranged attacks
- Items
- Traps
- Status effects
- Environmental hazards

---

## 12. AI Architecture

AI follows a layered decision pipeline:

```text
Perception
   ↓
Knowledge and memory
   ↓
Goal selection
   ↓
Action selection
   ↓
Shared action resolver
```

Suggested components:

```rust
use bevy_ecs::prelude::*;
use bevy_math::IVec2;

#[derive(Component)]
pub struct HostileToPlayer;

#[derive(Component)]
pub struct LastKnownPlayerPosition {
    pub level: LevelId,
    pub cell: IVec2,
    pub observed_at: u64,
}

#[derive(Component, Default)]
pub enum AiGoal {
    #[default]
    Idle,
    Wander,
    Investigate(GridPosition),
    Chase(Entity),
    Flee(Entity),
}
```

AI must emit the same `Action` type used by the player.

AI must not bypass:

- Collision
- Range checks
- Line of sight
- Action cost
- Combat validation
- Inventory rules

---

## 13. Field of View and Exploration

Field of view is derived simulation state.

A typical visibility update performs:

1. Clear current visibility for the active level.
2. Compute visible cells from the player's position.
3. Mark those cells as visible.
4. Mark visible cells as explored.
5. Notify the presentation layer that map visibility changed.

Visibility should be recalculated when:

- The player moves
- Sight-blocking terrain changes
- Sight-blocking entities move
- Vision range changes
- The player changes levels

The renderer may apply presentation rules such as:

- Visible cells: full brightness
- Explored cells: dimmed
- Unexplored cells: hidden

The renderer must not independently decide whether a cell is visible.

---

## 14. Rendering and Presentation Synchronization

### 14.1 Initial Model

The simplest implementation may attach presentation components directly to simulation entities.

```rust
use bevy_ecs::prelude::*;

#[derive(Component)]
pub struct Glyph {
    pub character: char,
}

#[derive(Component)]
pub struct RenderLayer(pub i32);
```

Presentation systems query `GridPosition` and update visual transforms.

### 14.2 Decoupled View Entities

As visual complexity grows, use separate presentation entities.

```rust
use std::collections::HashMap;
use bevy_ecs::prelude::*;

#[derive(Resource, Default)]
pub struct PresentationEntities {
    pub views: HashMap<Entity, Entity>,
}
```

```text
simulation entity 42 ──► presentation entity 301
```

This model supports:

- Multiple visual children
- Health bars
- Animation controllers
- Particle emitters
- Selection indicators
- Temporary effects

Presentation systems may interpolate or animate toward authoritative simulation state, but animation completion must not determine gameplay outcomes.

---

## 15. Content Definitions

Immutable content definitions are separate from mutable runtime state.

```rust
use std::collections::HashMap;
use bevy_ecs::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ActorDefinition {
    pub id: String,
    pub name: String,
    pub maximum_health: i32,
    pub power: i32,
    pub defense: i32,
    pub vision_range: u32,
    pub action_speed: u64,
}

#[derive(Resource, Default)]
pub struct ContentRegistry {
    pub actors: HashMap<String, ActorDefinition>,
    pub items: HashMap<String, ItemDefinition>,
}
```

A spawned actor stores only its definition reference and mutable overrides:

```text
PrototypeId("orc")
Health { current: 7, maximum: 10 }
GridPosition(...)
```

Content files may use RON initially:

```text
assets/data/actors.ron
assets/data/items.ron
assets/data/levels.ron
```

Content loading should validate:

- Duplicate IDs
- Missing references
- Invalid numeric ranges
- Unknown effect types
- Impossible equipment slots
- Malformed loot tables

Invalid content should fail during startup rather than during gameplay.

---

## 16. Randomness

Randomness is centralized and explicit.

```rust
use bevy_ecs::prelude::*;

#[derive(Resource)]
pub struct RandomStreams {
    pub generation: DeterministicRng,
    pub combat: DeterministicRng,
    pub loot: DeterministicRng,
    pub ai: DeterministicRng,
}
```

Separate streams prevent unrelated features from changing gameplay outcomes.

For example:

```text
Adding a cosmetic random particle
→ consumes a random value
→ changes future combat rolls
→ breaks deterministic replay
```

The presentation layer should maintain its own non-authoritative random stream for purely visual effects.

A new game records a root seed. A save file records the full state of each authoritative random stream, not only the original seed.

---

## 17. Persistence

### 17.1 Stable Identity

```rust
use bevy_ecs::prelude::*;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PersistentId(pub u64);
```

Persistent IDs are used for durable references between saved entities.

Raw Bevy `Entity` values must not appear in the save format.

### 17.2 Snapshot Model

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct GameSnapshot {
    pub version: u32,
    pub current_level: u32,
    pub current_tick: u64,
    pub levels: Vec<LevelSnapshot>,
    pub entities: Vec<EntitySnapshot>,
    pub rng: RandomSnapshot,
}

#[derive(Serialize, Deserialize)]
pub struct EntitySnapshot {
    pub id: u64,
    pub prototype: String,
    pub position: Option<SavedPosition>,
    pub health: Option<SavedHealth>,
    pub inventory_owner: Option<u64>,
}
```

Loading proceeds in phases:

1. Deserialize and validate the snapshot.
2. Create fresh Bevy entities.
3. Build a `PersistentId` to `Entity` map.
4. Restore simple components.
5. Reconnect cross-entity references.
6. Restore timelines and RNG state.
7. Rebuild derived data such as spatial indexes and FOV.
8. Validate world invariants.

### 17.3 Save Migration

Each save contains a schema version.

```text
save v1 → migration → save v2 → migration → current schema
```

Migrations operate on serialized domain snapshots rather than live ECS state.

---

## 18. Plugin Composition

```rust
use bevy::prelude::*;
use tactical_sim::SimulationPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .init_state::<AppState>()
        .add_plugins((
            SimulationPlugin,
            AssetPlugin,
            InputPlugin,
            PresentationPlugin,
            GameUiPlugin,
            SavePlugin,
        ))
        .run();
}
```

A presentation plugin may be structured as:

```rust
use bevy::prelude::*;

pub struct PresentationPlugin;

impl Plugin for PresentationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                synchronize_map_view,
                synchronize_actor_views,
                update_camera,
                update_animations,
            )
                .run_if(in_state(AppState::Playing)),
        );
    }
}
```

Recommended top-level plugins:

| Plugin | Responsibility |
|---|---|
| `SimulationPlugin` | Turn scheduling, actions, combat, AI, effects |
| `AssetPlugin` | Loading and validating static content |
| `InputPlugin` | Mapping platform input to player intentions |
| `PresentationPlugin` | Map and actor visualization |
| `GameUiPlugin` | HUD, inventory, targeting, combat log |
| `SavePlugin` | File I/O and snapshot orchestration |

---

## 19. System Ordering

System ordering should be explicit only where correctness requires it.

The simulation schedule uses ordered system sets:

```text
SelectActor
→ DecideAction
→ Validate
→ Resolve
→ ApplyEffects
→ HandleDeath
→ RebuildDerivedData
→ FinishStep
```

Within a set, systems may run in parallel when they:

- Do not depend on each other's output
- Do not mutate overlapping data
- Produce commutative results

Avoid global `.chain()` calls across large unrelated system collections. Excessive ordering reduces parallelism and makes hidden dependencies harder to identify.

---

## 20. Error Handling and Invariants

Expected domain failures should be represented explicitly.

Examples:

- Invalid movement
- Out-of-range attack
- Missing item
- Full inventory
- Blocked destination
- Invalid target

These are not application crashes.

```rust
pub enum ActionFailure {
    Blocked,
    InvalidTarget,
    OutOfRange,
    MissingItem,
    InventoryFull,
    ActorUnavailable,
}
```

Programming errors and broken invariants should fail loudly in development builds.

Useful debug validations include:

- Spatial index consistency
- No dead actor in the scheduler
- No item with multiple owners
- No actor outside map bounds
- No duplicate persistent IDs
- All referenced entities exist
- Player count is exactly one during active play

---

## 21. Testing Strategy

Most simulation tests should run without rendering or window initialization.

```rust
#[test]
fn walking_into_enemy_becomes_melee_attack() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);

    // Insert a small test map.
    // Spawn player and monster.
    // Queue movement toward the monster.
    // Run SimulationStep.
    // Assert positions and health.
}
```

### 21.1 Unit Tests

Use unit tests for:

- Tile indexing
- Action-cost calculations
- Damage calculations
- Inventory rules
- Status-effect duration
- Save migration functions
- Content validation

### 21.2 Simulation Tests

Use headless Bevy apps for:

- Movement and collision
- Bump-to-attack conversion
- AI action selection
- Turn scheduling
- Death cleanup
- Level transitions
- Item usage
- FOV updates

### 21.3 Property Tests

Useful properties include:

- Generated levels have a connected playable region.
- Actors never end a step inside blocking terrain.
- Two movement blockers never occupy the same cell.
- Inventory items have exactly one valid location.
- Dead actors never remain in the timeline.
- Every scheduled actor exists and can act.
- Saving and loading preserves a normalized world snapshot.

### 21.4 Deterministic Replay Tests

A replay fixture records:

```text
seed: 8A7D…
commands:
  - MoveNorth
  - MoveNorth
  - PickUp
  - MoveEast
expected_world_hash: …
```

The test initializes the world, applies the command sequence, and compares a normalized world hash.

Entity creation order and transient presentation data must not affect this hash.

---

## 22. Initial Vertical Slice

The first playable milestone should include only:

```text
Generate one room
→ spawn player
→ spawn one monster
→ accept eight-direction movement
→ convert bump into melee
→ monster moves toward player
→ calculate FOV
→ display map and combat log
→ restart on death
```

This validates the critical architecture:

- Input produces domain actions.
- The simulation schedule owns game rules.
- Tiles are dense data.
- Actors are ECS entities.
- AI uses the shared action API.
- Rendering mirrors simulation state.
- The core runs headlessly in tests.

After this slice is stable, add features incrementally.

---

## 23. Recommended Implementation Phases

### Phase 1: Core Movement

- Workspace setup
- Dense tile map
- Player actor
- Grid movement
- Spatial index
- Headless movement tests

### Phase 2: Turn Loop

- Action queue
- Simulation schedule
- Timeline scheduler
- Waiting-for-player state
- Basic hostile AI

### Phase 3: Combat

- Bump-to-attack
- Effect queue
- Damage and death
- Combat log notifications
- Deterministic combat tests

### Phase 4: Visibility and Presentation

- Field of view
- Explored tiles
- Map rendering
- Actor rendering
- Camera
- HUD

### Phase 5: Items and Content

- Item entities
- Inventory
- Equipment
- Usable effects
- RON content definitions
- Content validation

### Phase 6: Procedural Generation

- Room and corridor generation
- Connectivity validation
- Spawn placement
- Multi-level dungeon support
- Seed display and replay

### Phase 7: Persistence

- Persistent IDs
- Snapshot schema
- Save and load
- Version migration
- Save/load equivalence tests

---

## 24. Deferred Decisions

The following decisions should remain open until the initial vertical slice exposes concrete requirements:

- ASCII glyph rendering versus tile sprites
- Pathfinding algorithm and caching strategy
- Behavior trees versus utility AI
- Single-world versus streamed multi-level storage
- Animation interpolation model
- Modding interface
- Scripting language
- Networking
- Advanced scene formats

Prematurely committing to these systems would add complexity without validating the core game loop.

---

## 25. Architecture Decision Summary

| Decision | Choice |
|---|---|
| Project organization | Two-crate Cargo workspace |
| Simulation framework | Bevy ECS in a custom schedule |
| Tile storage | Dense arrays, not tile entities |
| Dynamic objects | ECS entities |
| Player and AI commands | Shared domain `Action` type |
| Turn model | Tick-based priority timeline |
| Gameplay work queue | Resource-backed queue |
| Effects | Explicit effect queue |
| Rendering | Projection of authoritative simulation |
| Content | Data-driven immutable definitions |
| Randomness | Centralized deterministic streams |
| Saves | Versioned domain snapshots |
| Durable identity | Explicit `PersistentId` |
| Testing | Headless simulation and replay tests |

---

## 26. Definition of Architectural Success

The architecture is working when the project can:

1. Run the complete game simulation without a window.
2. Replay a command sequence deterministically.
3. Replace the renderer without changing gameplay code.
4. Add a new AI actor without changing input handling.
5. Add a new damage source without duplicating damage rules.
6. Save and reload without relying on runtime entity IDs.
7. Test procedural generation and combat in isolation.
8. Detect world-invariant violations during development.

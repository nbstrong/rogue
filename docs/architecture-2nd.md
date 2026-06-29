# Architecture Migration Plan: Traditional Roguelike to Hierarchical City and Army Simulation

## 1. Purpose

This document instructs an implementation agent how to evolve the architecture defined in `architecture.md` into a hierarchical world-simulation architecture capable of supporting:

- A large city simulation inspired by the systemic depth of Dwarf Fortress, with cities such as ancient Rome as the target scale
- Strategic and operational army simulation
- Formation-level battles and optional tactical projection
- Continuous mouse-wheel camera zoom
- Semantic zoom, where visible representations change with scale
- Multiple simultaneous tile maps and map layers
- Interchangeable ASCII and graphical tile renderers
- A flexible, semantic coloring and overlay system
- Deterministic simulation, headless testing, and versioned persistence

This is a migration specification, not a greenfield design. The implementation must preserve working behavior while progressively replacing assumptions from the original architecture.

The central architectural shift is:

> Replace a single-resolution tactical roguelike simulation with a hierarchical, multi-rate simulation in which the same world can be represented statistically, as aggregates, or in detailed local form.

---

## 2. Agent Operating Instructions

The implementation agent must follow these rules throughout the migration.

### 2.1 Do Not Perform a Big-Bang Rewrite

Every migration phase must:

1. Compile independently.
2. Preserve or deliberately migrate existing tests.
3. Add characterization tests before replacing working behavior.
4. Leave the repository in a runnable state.
5. Avoid mixing unrelated refactors into the same change.
6. Include explicit migration adapters when old and new APIs must coexist.

Prefer this sequence:

```text
introduce new abstraction
→ adapt old implementation to new abstraction
→ migrate callers
→ add new behavior
→ remove obsolete compatibility layer
```

Do not use this sequence:

```text
delete old architecture
→ create many empty crates
→ attempt to restore functionality later
```

### 2.2 Preserve Architectural Invariants

The following invariants from `architecture.md` remain mandatory:

- Simulation state is authoritative.
- Rendering does not determine gameplay outcomes.
- Input produces domain commands rather than directly mutating gameplay state.
- Static tile data is stored densely or in chunked dense blocks.
- Runtime Bevy `Entity` values are not durable save identities.
- Authoritative randomness is deterministic and centrally managed.
- Save files use explicit, versioned domain snapshots.
- Core simulation can run without a window or graphics device.
- Simulation tests do not require the full Bevy renderer.
- Presentation crates never become dependencies of simulation crates.

### 2.3 Add Tests Before Moving Boundaries

Before moving a subsystem to a new crate or replacing its data model, add tests that capture current behavior.

At minimum, preserve tests for:

- Grid movement
- Bump-to-attack conversion
- Damage and death
- Turn ordering
- Spatial-index consistency
- Field of view
- Save/load equivalence
- Deterministic replay

### 2.4 Keep Compatibility Layers Temporary

Every compatibility adapter must include a removal condition in a comment or tracking issue.

Example:

```rust
/// Compatibility adapter for the original single-map API.
///
/// Remove after all movement, FOV, and persistence callers use `MapId`.
pub fn active_level_map(world: &World) -> &LevelMap {
    // ...
}
```

Temporary adapters must not become the permanent public API.

---

## 3. Current Architecture Summary

The starting architecture has two principal crates:

```text
rogue_app
    └── depends on rogue_core
```

`rogue_core` currently owns:

- A single dense `LevelMap`
- Tactical actors represented as ECS entities
- `GridPosition { level, cell }`
- Player and AI actions
- A resource-backed action queue
- Tactical action resolution
- Combat and effect queues
- Actor turn scheduling
- Field of view
- Content definitions
- Save snapshots

`rogue_app` currently owns:

- Keyboard input
- Camera and rendering
- Map and actor presentation
- UI
- Asset loading
- Save-file I/O

The original architecture is suitable for a single tactical map. It contains several assumptions that must now be removed.

---

## 4. Assumptions to Remove

The migration is complete only when none of the following assumptions are embedded in core APIs.

### 4.1 Single Map Assumption

Current assumption:

```text
one active level map
```

Target:

```text
many registered maps
many loaded maps
many map layers
multiple maps visible or simulated at once
```

### 4.2 Single Spatial Scale Assumption

Current assumption:

```text
every relevant object has a tactical cell position
```

Target:

```text
objects may be located in a region, settlement, district, building,
route segment, formation, battlefield sector, or tactical cell
```

### 4.3 Single Simulation Cadence Assumption

Current assumption:

```text
actors act one at a time on one tactical timeline
```

Target:

```text
tactical actions, minute tasks, hourly production, daily consumption,
strategic movement, and battle resolution advance on different cadences
```

### 4.4 Individual-Entity Simulation Assumption

Current assumption:

```text
every active person or combatant is an ECS entity with detailed behavior
```

Target:

```text
population and armies can exist as statistics, aggregates, or detailed agents
```

### 4.5 One Renderer Assumption

Current assumption:

```text
simulation components directly imply a glyph or sprite
```

Target:

```text
simulation state produces semantic visual descriptions;
ASCII and tile renderers independently resolve those descriptions
```

### 4.6 Literal Color Assumption

Current assumption:

```text
entities and tiles carry final colors
```

Target:

```text
entities and tiles carry semantic visual meaning;
themes, overlays, visibility, and accessibility resolve final colors
```

### 4.7 Camera Zoom Equals Magnification Assumption

Current assumption:

```text
zoom only changes camera scale
```

Target:

```text
continuous camera scale selects discrete semantic zoom bands,
which alter representation, density, labels, overlays, and detail
```

---

## 5. Target Architecture

The final dependency structure should be:

```text
game_app
├── presentation
│   ├── renderer_ascii
│   └── renderer_tiles
├── city_sim
├── military_sim
├── tactical_sim
└── world_model
    └── sim_core
```

A more explicit dependency graph is:

```text
                         ┌──────────────────┐
                         │     game_app     │
                         └───────┬──────────┘
                                 │
                 ┌───────────────┼────────────────┐
                 │               │                │
                 ▼               ▼                ▼
        ┌────────────────┐ ┌───────────┐  ┌─────────────┐
        │  presentation  │ │ city_sim  │  │ military_sim│
        └───────┬────────┘ └─────┬─────┘  └──────┬──────┘
                │                │               │
       ┌────────┴────────┐       └───────┬───────┘
       ▼                 ▼               ▼
┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│renderer_ascii│ │renderer_tiles│ │ tactical_sim │
└──────────────┘ └──────────────┘ └──────┬───────┘
                                         │
                                         ▼
                                ┌────────────────┐
                                │  world_model   │
                                └───────┬────────┘
                                        ▼
                                ┌────────────────┐
                                │    sim_core    │
                                └────────────────┘
```

No reverse dependencies are allowed.

In particular:

- `sim_core` must not know about maps, cities, armies, rendering, or UI.
- `world_model` must not know about Bevy cameras, glyphs, sprites, or colors.
- `city_sim` must not depend on `military_sim` implementation details.
- `military_sim` must not depend on renderer types.
- `presentation` may read public simulation data but may not mutate authoritative state except through commands.
- Render backends consume semantic presentation data; they do not query arbitrary simulation internals.

---

## 6. Target Workspace Layout

The final workspace should approximate:

```text
crates/
├── sim_core/
│   └── src/
│       ├── lib.rs
│       ├── command.rs
│       ├── effects.rs
│       ├── identity.rs
│       ├── rng.rs
│       ├── schedule.rs
│       ├── time.rs
│       ├── work_budget.rs
│       └── persistence/
│           ├── mod.rs
│           ├── migration.rs
│           └── version.rs
├── world_model/
│   └── src/
│       ├── lib.rs
│       ├── location.rs
│       ├── map_id.rs
│       ├── map_catalog.rs
│       ├── map_stack.rs
│       ├── chunk.rs
│       ├── layers/
│       ├── spatial/
│       ├── routes/
│       ├── geography/
│       └── persistence/
├── tactical_sim/
│   └── src/
│       ├── lib.rs
│       ├── action/
│       ├── actor/
│       ├── combat/
│       ├── effects/
│       ├── fov/
│       ├── movement/
│       ├── pathfinding/
│       └── projection/
├── city_sim/
│   └── src/
│       ├── lib.rs
│       ├── settlement/
│       ├── district/
│       ├── building/
│       ├── people/
│       ├── household/
│       ├── occupation/
│       ├── economy/
│       ├── logistics/
│       ├── institutions/
│       ├── tasks/
│       ├── fidelity/
│       └── persistence/
├── military_sim/
│   └── src/
│       ├── lib.rs
│       ├── army/
│       ├── formation/
│       ├── command/
│       ├── movement/
│       ├── supply/
│       ├── morale/
│       ├── battle/
│       ├── tactical_projection/
│       ├── fidelity/
│       └── persistence/
├── presentation/
│   └── src/
│       ├── lib.rs
│       ├── visual_key.rs
│       ├── visual_descriptor.rs
│       ├── view_adapter.rs
│       ├── zoom.rs
│       ├── theme.rs
│       ├── palette.rs
│       ├── color_resolution.rs
│       ├── overlay.rs
│       ├── selection.rs
│       └── dirty_regions.rs
├── renderer_ascii/
│   └── src/
│       ├── lib.rs
│       ├── glyph_cache.rs
│       ├── ascii_theme.rs
│       ├── map_renderer.rs
│       └── entity_renderer.rs
├── renderer_tiles/
│   └── src/
│       ├── lib.rs
│       ├── atlas_registry.rs
│       ├── tile_theme.rs
│       ├── chunk_mesh.rs
│       ├── map_renderer.rs
│       └── entity_renderer.rs
└── game_app/
    └── src/
        ├── main.rs
        ├── app_state.rs
        ├── input/
        ├── camera/
        ├── ui/
        ├── assets/
        ├── save_files/
        └── plugins/
```

Do not create all crates as empty shells at the beginning. Extract them only when each boundary has a real implementation and tests.

---

## 7. Core Data Model Changes

## 7.1 Introduce Typed Stable IDs

Replace ad hoc numeric IDs and durable references to Bevy entities with typed stable IDs.

```rust
use core::marker::PhantomData;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(Serialize, Deserialize)]
pub struct SimId<T> {
    raw: u64,
    #[serde(skip)]
    marker: PhantomData<fn() -> T>,
}
```

Provide domain aliases:

```rust
pub enum PersonTag {}
pub enum HouseholdTag {}
pub enum BuildingTag {}
pub enum SettlementTag {}
pub enum DistrictTag {}
pub enum ArmyTag {}
pub enum FormationTag {}
pub enum MapTag {}

pub type PersonId = SimId<PersonTag>;
pub type HouseholdId = SimId<HouseholdTag>;
pub type BuildingId = SimId<BuildingTag>;
pub type SettlementId = SimId<SettlementTag>;
pub type DistrictId = SimId<DistrictTag>;
pub type ArmyId = SimId<ArmyTag>;
pub type FormationId = SimId<FormationTag>;
pub type MapId = SimId<MapTag>;
```

Requirements:

- IDs are stable across save/load.
- IDs are never reused within a running campaign.
- ID allocation is deterministic.
- ECS entities may carry a stable ID component.
- Registries may contain records that are not materialized as ECS entities.
- Cross-domain APIs exchange stable IDs, not Bevy entities.

Migration rule:

- Keep Bevy `Entity` inside short-lived tactical commands only when execution occurs in the same world session.
- Prefer stable IDs for queued work, persistence, cross-map references, and domain events.

---

## 7.2 Replace `GridPosition` with Hierarchical Location

The original:

```rust
pub struct GridPosition {
    pub level: LevelId,
    pub cell: IVec2,
}
```

must become a tactical specialization rather than the universal location type.

Introduce:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellCoord {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkCoord {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Region {
        region: RegionId,
    },
    Settlement {
        settlement: SettlementId,
    },
    District {
        settlement: SettlementId,
        district: DistrictId,
    },
    Building {
        settlement: SettlementId,
        building: BuildingId,
    },
    Route {
        route: RouteId,
        progress: RouteProgress,
    },
    Formation {
        formation: FormationId,
    },
    MapCell {
        map: MapId,
        cell: CellCoord,
    },
}
```

Use specialized ECS components for hot queries:

```rust
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapPosition {
    pub map: MapId,
    pub cell: CellCoord,
}
```

Do not force every person or army to have a `MapPosition`.

A citizen represented statistically may have a district location. An army traveling strategically may have a route location. A formation projected onto a battlefield may additionally have a map-cell position.

---

## 7.3 Separate Records from Materialized ECS Agents

Introduce persistent domain registries.

```rust
#[derive(Resource, Default)]
pub struct PersonRegistry {
    pub people: BTreeMap<PersonId, PersonRecord>,
}

pub struct PersonRecord {
    pub id: PersonId,
    pub household: HouseholdId,
    pub age_days: u32,
    pub occupation: Option<OccupationId>,
    pub location: Location,
    pub health: PersonHealth,
    pub status: PersonStatus,
}
```

Only detailed, currently active people require ECS entities.

```rust
#[derive(Component)]
pub struct MaterializedPerson {
    pub id: PersonId,
}
```

Maintain a mapping:

```rust
#[derive(Resource, Default)]
pub struct MaterializationIndex {
    pub person_entities: HashMap<PersonId, Entity>,
    pub formation_entities: HashMap<FormationId, Entity>,
}
```

Requirements:

- Persistent records are authoritative for long-lived identity.
- ECS components are optimized working sets.
- Dematerializing an agent writes all authoritative changes back to the record.
- Materializing an agent initializes it from the record.
- A record cannot have two materialized entities.
- A detailed entity cannot exist without a corresponding record unless it is explicitly ephemeral.

---

## 8. Multi-Map World Model

## 8.1 Add a Map Catalog

Replace the single `LevelMap` resource with a catalog.

```rust
#[derive(Resource, Default)]
pub struct MapCatalog {
    pub descriptors: BTreeMap<MapId, MapDescriptor>,
    pub loaded: HashMap<MapId, LoadedMap>,
}

pub struct MapDescriptor {
    pub id: MapId,
    pub kind: MapKind,
    pub dimensions: MapDimensions,
    pub parent: Option<MapId>,
    pub world_anchor: WorldAnchor,
}

pub enum MapKind {
    Region,
    City,
    District,
    BuildingInterior,
    Battlefield,
    TacticalSite,
}
```

The catalog distinguishes:

- Maps known to exist
- Maps currently loaded in memory
- Maps currently visible
- Maps currently receiving detailed simulation

These sets are not necessarily identical.

---

## 8.2 Replace `LevelMap` with `MapStack`

Each loaded map contains multiple layers.

```rust
pub struct LoadedMap {
    pub descriptor: MapDescriptor,
    pub stack: MapStack,
    pub spatial: MapSpatialIndex,
    pub revision: MapRevision,
}

pub struct MapStack {
    pub terrain: ChunkedLayer<TerrainCell>,
    pub elevation: ChunkedLayer<i16>,
    pub structures: ChunkedLayer<StructureCell>,
    pub roads: ChunkedLayer<RoadCell>,
    pub ownership: SparseChunkedLayer<OwnerId>,
    pub visibility: SparseChunkedLayer<VisibilityCell>,
}
```

Layer rules:

- Terrain is authoritative static or slowly changing world state.
- Structures are authoritative simulation state.
- Occupancy is derived from positioned entities and records.
- Visibility is viewer-dependent derived state.
- Selection and UI highlights are presentation overlays and must not be stored in authoritative map snapshots.
- Political, economic, danger, and traffic heatmaps may be derived simulation layers or presentation overlays depending on whether they affect gameplay.

---

## 8.3 Add Chunked Storage

```rust
pub const CHUNK_SIDE: usize = 32;
pub const CHUNK_AREA: usize = CHUNK_SIDE * CHUNK_SIDE;

pub struct Chunk<T> {
    pub cells: Box<[T; CHUNK_AREA]>,
    pub revision: u64,
}

pub struct ChunkedLayer<T> {
    pub chunks: HashMap<ChunkCoord, Chunk<T>>,
}
```

The chunk size is an implementation parameter, not a domain guarantee. Keep it configurable behind APIs.

Required APIs:

```rust
pub trait LayerRead<T> {
    fn get(&self, cell: CellCoord) -> Option<&T>;
}

pub trait LayerWrite<T> {
    fn get_mut(&mut self, cell: CellCoord) -> Option<&mut T>;
    fn set(&mut self, cell: CellCoord, value: T) -> Result<(), MapError>;
}

pub trait ChunkQuery {
    fn chunk_revision(&self, chunk: ChunkCoord) -> Option<u64>;
    fn loaded_chunks(&self) -> impl Iterator<Item = ChunkCoord>;
}
```

All mutations must increment an appropriate revision.

---

## 8.4 Make Spatial Indexes Map-Scoped

Replace the global spatial index with:

```rust
pub struct MapSpatialIndex {
    pub occupants: HashMap<CellCoord, SmallVec<[Entity; 4]>>,
    pub movement_blockers: HashSet<CellCoord>,
    pub sight_blockers: HashSet<CellCoord>,
}
```

The `MapCatalog` owns one spatial index per loaded map.

All movement and targeting APIs must require a `MapId`.

Bad:

```rust
fn entities_at(cell: CellCoord) -> &[Entity];
```

Good:

```rust
fn entities_at(map: MapId, cell: CellCoord) -> &[Entity];
```

The migration is incomplete while any map-sensitive API silently assumes an active global map.

---

## 9. Simulation Time and Scheduling

## 9.1 Preserve Tactical Action Scheduling

The existing tactical action queue and timeline remain useful inside `tactical_sim`.

Do not stretch the tactical actor scheduler to handle every city and strategic process.

The tactical scheduler should become one client of a larger calendar.

---

## 9.2 Add a Domain Simulation Clock

```rust
#[derive(Resource, Debug, Clone, Copy)]
pub struct SimClock {
    pub minute: u64,
    pub paused: bool,
    pub speed: SimSpeed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimSpeed {
    Paused,
    Normal,
    Fast,
    VeryFast,
}
```

Do not derive domain time directly from frame count.

Do not use render delta time inside authoritative systems.

---

## 9.3 Add Multi-Cadence Schedules

```rust
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TacticalStep;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MinuteStep;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HourStep;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DayStep;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct StrategicStep;
```

Example responsibilities:

| Schedule | Typical work |
|---|---|
| `TacticalStep` | Local movement, attacks, immediate effects |
| `MinuteStep` | Citizen tasks, local traffic, short battle exchanges |
| `HourStep` | Production, market clearing, construction progress |
| `DayStep` | Consumption, wages, recruitment, demographic changes |
| `StrategicStep` | Army route movement, strategic AI, trade movement |

A domain system must run at the coarsest cadence that still produces correct gameplay.

---

## 9.4 Add a Work Budget

Fast-forwarding a city can generate more work than fits in one rendered frame.

```rust
#[derive(Resource)]
pub struct SimulationWorkBudget {
    pub maximum_steps_per_frame: usize,
    pub maximum_domain_events_per_frame: usize,
}
```

The driver must:

1. Determine target simulated time.
2. Process due schedules in deterministic order.
3. Stop when the work budget is exhausted.
4. Continue next rendered frame without changing semantic order.
5. Expose backlog metrics to UI and tests.

Simulation speed changes how aggressively target time advances. It must not change the order or results of domain operations.

---

## 9.5 Define Deterministic Schedule Ordering

When multiple cadences are due at the same simulated minute, execute them in a documented order.

Recommended order:

```text
TacticalStep
→ MinuteStep
→ HourStep
→ DayStep
→ StrategicStep
→ derived-data rebuild
→ presentation notification emission
```

The exact order may change, but it must be:

- Explicit
- Tested
- Stable across platforms
- Included in replay semantics

Never rely on iteration order from `HashMap` for authoritative processing. Use stable sorting or ordered collections where outcome order matters.

---

## 10. Simulation Fidelity Architecture

## 10.1 Define Fidelity Levels

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SimulationFidelity {
    Statistical,
    Aggregate,
    Detailed,
}
```

Interpretation:

- `Statistical`: summarized quantities and rates
- `Aggregate`: households, buildings, cohorts, or formations
- `Detailed`: individually materialized tactical agents

The level is domain-specific. A city district may be aggregate while one market is detailed. An army may be aggregate while one battle sector is detailed.

---

## 10.2 Add Fidelity Policies

```rust
pub struct FidelityRequest {
    pub subject: FidelitySubject,
    pub desired: SimulationFidelity,
    pub reason: FidelityReason,
    pub priority: u16,
}

pub enum FidelityReason {
    CameraInterest,
    PlayerSelection,
    ActiveBattle,
    ScriptedEvent,
    DebugInspection,
    SimulationRequirement,
}
```

Camera zoom may request detail, but the fidelity manager decides whether to grant it.

Do not directly materialize thousands of agents because the camera crossed a zoom threshold.

---

## 10.3 Implement Promotion and Demotion

Each promotable domain type must define:

- Source aggregate state
- Generated detailed state
- Stable identity mapping
- Conservation validation
- Fold-back logic

Example city promotion:

```text
District population aggregate
→ select households required in active area
→ materialize household members
→ assign current tasks and positions
→ validate total population and inventories
```

Example military promotion:

```text
Formation aggregate
→ generate tactical subunits or representatives
→ assign battlefield positions
→ run local combat
→ fold casualties, fatigue, morale, and equipment back
```

Promotion must be deterministic given:

- Aggregate state
- Simulated time
- Relevant random-stream state
- Promotion context

---

## 10.4 Conservation Ledger

Introduce a debugging ledger for quantities that must survive fidelity transitions.

```rust
pub struct ConservationSnapshot {
    pub population: u64,
    pub military_manpower: u64,
    pub currency: i128,
    pub food_units: i128,
    pub equipment_units: BTreeMap<ItemKindId, i128>,
}
```

For every promotion and demotion:

```text
capture before
→ perform transition
→ capture after
→ compare allowed deltas
```

Allowed deltas must be explicit, such as casualties that occurred while detailed.

Add debug assertions and property tests.

---

## 11. City Simulation

## 11.1 Add Core City Hierarchy

```text
Settlement
├── Districts
├── Buildings
├── Households
├── People
├── Institutions
├── Markets
└── Infrastructure networks
```

Recommended records:

```rust
pub struct SettlementRecord {
    pub id: SettlementId,
    pub districts: Vec<DistrictId>,
    pub treasury: i64,
    pub population_summary: PopulationSummary,
}

pub struct DistrictRecord {
    pub id: DistrictId,
    pub settlement: SettlementId,
    pub buildings: Vec<BuildingId>,
    pub population_summary: PopulationSummary,
    pub land_use: LandUseSummary,
}

pub struct HouseholdRecord {
    pub id: HouseholdId,
    pub members: Vec<PersonId>,
    pub residence: Option<BuildingId>,
    pub wealth: i64,
    pub inventories: InventorySummary,
}

pub struct BuildingRecord {
    pub id: BuildingId,
    pub district: DistrictId,
    pub kind: BuildingKindId,
    pub capacity: BuildingCapacity,
    pub condition: u16,
    pub owner: Option<OwnerId>,
}
```

---

## 11.2 Use Households as Primary Consumption Units

Do not simulate every routine household purchase as an independent person-to-merchant transaction.

Households should own:

- Food inventory
- Wealth
- Residence
- Dependents
- Consumption demand
- Basic purchasing decisions

Individuals should own:

- Identity
- Age
- Health
- Occupation
- Relationships
- Allegiances
- Current task when materialized
- Exceptional decisions

This division permits a large population while retaining individual history.

---

## 11.3 Add Citizen Task Abstraction

```rust
pub enum CitizenTask {
    RemainAt(BuildingId),
    TravelTo(Location),
    WorkAt(BuildingId),
    BuyGoods(MarketId),
    DeliverGoods(DeliveryId),
    AttendInstitution(BuildingId),
    Socialize(Location),
    Flee(DangerId),
    ServeFormation(FormationId),
}
```

At aggregate fidelity, tasks may resolve as probabilities, capacities, and flow rates.

At detailed fidelity, tasks generate local pathfinding and tactical movement.

The task is authoritative. The animated path is a detailed realization of the task.

---

## 11.4 Separate Hot and Cold Person Data

Hot data belongs in ECS or compact active arrays:

- Materialized position
- Current task
- Immediate health state
- Movement
- Local threat response
- Current workplace

Cold data belongs in registries:

- Full name
- Biography
- Ancestry
- Relationship history
- Long-term reputation
- Generated descriptions
- Historical event references

Do not place large strings or history vectors on every active ECS entity.

---

## 11.5 Introduce Flow-Based Economy First

The first city economy should model:

- Inventories
- Production recipes
- Labor capacity
- Household demand
- Transport capacity
- Market price or scarcity signals

Do not begin with independent bargaining agents for every item.

Recommended pipeline:

```text
producers publish supply
→ households and institutions publish demand
→ market allocates goods
→ inventories and currency update
→ scarcity and price signals update
```

The economy must be deterministic for a fixed state and RNG stream.

---

## 11.6 Add Infrastructure Networks

Roads, water, walls, and transport should be modeled as graph or map-layer networks.

Examples:

```rust
pub struct RoadNetwork {
    pub nodes: BTreeMap<RoadNodeId, RoadNode>,
    pub edges: BTreeMap<RoadEdgeId, RoadEdge>,
}

pub struct WaterNetwork {
    pub sources: Vec<WaterSourceId>,
    pub capacities: BTreeMap<NetworkEdgeId, u32>,
}
```

Use map cells for display and local routing, but use graph abstractions for city-wide flow calculations where possible.

---

## 12. Military Simulation

## 12.1 Add Army and Formation Records

```rust
pub struct ArmyRecord {
    pub id: ArmyId,
    pub commander: Option<PersonId>,
    pub formations: Vec<FormationId>,
    pub location: Location,
    pub supplies: ArmySupply,
    pub objective: ArmyObjective,
}

pub struct FormationRecord {
    pub id: FormationId,
    pub army: ArmyId,
    pub parent: Option<FormationId>,
    pub echelon: Echelon,
    pub manpower: u32,
    pub frontage: u32,
    pub cohesion: u16,
    pub morale: u16,
    pub fatigue: u16,
    pub training: u16,
    pub equipment: EquipmentSummary,
}
```

Do not represent every soldier as an ECS entity at strategic or ordinary battlefield fidelity.

Named commanders and notable soldiers may remain individual `PersonRecord`s linked to formations.

---

## 12.2 Add Strategic Army Movement

Strategic army movement operates over routes, terrain regions, and supply constraints.

```rust
pub enum ArmyObjective {
    Hold(Location),
    MoveTo(Location),
    Besiege(SettlementId),
    Intercept(ArmyId),
    Raid(RegionId),
    RetreatTo(Location),
}
```

Movement cost may depend on:

- Terrain
- Road quality
- Formation size
- Baggage
- Weather
- Supply
- Enemy control
- Fatigue

Strategic movement must not require a tactical map to be loaded.

---

## 12.3 Add Supply and Attrition

Supply should be explicit and independently testable.

```rust
pub struct ArmySupply {
    pub food_days: Fixed,
    pub ammunition: EquipmentSummary,
    pub replacement_equipment: EquipmentSummary,
    pub transport_capacity: u32,
}
```

Daily military processing should handle:

- Consumption
- Resupply
- Foraging
- Desertion
- Disease or non-combat attrition
- Fatigue recovery
- Morale effects

---

## 12.4 Add Formation-Level Battle Resolution

Model a battle as fronts between formations.

```rust
pub struct BattleRecord {
    pub id: BattleId,
    pub battlefield: MapId,
    pub participants: Vec<ArmyId>,
    pub fronts: Vec<CombatFront>,
    pub state: BattleState,
}

pub struct CombatFront {
    pub attackers: Vec<FormationId>,
    pub defenders: Vec<FormationId>,
    pub sector: BattlefieldSectorId,
    pub width: u32,
    pub terrain: BattlefieldTerrain,
}
```

Recommended exchange:

```text
establish contact
→ assign formations to fronts
→ compute effective frontage
→ compute local advantage
→ apply casualties, fatigue, and cohesion loss
→ perform morale checks
→ advance, hold, withdraw, or rout
→ update battle geometry
```

Avoid all-pairs combat between every soldier or formation.

---

## 12.5 Add Tactical Projection as an Adapter

Tactical projection converts a formation-level battle sector into tactical entities.

```rust
pub struct TacticalProjection {
    pub battle: BattleId,
    pub sector: BattlefieldSectorId,
    pub map: MapId,
    pub projected_formations: BTreeMap<FormationId, ProjectedFormation>,
}
```

A projected formation may become:

- Several tactical subunits
- Representative sprites
- ASCII counters
- Named officers
- A local command entity

It does not need to become one entity per soldier.

When the tactical projection ends:

1. Resolve remaining tactical effects.
2. Count casualties and equipment loss.
3. Compute resulting morale, fatigue, and cohesion.
4. Update formation records.
5. Remove projected entities.
6. Validate conservation.

---

## 13. Semantic Presentation Architecture

## 13.1 Introduce `VisualKey`

Simulation code must not expose atlas indices, glyphs, or final colors.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VisualKey {
    Terrain(TerrainVisual),
    Structure(StructureVisual),
    Person(PersonVisual),
    Formation(FormationVisual),
    Item(ItemVisual),
    Infrastructure(InfrastructureVisual),
    Overlay(OverlayVisual),
}
```

Examples:

```rust
pub enum PersonVisual {
    Civilian,
    Worker(OccupationCategory),
    Soldier(FactionId),
    Official(FactionId),
    Injured,
    Dead,
}
```

---

## 13.2 Introduce `VisualDescriptor`

A semantic adapter converts simulation state into a renderer-neutral descriptor.

```rust
pub struct VisualDescriptor {
    pub key: VisualKey,
    pub position: ViewPosition,
    pub layer: VisualLayer,
    pub color_tokens: SmallVec<[ColorToken; 4]>,
    pub label: Option<LabelDescriptor>,
    pub visibility: VisualVisibility,
    pub importance: VisualImportance,
}
```

`VisualDescriptor` may contain semantic color tokens but must not contain final renderer resources.

---

## 13.3 Add View Adapters

```rust
pub trait ViewAdapter {
    fn describe(
        &self,
        subject: ViewSubject,
        context: &ViewContext,
        output: &mut Vec<VisualDescriptor>,
    );
}
```

Implement adapters for:

- Maps
- Buildings
- People
- Households when aggregate markers are desired
- Formations
- Armies
- Routes
- Economic and political overlays

View adapters are allowed to read public simulation state. They must not modify it.

---

## 13.4 Add Dirty-Region Tracking

Do not rebuild an entire city visualization after every simulation step.

Track revisions at:

- Map
- Layer
- Chunk
- Entity descriptor
- Overlay data source

```rust
pub struct DirtyViewRegions {
    pub chunks: BTreeSet<(MapId, ChunkCoord)>,
    pub entities: BTreeSet<StableVisualId>,
    pub overlays: BTreeSet<OverlayId>,
}
```

Simulation systems emit semantic change notices or increment revisions. Presentation systems determine which descriptors require rebuilding.

Presentation notifications must not be authoritative and may be reconstructed from revisions after loading.

---

## 14. Camera and Semantic Zoom

## 14.1 Add Continuous Camera Zoom

Input must accumulate mouse-wheel deltas into a target projection scale.

```rust
#[derive(Resource)]
pub struct CameraZoom {
    pub current_scale: f32,
    pub target_scale: f32,
    pub minimum_scale: f32,
    pub maximum_scale: f32,
}
```

Requirements:

- Clamp scale.
- Smoothly approach target scale.
- Preserve deterministic simulation independence.
- Support zooming around the cursor when practical.
- Do not generate simulation commands for ordinary camera zoom.

---

## 14.2 Add Discrete Zoom Bands

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZoomBand {
    Individual,
    Building,
    Neighborhood,
    District,
    City,
    Region,
}
```

Map continuous scale into bands using configurable thresholds.

Use hysteresis:

```rust
pub struct ZoomThreshold {
    pub enter_scale: f32,
    pub exit_scale: f32,
}
```

Without hysteresis, visual representations will flicker near boundaries.

---

## 14.3 Separate Zoom Band from Simulation Fidelity

Zoom band controls presentation.

Simulation fidelity controls simulation detail.

They influence each other through requests, but they are not identical.

Example:

```text
player zooms to city view
→ presentation switches to district summaries
→ fidelity manager may demote unimportant citizens
→ active battle remains aggregate or detailed due to battle policy
```

Never write:

```rust
if zoom_band == ZoomBand::Individual {
    materialize_every_person();
}
```

Instead:

```rust
fidelity_requests.push(FidelityRequest {
    subject: visible_district,
    desired: SimulationFidelity::Detailed,
    reason: FidelityReason::CameraInterest,
    priority: CAMERA_PRIORITY,
});
```

---

## 14.4 Define Representation by Zoom Band

Minimum target behavior:

| Subject | Individual | Building | District | City | Region |
|---|---|---|---|---|---|
| Person | Glyph/sprite | Optional marker | Hidden or density | Population overlay | Hidden |
| Building | Detailed cells | Footprint | Simplified block | Functional color | Hidden |
| Formation | Subunits | Formation counter | Formation counter | Army banner | Army marker |
| Road | Cells | Cells/line | Simplified line | Network | Major route |
| Market | Stalls/people | Building | Trade marker | Trade heatmap | Trade route |
| Label | Selected entity | Building names | District names | City name | Region names |

The exact aesthetics are theme-controlled.

---

## 15. ASCII and Tile Rendering Backends

## 15.1 Extract ASCII Renderer

Move glyph-specific code into `renderer_ascii`.

```rust
pub struct AsciiStyle {
    pub glyph: char,
    pub foreground: ColorToken,
    pub background: Option<ColorToken>,
    pub modifiers: AsciiModifiers,
}
```

The ASCII theme maps `VisualKey` to `AsciiStyle`.

The renderer consumes `VisualDescriptor` plus the active theme.

---

## 15.2 Add Tile Renderer

```rust
pub struct TileStyle {
    pub atlas: AtlasId,
    pub index: u32,
    pub tint: ColorToken,
    pub transform: TileTransform,
}
```

The tile renderer must support:

- Multiple atlases
- Different tile sets by map kind or culture
- Animated tiles where required
- Chunk rebuilds
- Semantic zoom representations
- Mixed tile and sprite sizes
- Missing-asset fallback

Atlas IDs and indices belong only in presentation assets and themes.

---

## 15.3 Support Mixed Rendering

The architecture must allow combinations such as:

```text
terrain: graphical tiles
citizens: ASCII glyphs
formations: counters
selection: vector overlay
labels: text
```

Represent renderer choice per visual category or view, not as a global compile-time assumption.

```rust
pub enum RenderBackend {
    Ascii,
    Tiles,
    Counter,
    Overlay,
}
```

A theme or user configuration may choose the backend.

---

## 15.4 Define Renderer-Neutral Selection

Selection and hit testing must operate on semantic visual subjects.

```rust
pub struct PickResult {
    pub subject: ViewSubject,
    pub map: MapId,
    pub world_position: Vec2,
    pub priority: i32,
}
```

Do not make UI code depend directly on a sprite entity or glyph entity. Presentation entities may be recreated during chunk rebuilds.

---

## 16. Semantic Coloring System

## 16.1 Replace Literal Colors with Tokens

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ColorToken {
    TerrainStone,
    TerrainRoad,
    TerrainWater,
    StructureResidential,
    StructureCommercial,
    StructureMilitary,
    PersonCivilian,
    PersonWorker,
    MilitaryFriendly,
    MilitaryHostile,
    Selection,
    Hover,
    Warning,
    Danger,
    Hidden,
    Explored,
    Faction(FactionId),
    WealthBand(u8),
    PopulationDensity(u8),
    SupplyBand(u8),
    MoraleBand(u8),
}
```

Simulation code may produce semantic classifications such as `MoraleBand(2)`. It must not select an RGB color.

---

## 16.2 Add Palette Assets

```rust
#[derive(Asset, TypePath)]
pub struct ColorPaletteAsset {
    pub entries: HashMap<ColorTokenDef, PaletteColor>,
}
```

Recommended assets:

```text
assets/themes/
├── default/
│   ├── palette.ron
│   ├── ascii.ron
│   └── tiles.ron
├── ascii_classic/
├── high_contrast/
└── color_vision/
```

Palette loading must validate missing required tokens and provide fallback behavior.

---

## 16.3 Define Color Composition Order

Final color must be resolved in one place.

Recommended order:

```text
semantic base
→ faction or ownership tint
→ data overlay
→ lighting
→ visibility/fog
→ hover or selection
→ accessibility transform
→ renderer color-space conversion
```

```rust
pub struct ColorContext {
    pub ownership: Option<FactionId>,
    pub overlay: Option<OverlaySample>,
    pub lighting: LightingSample,
    pub visibility: VisibilityState,
    pub interaction: InteractionState,
    pub accessibility: AccessibilityColorMode,
}
```

Add golden tests for important color-resolution cases.

---

## 16.4 Add Overlay Framework

```rust
pub enum OverlayKind {
    None,
    Ownership,
    Population,
    Wealth,
    FoodSupply,
    Traffic,
    Crime,
    FireRisk,
    MilitaryControl,
    ArmySupply,
    Morale,
}
```

Overlay providers emit normalized samples:

```rust
pub struct OverlaySample {
    pub value: f32,
    pub category: Option<u16>,
    pub confidence: f32,
}
```

Presentation maps samples to tokens or gradients.

Overlay calculations that affect game rules belong in simulation. Pure visualization transforms belong in presentation.

---

## 17. Input and Command Changes

## 17.1 Separate Camera Input from Simulation Commands

Input mapping must produce distinct outputs:

```rust
pub enum AppInput {
    Camera(CameraCommand),
    Simulation(SimulationCommand),
    Ui(UiCommand),
}
```

Examples:

```rust
pub enum CameraCommand {
    Pan(Vec2),
    Zoom(f32),
    CenterOn(ViewSubject),
}

pub enum SimulationCommand {
    Tactical(TacticalCommand),
    Strategic(StrategicCommand),
    TimeControl(TimeControlCommand),
}
```

Mouse wheel should normally produce `CameraCommand::Zoom`.

It must not consume a simulation turn.

---

## 17.2 Add Contextual Selection

A click may select different subjects at different zoom bands.

Selection resolution must consider:

- Zoom band
- Active overlay
- Visual priority
- Current tool or mode
- Map under cursor

At city zoom, clicking may select a district. At individual zoom, the same screen location may select a person or building tile.

---

## 18. Persistence Migration

## 18.1 Introduce a New Save Schema Version

The new schema must store:

- Global simulation time
- ID allocator state
- Random stream states
- Map catalog and descriptors
- Loaded or persisted map chunks
- Settlements
- Districts
- Buildings
- Households
- People
- Armies
- Formations
- Battles
- Fidelity state where necessary
- Tactical projections where save-during-battle is supported
- User presentation preferences separately from authoritative campaign state

---

## 18.2 Separate Campaign and Presentation Saves

Recommended files:

```text
campaign.save
user-view-settings.ron
```

Campaign save includes authoritative simulation state.

View settings include:

- Active renderer
- Theme
- Palette
- Camera position
- Zoom scale
- Active overlay
- UI layout

Camera and palette preferences must not affect deterministic campaign checksums.

---

## 18.3 Add Old-Save Migration

The original save contains a tactical level and tactical entities.

Migration should create:

```text
one `MapDescriptor` of kind `TacticalSite`
one `LoadedMap` with terrain and structure layers
one settlement or site container if required
tactical actors with stable domain IDs
a default simulation clock
empty city and military registries
```

Do not promise perfect migration of saves created during intermediate development versions unless required. Support at least the last declared stable schema.

---

## 18.4 Normalize Snapshot Hashing

Deterministic snapshot hashes must exclude:

- Bevy entity IDs
- Hash-map iteration order
- Presentation entities
- Camera state
- Dirty-region caches
- Loaded asset handles
- Frame counters
- Non-authoritative visual RNG

Sort records and references by stable ID before hashing.

---

## 19. Crate Extraction Sequence

Do not begin by splitting every module. Use this exact extraction order unless a concrete repository constraint requires otherwise.

### 19.1 Extract `sim_core`

Move:

- Stable IDs
- ID allocation
- RNG streams
- Domain clock
- Work budgeting
- Generic command/effect infrastructure
- Snapshot versioning utilities

Leave tactical actions in the old crate until `tactical_sim` is ready.

Acceptance:

- `sim_core` has no dependency on full `bevy`.
- Headless ID, clock, and RNG tests pass.
- Existing game compiles through adapters.

### 19.2 Extract `world_model`

Move:

- Map IDs
- Coordinates
- Map catalog
- Chunk storage
- Map layers
- Map-scoped spatial index
- Route and geography primitives

Adapt the original `LevelMap` into a one-map `MapCatalog`.

Acceptance:

- Existing tactical level renders through the map catalog.
- Movement and FOV require `MapId`.
- Multi-map test can load two maps with independent occupancy.

### 19.3 Extract `tactical_sim`

Move:

- Actions
- Actor components
- Tactical scheduler
- Movement
- Combat
- Effects
- FOV
- Tactical AI
- Tactical persistence adapters

Acceptance:

- Original vertical slice behaves unchanged.
- All original headless tests pass.
- `tactical_sim` depends on `world_model` and `sim_core`, not the application.

### 19.4 Extract `presentation`

Move:

- Renderer-neutral visual descriptions
- Zoom bands
- Palette and color tokens
- Selection subjects
- Dirty-region tracking
- View adapters

Initially, adapt existing rendering through these abstractions.

Acceptance:

- No simulation component contains a final color, glyph, or atlas index.
- Existing renderer consumes `VisualDescriptor`.

### 19.5 Extract `renderer_ascii`

Move all glyph-specific behavior.

Acceptance:

- ASCII rendering works without tile-renderer code.
- ASCII theme can be replaced at runtime or load time.
- Renderer does not query arbitrary simulation components.

### 19.6 Add `renderer_tiles`

Implement the graphical backend after semantic presentation is stable.

Acceptance:

- Same map can render in ASCII or tiles.
- Switching backend does not alter simulation hash.
- At least two tile themes or atlases can coexist.

### 19.7 Add `city_sim`

Begin with settlement, district, building, household, and population summaries.

Do not begin with detailed citizens.

Acceptance:

- A settlement can advance for one simulated year headlessly.
- Population, food, and currency invariants hold.
- No rendering crate is required.

### 19.8 Add `military_sim`

Begin with armies, formations, movement, supply, and aggregate battles.

Acceptance:

- Two armies can move, meet, fight, and produce deterministic results.
- Formation casualties conserve manpower.
- Battle simulation does not require tactical materialization.

### 19.9 Add Fidelity Management

Add promotion and demotion only after aggregate city and military systems exist.

Acceptance:

- A district can materialize a subset of citizens and fold them back.
- A formation can produce tactical representatives and fold battle results back.
- Conservation tests pass.

### 19.10 Rename `rogue_app` to `game_app`

Perform the application rename last, after dependencies are stable.

---

## 20. Migration Phases and Deliverables

## Phase 0: Characterize the Existing System

### Tasks

- Record current workspace and module boundaries.
- Add or repair deterministic replay tests.
- Add save/load round-trip tests.
- Add spatial-index invariant tests.
- Add a normalized tactical-world hash.
- Document all direct uses of:
  - `LevelMap`
  - `LevelId`
  - `GridPosition`
  - literal colors
  - glyph components
  - atlas indices
  - global spatial index
  - direct camera assumptions

### Exit Criteria

- Current behavior is covered well enough to detect accidental changes.
- A baseline replay fixture is committed.
- The existing vertical slice is runnable.

---

## Phase 1: Shared Identity, Time, and Determinism

### Tasks

- Introduce typed stable IDs.
- Centralize ID allocation.
- Replace durable `Entity` references.
- Add `SimClock`.
- Add work-budget infrastructure.
- Separate authoritative and presentation RNG.
- Add stable ordered iteration utilities.

### Exit Criteria

- Save snapshots use stable IDs.
- Replay results do not depend on Bevy entity allocation.
- No authoritative system uses render delta time.

---

## Phase 2: Multi-Map and Chunked World

### Tasks

- Add `MapId`, `MapCatalog`, and `MapDescriptor`.
- Wrap existing level as a map.
- Add chunked layers.
- Move terrain, structures, and visibility into separate layers.
- Scope spatial indexes by map.
- Update movement, pathfinding, FOV, and targeting APIs.
- Add map load/unload lifecycle.

### Exit Criteria

- Two maps can coexist.
- An actor on map A does not affect occupancy or FOV on map B.
- Existing tactical gameplay operates on one catalog map.
- Chunk revisions update correctly.

---

## Phase 3: Semantic Presentation, Zoom, and Color

### Tasks

- Introduce `VisualKey` and `VisualDescriptor`.
- Add view adapters.
- Replace literal colors with `ColorToken`.
- Add palette assets.
- Add overlay framework.
- Implement continuous mouse-wheel zoom.
- Add `ZoomBand` with hysteresis.
- Add dirty-region tracking.

### Exit Criteria

- Simulation crates contain no final colors or renderer handles.
- Mouse-wheel zoom is smooth and consumes no simulation turn.
- At least three zoom bands visibly change representation.
- One overlay, such as population or ownership, works through the color system.

---

## Phase 4: Renderer Extraction

### Tasks

- Extract ASCII renderer.
- Add tile renderer.
- Support multiple tile themes.
- Support mixed backends.
- Add renderer-neutral picking and selection.

### Exit Criteria

- ASCII and tiles render the same simulation.
- Backend selection does not change authoritative state.
- Missing assets fall back safely.
- Renderer entities may be destroyed and rebuilt without losing selection identity.

---

## Phase 5: City Aggregate Simulation

### Tasks

- Add settlements and districts.
- Add building records and land use.
- Add households and population summaries.
- Add production and consumption.
- Add market allocation.
- Add district-level movement or flow.
- Add city overlays.
- Add year-scale deterministic simulation tests.

### Exit Criteria

- A large city can advance headlessly.
- Population, wealth, food, and inventories remain internally consistent.
- Performance does not depend linearly on every citizen making detailed decisions.

---

## Phase 6: Detailed Citizen Materialization

### Tasks

- Add person records.
- Add active-agent ECS components.
- Add citizen tasks.
- Add household-to-person relationships.
- Add materialization index.
- Promote visible or important citizens.
- Demote inactive citizens.
- Add conservation and identity tests.

### Exit Criteria

- A selected district can display active individuals.
- Leaving the district dematerializes them.
- Named individuals preserve identity and history.
- Population totals do not change during promotion or demotion.

---

## Phase 7: Army and Formation Simulation

### Tasks

- Add army and formation records.
- Add route movement.
- Add supply and attrition.
- Add battle records and fronts.
- Add morale, fatigue, cohesion, and casualties.
- Add strategic and battle overlays.
- Add deterministic battle fixtures.

### Exit Criteria

- Large armies can clash without individual soldier entities.
- Frontage and terrain affect outcomes.
- Manpower and equipment losses are conserved.
- Battle results are deterministic for a fixed seed and state.

---

## Phase 8: Tactical Battle Projection

### Tasks

- Create battlefield maps.
- Project selected formations into tactical representatives.
- Link tactical units to parent formations.
- Run tactical actions using `tactical_sim`.
- Fold tactical results back into military records.
- Add save/load support for active projections if required.

### Exit Criteria

- Zooming or selecting a battle can show a tactical sector.
- Tactical casualties update aggregate formations exactly once.
- Removing the tactical projection does not remove the battle record.
- Conservation tests pass.

---

## Phase 9: Persistence and Cleanup

### Tasks

- Finalize campaign snapshot schema.
- Add migrations from original architecture.
- Separate presentation settings.
- Remove compatibility APIs.
- Rename final crates and modules.
- Remove deprecated single-map resources.
- Update `architecture.md` or mark it as historical.
- Make this document the current implementation reference.

### Exit Criteria

- No production code assumes one map.
- No production code assumes one simulation cadence.
- No production code assumes all people or soldiers are detailed entities.
- No simulation code depends on rendering.
- All stable save migrations and replay tests pass.

---

## 21. File-by-File Migration Map

Use this as the initial mapping from the original layout.

| Original path | Target |
|---|---|
| `rogue_core/src/time/*` | `sim_core/src/time.rs`, `schedule.rs`, and `work_budget.rs` |
| `rogue_core/src/persistence/migration.rs` | `sim_core/src/persistence/migration.rs` |
| `rogue_core/src/world/map.rs` | `world_model/src/map_catalog.rs` and `map_stack.rs` |
| `rogue_core/src/world/tile.rs` | `world_model/src/layers/terrain.rs` and `structures.rs` |
| `rogue_core/src/world/spatial.rs` | `world_model/src/spatial/` |
| `rogue_core/src/world/fov.rs` | `tactical_sim/src/fov/` |
| `rogue_core/src/world/generation.rs` | Initially `tactical_sim`, later map-kind-specific generation modules |
| `rogue_core/src/action/*` | `tactical_sim/src/action/` |
| `rogue_core/src/actor/*` | `tactical_sim/src/actor/` |
| `rogue_core/src/item/*` | `tactical_sim` initially; shared inventories may later move to domain modules |
| `rogue_core/src/content/*` | Split between domain definition crates and `game_app` asset loading |
| `rogue_app/src/presentation/*` | `presentation`, `renderer_ascii`, and `renderer_tiles` |
| `rogue_app/src/input/*` | `game_app/src/input/` |
| `rogue_app/src/ui/*` | `game_app/src/ui/` |
| `rogue_app/src/assets/*` | `game_app/src/assets/` |
| `rogue_app/src/persistence/*` | `game_app/src/save_files/` |

Do not move files mechanically without first separating their domain and renderer dependencies.

---

## 22. Required Cross-Domain APIs

Cross-domain communication must use narrow commands, queries, and events.

Examples:

```rust
pub enum WorldCommand {
    CreateMap(CreateMapCommand),
    LoadMap(MapId),
    UnloadMap(MapId),
}

pub enum CityCommand {
    FoundSettlement(FoundSettlementCommand),
    SetPolicy(SetPolicyCommand),
}

pub enum MilitaryCommand {
    MoveArmy {
        army: ArmyId,
        destination: Location,
    },
    Engage {
        attacker: ArmyId,
        defender: ArmyId,
    },
}

pub enum TacticalCommand {
    Move {
        actor: TacticalActorId,
        delta: CellDelta,
    },
    Attack {
        actor: TacticalActorId,
        target: TacticalActorId,
    },
}
```

Domain events use stable IDs:

```rust
pub enum DomainEvent {
    PersonDied(PersonId),
    BuildingDestroyed(BuildingId),
    ArmyArrived(ArmyId, Location),
    BattleStarted(BattleId),
    FormationRouted(FormationId),
}
```

Do not use presentation observers as the primary mechanism for cross-domain simulation.

---

## 23. Performance Rules

### 23.1 Do Not Query the Entire World Per Tick

Systems must operate on:

- Due records
- Active chunks
- Changed entities
- Scheduled tasks
- Relevant districts
- Active battles

Avoid:

```text
for every person
    every minute
        scan every building
```

Prefer indexed work queues and scheduled due times.

### 23.2 Use Coarse Simulation by Default

Detailed simulation is the exception.

Default fidelity for off-screen population and armies should be statistical or aggregate.

### 23.3 Cache Derived Data with Revisions

Examples:

- Chunk meshes
- District summaries
- Route costs
- FOV
- Overlay samples
- Building footprints
- Formation effective strength

Every cache must have:

- Inputs
- Revision or invalidation rule
- Rebuild path
- Exclusion from authoritative snapshots where appropriate

### 23.4 Profile Before Specialized Optimization

Do not introduce unsafe code, custom allocators, or complex parallel algorithms until profiling identifies a bottleneck.

Architectural scalability should first come from:

- Fidelity levels
- Chunking
- Scheduled work
- Aggregation
- Sparse processing
- Dirty-region updates

---

## 24. Concurrency Rules

Parallel systems may process independent:

- Maps
- Chunks
- Districts
- Battles
- Formations
- Households

However, deterministic results require controlled merge steps.

Recommended pattern:

```text
parallel read-only evaluation
→ produce local deterministic outputs
→ stable sort by domain key
→ serial or partitioned application
```

Do not allow parallel mutation order to influence:

- ID allocation
- RNG consumption
- Market allocation
- Casualties
- Event ordering
- Save hashes

Use domain-separated RNG streams or deterministic keyed random functions for parallelizable work.

---

## 25. Testing Requirements

## 25.1 Multi-Map Tests

- Two maps maintain separate occupancy.
- Map loading and unloading preserves snapshots.
- Moving between maps updates both spatial indexes.
- Chunk mutation updates only the affected revision.

## 25.2 Zoom and Renderer Tests

- Zoom hysteresis prevents band flicker.
- Camera zoom does not advance simulation time.
- ASCII and tile renderers produce descriptors for the same semantic subjects.
- Changing theme does not change campaign hash.
- Selection survives renderer rebuild.

## 25.3 City Tests

- Population is conserved across district promotion.
- Household members are unique.
- Goods cannot exist in two inventories.
- Production consumes required inputs.
- Market allocation is deterministic.
- A city can run for a simulated year without invalid state.

## 25.4 Military Tests

- Army manpower equals the sum of subordinate formations.
- Casualties are applied once.
- Routed formations cannot remain in an attacking front.
- Supply consumption is deterministic.
- Tactical projection fold-back conserves manpower and equipment.

## 25.5 Replay Tests

Maintain separate fixtures for:

- Tactical scenario
- City month
- City year
- Army march
- Formation battle
- Tactical battle projection

Each fixture should compare a normalized authoritative hash.

---

## 26. Acceptance Scenario for the Fully Formed Architecture

The target architecture is demonstrated by the following integrated scenario:

1. Load a regional map containing a large city and surrounding routes.
2. View the city at region zoom using ASCII or graphical tiles.
3. Zoom into city view and display district land use and population.
4. Enable a food-supply overlay using semantic color tokens.
5. Zoom into a district and display buildings.
6. Zoom into a market and materialize a bounded subset of citizens.
7. Observe individual citizens performing tasks without materializing the entire city.
8. Advance time rapidly and allow the city economy to continue at aggregate fidelity.
9. Move two armies toward each other on the regional map.
10. Resolve supply consumption during the march.
11. Create a battle when the armies meet.
12. Resolve most of the battle at formation fidelity.
13. Select a battle sector and create a tactical projection.
14. Render the sector in either ASCII or graphical tiles.
15. Resolve tactical movement and combat.
16. Fold casualties and morale effects into parent formations.
17. Return to city zoom without losing battle or citizen identity.
18. Save, reload, and reproduce the same normalized authoritative state.

---

## 27. Explicit Non-Goals During Migration

Do not make the following prerequisites for the architecture migration:

- Fully realistic Roman economics
- One ECS entity per citizen
- One ECS entity per soldier
- A complete political simulation
- Multiplayer networking
- Distributed simulation
- GPU-driven authoritative simulation
- Full 3D rendering
- Mod scripting
- Procedural generation for every map type
- Perfect migration from every experimental save version

These may be added later. They must not distort the core boundaries.

---

## 28. Completion Checklist

The migration is complete when all statements below are true.

### Simulation

- [ ] The simulation runs headlessly.
- [ ] Domain time is independent of frame time.
- [ ] Multiple simulation cadences are supported.
- [ ] Fast-forward uses a deterministic work budget.
- [ ] Stable IDs are used for durable references.
- [ ] Authoritative RNG is isolated from presentation RNG.

### World

- [ ] Multiple maps coexist.
- [ ] Maps contain chunked layers.
- [ ] Spatial indexes are map-scoped.
- [ ] Hierarchical locations are supported.
- [ ] Map and chunk revisions drive derived-data updates.

### City

- [ ] Settlements, districts, buildings, households, and people have records.
- [ ] City simulation works at aggregate fidelity.
- [ ] Detailed citizens can be materialized and dematerialized.
- [ ] Promotion and demotion conserve population and inventories.

### Military

- [ ] Armies and formations exist without individual soldier entities.
- [ ] Strategic movement and supply are modeled.
- [ ] Formation-level battles are deterministic.
- [ ] Tactical projections fold results back exactly once.

### Presentation

- [ ] `VisualKey` and `VisualDescriptor` are renderer-neutral.
- [ ] Continuous camera zoom works.
- [ ] Semantic zoom bands work with hysteresis.
- [ ] ASCII and tile renderers are interchangeable.
- [ ] Multiple tile themes are supported.
- [ ] Selection is independent of presentation entity lifetime.

### Color

- [ ] Simulation code contains no final display colors.
- [ ] Semantic color tokens are used.
- [ ] Palette assets resolve tokens.
- [ ] Overlay composition is centralized.
- [ ] Accessibility transforms can be applied.

### Persistence

- [ ] Campaign snapshots include all authoritative domains.
- [ ] Presentation settings are separate.
- [ ] Old stable saves migrate or fail with a clear version error.
- [ ] Normalized hashes exclude presentation state.
- [ ] Integrated save/load determinism tests pass.

---

## 29. Final Architectural Rule

When adding any new feature, the implementation agent must answer these questions before choosing a representation:

1. At what simulation fidelity does this feature need to operate?
2. At what cadence does it need to update?
3. What is its stable identity?
4. What location scale does it use?
5. Is its state authoritative or derived?
6. Which map or map layer owns its spatial data?
7. How is it promoted or demoted?
8. What quantities must be conserved?
9. How does it become a semantic visual description?
10. Can ASCII, tile, and overlay renderers all consume that description?
11. How is it serialized and migrated?
12. How is deterministic behavior tested?

A feature is not architecturally complete until those questions have concrete answers.

use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use serde::{Deserialize, Serialize};

use crate::action::intent::{Action, ActionKind, ActionTarget};
use crate::action::queue::ActionQueue;
use crate::actor::combat::{DamageKind, StatusEffect};
use crate::actor::components::{
    ActionSpeed, ActiveStatuses, Actor, AiGoal, BlocksMovement, BlocksSight, CombatStats, Health,
    HostileToPlayer, Monster, PersistentId, PersistentIdAllocator, Player, PrototypeId,
    StableActorId, StableEntityIndex, StableItemId, Vision,
};
use crate::item::components::{CarriedBy, Inventory, Item};
use crate::item::effects::{Effect, EffectQueue};
use crate::simulation::{SimulationDriverState, SimulationStatus};
use crate::time::clock::{CurrentActor, ScheduledActor, TurnClock};
use crate::world::fov::recalculate_fov_for_player;
use crate::world::map::{GridPosition, LevelId, LevelMap};
use crate::world::spatial::SpatialIndex;
use crate::world::tile::{Tile, TileKind};
use sim_core::Cadence;

use super::migration::{LegacyGameSnapshotV1, LegacyGameSnapshotV2, SnapshotFile};
use super::rng::{RandomSnapshot, RandomStreams};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedPosition {
    pub level: u32,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedHealth {
    pub current: i32,
    pub maximum: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedCombatStats {
    pub power: i32,
    pub defense: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedVision {
    pub range: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedActionSpeed {
    pub ticks_per_action: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedLastKnownPlayerPosition {
    pub level: u32,
    pub x: i32,
    pub y: i32,
    pub observed_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct TileSnapshot {
    pub kind: TileKind,
    pub explored: bool,
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LevelSnapshot {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub tiles: Vec<TileSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedInventory {
    pub capacity: usize,
    pub items: Vec<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AiGoalSnapshot {
    Idle,
    Wander,
    Investigate(SavedPosition),
    Chase(u64),
    Flee(u64),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActionTargetSnapshot {
    SelfTarget,
    Entity(u64),
    Cell { level: u32, x: i32, y: i32 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActionKindSnapshot {
    Wait,
    Move {
        dx: i32,
        dy: i32,
    },
    Melee {
        target: u64,
    },
    PickUp {
        item: u64,
    },
    Drop {
        item: u64,
    },
    UseItem {
        item: u64,
        target: ActionTargetSnapshot,
    },
    Descend,
    Ascend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionSnapshot {
    pub actor: u64,
    pub kind: ActionKindSnapshot,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EffectSnapshot {
    Damage {
        source: Option<u64>,
        target: u64,
        amount: i32,
        kind: DamageKind,
    },
    Heal {
        target: u64,
        amount: i32,
    },
    Teleport {
        target: u64,
        destination: SavedPosition,
    },
    ApplyStatus {
        target: u64,
        status: StatusEffect,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledActorSnapshot {
    pub next_tick: u64,
    pub sequence: u64,
    pub actor: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistentIdAllocatorSnapshot {
    pub next_available: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SimulationStatusSnapshot {
    WaitingForPlayer,
    Resolving,
    GameOver,
}

impl From<SimulationStatus> for SimulationStatusSnapshot {
    fn from(value: SimulationStatus) -> Self {
        match value {
            SimulationStatus::WaitingForPlayer => Self::WaitingForPlayer,
            SimulationStatus::Resolving => Self::Resolving,
            SimulationStatus::GameOver => Self::GameOver,
        }
    }
}

impl From<SimulationStatusSnapshot> for SimulationStatus {
    fn from(value: SimulationStatusSnapshot) -> Self {
        match value {
            SimulationStatusSnapshot::WaitingForPlayer => Self::WaitingForPlayer,
            SimulationStatusSnapshot::Resolving => Self::Resolving,
            SimulationStatusSnapshot::GameOver => Self::GameOver,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntitySnapshot {
    pub id: u64,
    pub prototype: String,
    pub actor: bool,
    pub player: bool,
    pub monster: bool,
    pub item: bool,
    pub blocks_movement: bool,
    pub blocks_sight: bool,
    pub hostile_to_player: bool,
    pub position: Option<SavedPosition>,
    pub health: Option<SavedHealth>,
    pub combat_stats: Option<SavedCombatStats>,
    pub vision: Option<SavedVision>,
    pub action_speed: Option<SavedActionSpeed>,
    pub inventory: Option<SavedInventory>,
    pub carried_by: Option<u64>,
    pub ai_goal: Option<AiGoalSnapshot>,
    pub last_known_player_position: Option<SavedLastKnownPlayerPosition>,
    #[serde(default)]
    pub active_statuses: Vec<StatusEffect>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameSnapshot {
    pub version: u32,
    pub root_seed: u64,
    pub current_level: u32,
    pub current_tick: u64,
    pub next_sequence: u64,
    pub current_actor: Option<u64>,
    pub simulation_status: SimulationStatusSnapshot,
    pub persistent_ids: PersistentIdAllocatorSnapshot,
    pub levels: Vec<LevelSnapshot>,
    pub entities: Vec<EntitySnapshot>,
    pub timeline: Vec<ScheduledActorSnapshot>,
    pub pending_actions: Vec<ActionSnapshot>,
    pub pending_effects: Vec<EffectSnapshot>,
    pub simulation_driver: SimulationDriverState,
    pub rng: RandomSnapshot,
}

pub type SnapshotResult<T> = Result<T, String>;

fn position_to_saved(position: GridPosition) -> SavedPosition {
    SavedPosition {
        level: position.level.0,
        x: position.cell.x,
        y: position.cell.y,
    }
}

fn saved_to_position(position: &SavedPosition) -> GridPosition {
    GridPosition {
        level: LevelId(position.level),
        cell: IVec2::new(position.x, position.y),
    }
}

fn pid_of(entity: Entity, ids: &HashMap<Entity, u64>) -> SnapshotResult<u64> {
    ids.get(&entity)
        .copied()
        .ok_or_else(|| format!("missing persistent id for entity {:?}", entity))
}

fn action_target_to_snapshot(target: &ActionTarget) -> SnapshotResult<ActionTargetSnapshot> {
    match target {
        ActionTarget::SelfTarget => Ok(ActionTargetSnapshot::SelfTarget),
        ActionTarget::Actor(entity) => Ok(ActionTargetSnapshot::Entity(entity.raw())),
        ActionTarget::Cell { level, position } => Ok(ActionTargetSnapshot::Cell {
            level: level.0,
            x: position.x,
            y: position.y,
        }),
    }
}

fn action_target_from_snapshot(target: &ActionTargetSnapshot) -> SnapshotResult<ActionTarget> {
    match target {
        ActionTargetSnapshot::SelfTarget => Ok(ActionTarget::SelfTarget),
        ActionTargetSnapshot::Entity(id) => Ok(ActionTarget::Actor(
            sim_core::ActorId::new(*id).ok_or_else(|| format!("invalid actor id {}", id))?,
        )),
        ActionTargetSnapshot::Cell { level, x, y } => Ok(ActionTarget::Cell {
            level: LevelId(*level),
            position: IVec2::new(*x, *y),
        }),
    }
}

fn action_kind_to_snapshot(kind: &ActionKind) -> SnapshotResult<ActionKindSnapshot> {
    Ok(match kind {
        ActionKind::Wait => ActionKindSnapshot::Wait,
        ActionKind::Move { delta } => ActionKindSnapshot::Move {
            dx: delta.x,
            dy: delta.y,
        },
        ActionKind::Melee { target } => ActionKindSnapshot::Melee {
            target: target.raw(),
        },
        ActionKind::PickUp { item } => ActionKindSnapshot::PickUp { item: item.raw() },
        ActionKind::Drop { item } => ActionKindSnapshot::Drop { item: item.raw() },
        ActionKind::UseItem { item, target } => ActionKindSnapshot::UseItem {
            item: item.raw(),
            target: action_target_to_snapshot(target)?,
        },
        ActionKind::Descend => ActionKindSnapshot::Descend,
        ActionKind::Ascend => ActionKindSnapshot::Ascend,
    })
}

fn action_kind_from_snapshot(kind: &ActionKindSnapshot) -> SnapshotResult<ActionKind> {
    Ok(match kind {
        ActionKindSnapshot::Wait => ActionKind::Wait,
        ActionKindSnapshot::Move { dx, dy } => ActionKind::Move {
            delta: IVec2::new(*dx, *dy),
        },
        ActionKindSnapshot::Melee { target } => ActionKind::Melee {
            target: sim_core::ActorId::new(*target)
                .ok_or_else(|| format!("invalid actor id {}", target))?,
        },
        ActionKindSnapshot::PickUp { item } => ActionKind::PickUp {
            item: sim_core::ItemId::new(*item)
                .ok_or_else(|| format!("invalid item id {}", item))?,
        },
        ActionKindSnapshot::Drop { item } => ActionKind::Drop {
            item: sim_core::ItemId::new(*item)
                .ok_or_else(|| format!("invalid item id {}", item))?,
        },
        ActionKindSnapshot::UseItem { item, target } => ActionKind::UseItem {
            item: sim_core::ItemId::new(*item)
                .ok_or_else(|| format!("invalid item id {}", item))?,
            target: action_target_from_snapshot(target)?,
        },
        ActionKindSnapshot::Descend => ActionKind::Descend,
        ActionKindSnapshot::Ascend => ActionKind::Ascend,
    })
}

fn ai_goal_to_snapshot(goal: &AiGoal) -> SnapshotResult<AiGoalSnapshot> {
    Ok(match goal {
        AiGoal::Idle => AiGoalSnapshot::Idle,
        AiGoal::Wander => AiGoalSnapshot::Wander,
        AiGoal::Investigate(position) => AiGoalSnapshot::Investigate(position_to_saved(*position)),
        AiGoal::Chase(entity) => AiGoalSnapshot::Chase(entity.raw()),
        AiGoal::Flee(entity) => AiGoalSnapshot::Flee(entity.raw()),
    })
}

fn ai_goal_from_snapshot(goal: &AiGoalSnapshot) -> SnapshotResult<AiGoal> {
    Ok(match goal {
        AiGoalSnapshot::Idle => AiGoal::Idle,
        AiGoalSnapshot::Wander => AiGoal::Wander,
        AiGoalSnapshot::Investigate(position) => AiGoal::Investigate(saved_to_position(position)),
        AiGoalSnapshot::Chase(id) => AiGoal::Chase(
            sim_core::ActorId::new(*id).ok_or_else(|| format!("invalid actor id {}", id))?,
        ),
        AiGoalSnapshot::Flee(id) => AiGoal::Flee(
            sim_core::ActorId::new(*id).ok_or_else(|| format!("invalid actor id {}", id))?,
        ),
    })
}

fn effect_to_snapshot(effect: &Effect) -> SnapshotResult<EffectSnapshot> {
    Ok(match effect {
        Effect::Damage {
            source,
            target,
            amount,
            kind,
        } => EffectSnapshot::Damage {
            source: source.map(|actor| actor.raw()),
            target: target.raw(),
            amount: *amount,
            kind: *kind,
        },
        Effect::Heal { target, amount } => EffectSnapshot::Heal {
            target: target.raw(),
            amount: *amount,
        },
        Effect::Teleport {
            target,
            destination,
        } => EffectSnapshot::Teleport {
            target: target.raw(),
            destination: position_to_saved(*destination),
        },
        Effect::ApplyStatus { target, status } => EffectSnapshot::ApplyStatus {
            target: target.raw(),
            status: *status,
        },
    })
}

fn effect_from_snapshot(effect: &EffectSnapshot) -> SnapshotResult<Effect> {
    Ok(match effect {
        EffectSnapshot::Damage {
            source,
            target,
            amount,
            kind,
        } => Effect::Damage {
            source: match source {
                Some(id) => Some(
                    sim_core::ActorId::new(*id)
                        .ok_or_else(|| format!("invalid actor id {}", id))?,
                ),
                None => None,
            },
            target: sim_core::ActorId::new(*target)
                .ok_or_else(|| format!("invalid actor id {}", target))?,
            amount: *amount,
            kind: *kind,
        },
        EffectSnapshot::Heal { target, amount } => Effect::Heal {
            target: sim_core::ActorId::new(*target)
                .ok_or_else(|| format!("invalid actor id {}", target))?,
            amount: *amount,
        },
        EffectSnapshot::Teleport {
            target,
            destination,
        } => Effect::Teleport {
            target: sim_core::ActorId::new(*target)
                .ok_or_else(|| format!("invalid actor id {}", target))?,
            destination: saved_to_position(destination),
        },
        EffectSnapshot::ApplyStatus { target, status } => Effect::ApplyStatus {
            target: sim_core::ActorId::new(*target)
                .ok_or_else(|| format!("invalid actor id {}", target))?,
            status: *status,
        },
    })
}

fn build_level_snapshot(map: &LevelMap) -> LevelSnapshot {
    LevelSnapshot {
        id: map.id.0,
        width: map.width,
        height: map.height,
        tiles: map
            .tiles
            .iter()
            .map(|tile: &Tile| TileSnapshot {
                kind: tile.kind,
                explored: tile.explored,
                visible: tile.visible,
            })
            .collect(),
    }
}

fn build_entity_snapshot(
    entity: Entity,
    ids: &HashMap<Entity, u64>,
    entity_ref: &bevy_ecs::world::EntityRef<'_>,
) -> SnapshotResult<EntitySnapshot> {
    let id = pid_of(entity, ids)?;
    let prototype = entity_ref
        .get::<PrototypeId>()
        .ok_or_else(|| format!("entity {:?} missing prototype id", entity))?
        .0
        .clone();

    let position = entity_ref
        .get::<GridPosition>()
        .copied()
        .map(position_to_saved);
    let health = entity_ref
        .get::<Health>()
        .copied()
        .map(|health| SavedHealth {
            current: health.current,
            maximum: health.maximum,
        });
    let combat_stats = entity_ref
        .get::<CombatStats>()
        .copied()
        .map(|stats| SavedCombatStats {
            power: stats.power,
            defense: stats.defense,
        });
    let vision = entity_ref
        .get::<Vision>()
        .copied()
        .map(|vision| SavedVision {
            range: vision.range,
        });
    let action_speed = entity_ref
        .get::<ActionSpeed>()
        .copied()
        .map(|speed| SavedActionSpeed {
            ticks_per_action: speed.ticks_per_action,
        });
    let inventory = entity_ref
        .get::<Inventory>()
        .map(|inventory| {
            let mut items = Vec::with_capacity(inventory.items.len());
            for item in &inventory.items {
                items.push(item.raw());
            }
            Ok::<SavedInventory, String>(SavedInventory {
                capacity: inventory.capacity,
                items,
            })
        })
        .transpose()?;
    let carried_by = entity_ref
        .get::<CarriedBy>()
        .map(|carried| Ok::<u64, String>(carried.0.raw()))
        .transpose()?;
    let ai_goal = entity_ref
        .get::<AiGoal>()
        .map(|goal| ai_goal_to_snapshot(goal))
        .transpose()?;
    let last_known_player_position = entity_ref
        .get::<crate::actor::components::LastKnownPlayerPosition>()
        .copied()
        .map(|position| SavedLastKnownPlayerPosition {
            level: position.level.0,
            x: position.cell.x,
            y: position.cell.y,
            observed_at: position.observed_at,
        });
    let active_statuses = entity_ref
        .get::<ActiveStatuses>()
        .map(|statuses| statuses.0.clone())
        .unwrap_or_default();

    Ok(EntitySnapshot {
        id,
        prototype,
        actor: entity_ref.contains::<Actor>(),
        player: entity_ref.contains::<Player>(),
        monster: entity_ref.contains::<Monster>(),
        item: entity_ref.contains::<Item>(),
        blocks_movement: entity_ref.contains::<BlocksMovement>(),
        blocks_sight: entity_ref.contains::<BlocksSight>(),
        hostile_to_player: entity_ref.contains::<HostileToPlayer>(),
        position,
        health,
        combat_stats,
        vision,
        action_speed,
        inventory,
        carried_by,
        ai_goal,
        last_known_player_position,
        active_statuses,
    })
}

fn validate_snapshot_shape(snapshot: &GameSnapshot) -> SnapshotResult<()> {
    if snapshot.root_seed != snapshot.rng.seed {
        return Err("snapshot root seed must match rng seed".to_string());
    }

    let mut ids = HashSet::new();
    for entity in &snapshot.entities {
        if entity.id == 0 {
            return Err("persistent id 0 is invalid".to_string());
        }
        if !ids.insert(entity.id) {
            return Err(format!("duplicate persistent id {}", entity.id));
        }
    }

    let actor_ids: HashSet<u64> = snapshot
        .entities
        .iter()
        .filter(|entity| entity.actor)
        .map(|entity| entity.id)
        .collect();
    let item_ids: HashSet<u64> = snapshot
        .entities
        .iter()
        .filter(|entity| entity.item)
        .map(|entity| entity.id)
        .collect();
    let level_ids: HashSet<u32> = snapshot.levels.iter().map(|level| level.id).collect();
    let mut item_owners: HashMap<u64, u64> = HashMap::new();

    if level_ids.len() != snapshot.levels.len() {
        return Err("duplicate level ids are not allowed".to_string());
    }

    if snapshot.levels.is_empty() {
        return Err("snapshot does not contain any levels".to_string());
    }

    if snapshot
        .simulation_driver
        .driver
        .pending_target_minute()
        .is_some_and(|target| target < snapshot.simulation_driver.driver.clock.minute)
    {
        return Err("simulation driver pending target cannot precede its clock".to_string());
    }
    if snapshot
        .simulation_driver
        .request
        .target_minute()
        .is_some_and(|target| target < snapshot.simulation_driver.driver.clock.minute)
    {
        return Err("simulation driver request target cannot precede its clock".to_string());
    }
    if let Some(pending_target) = snapshot.simulation_driver.driver.pending_target_minute() {
        match snapshot.simulation_driver.request.target_minute() {
            Some(final_target) if pending_target <= final_target => {}
            Some(_) => {
                return Err(
                    "simulation driver pending target cannot exceed its active request".to_string(),
                );
            }
            None => {
                return Err(
                    "simulation driver pending target requires an active request".to_string(),
                );
            }
        }
    }
    if snapshot
        .simulation_driver
        .driver
        .backlog
        .entries()
        .iter()
        .any(|work| work.cadence == Cadence::Tactical)
    {
        return Err("simulation driver backlog must not contain tactical work".to_string());
    }
    if snapshot
        .simulation_driver
        .driver
        .backlog
        .entries()
        .iter()
        .any(|work| work.domain_event_cost != 1)
    {
        return Err(
            "simulation driver backlog work must emit exactly one domain event".to_string(),
        );
    }

    let max_entity_id = ids.iter().copied().max().unwrap_or(0);
    if snapshot.persistent_ids.next_available <= max_entity_id {
        return Err(format!(
            "persistent id allocator next_available {} must exceed max entity id {}",
            snapshot.persistent_ids.next_available, max_entity_id
        ));
    }

    let max_sequence = snapshot
        .timeline
        .iter()
        .map(|entry| entry.sequence)
        .max()
        .unwrap_or(0);
    if !snapshot.timeline.is_empty() && snapshot.next_sequence <= max_sequence {
        return Err(format!(
            "turn clock next_sequence {} must exceed max timeline sequence {}",
            snapshot.next_sequence, max_sequence
        ));
    }

    for level in &snapshot.levels {
        if level.width == 0 || level.height == 0 {
            return Err(format!("level {} has zero dimensions", level.id));
        }
        let expected_tiles = u64::from(level.width)
            .checked_mul(u64::from(level.height))
            .ok_or_else(|| format!("level {} dimensions overflow", level.id))?;
        if expected_tiles != level.tiles.len() as u64 {
            return Err(format!(
                "level {} tile count does not match dimensions",
                level.id
            ));
        }
    }

    if !level_ids.contains(&snapshot.current_level) {
        return Err(format!(
            "current level {} is not present in the snapshot",
            snapshot.current_level
        ));
    }

    for entity in &snapshot.entities {
        if entity.inventory.is_some() && !entity.actor {
            return Err(format!(
                "entity {} has inventory but is not marked as an actor",
                entity.id
            ));
        }

        if let Some(position) = &entity.position {
            if !level_ids.contains(&position.level) {
                return Err(format!(
                    "entity {} references unknown level {}",
                    entity.id, position.level
                ));
            }
            let level = snapshot
                .levels
                .iter()
                .find(|level| level.id == position.level)
                .ok_or_else(|| {
                    format!(
                        "entity {} references unknown level {}",
                        entity.id, position.level
                    )
                })?;
            position_within_level(position, level, entity.id, "position")?;
        }

        if let Some(inventory) = &entity.inventory {
            if inventory.items.len() > inventory.capacity {
                return Err(format!(
                    "entity {} inventory exceeds its capacity",
                    entity.id
                ));
            }
            for item in &inventory.items {
                if !item_ids.contains(item) {
                    return Err(format!(
                        "entity {} inventory references missing item {}",
                        entity.id, item
                    ));
                }
                let item_entity = snapshot
                    .entities
                    .iter()
                    .find(|candidate| candidate.id == *item)
                    .ok_or_else(|| {
                        format!(
                            "entity {} inventory references missing item {}",
                            entity.id, item
                        )
                    })?;
                if !item_entity.item {
                    return Err(format!(
                        "entity {} inventory references non-item {}",
                        entity.id, item
                    ));
                }
                match item_entity.carried_by {
                    Some(owner) if owner == entity.id => {}
                    Some(owner) => {
                        return Err(format!(
                            "item {} carried_by {} disagrees with inventory owner {}",
                            item, owner, entity.id
                        ));
                    }
                    None => {
                        return Err(format!(
                            "item {} listed in inventory {} is missing carried_by",
                            item, entity.id
                        ));
                    }
                }
                if item_owners.insert(*item, entity.id).is_some() {
                    return Err(format!("item {} appears in multiple inventories", item));
                }
            }
        }

        if let Some(owner) = entity.carried_by
            && !actor_ids.contains(&owner)
        {
            return Err(format!(
                "entity {} carried_by references missing entity {}",
                entity.id, owner
            ));
        }

        if let Some(goal) = &entity.ai_goal {
            match goal {
                AiGoalSnapshot::Chase(id) | AiGoalSnapshot::Flee(id) => {
                    if !actor_ids.contains(id) {
                        return Err(format!(
                            "entity {} ai goal references missing entity {}",
                            entity.id, id
                        ));
                    }
                }
                AiGoalSnapshot::Investigate(position) => {
                    if !level_ids.contains(&position.level) {
                        return Err(format!(
                            "entity {} ai goal references missing level {}",
                            entity.id, position.level
                        ));
                    }
                    let level = snapshot
                        .levels
                        .iter()
                        .find(|level| level.id == position.level)
                        .ok_or_else(|| {
                            format!(
                                "entity {} ai goal references missing level {}",
                                entity.id, position.level
                            )
                        })?;
                    position_within_level(position, level, entity.id, "ai goal")?;
                }
                AiGoalSnapshot::Idle | AiGoalSnapshot::Wander => {}
            }
        }

        if let Some(position) = &entity.last_known_player_position {
            if !level_ids.contains(&position.level) {
                return Err(format!(
                    "entity {} last-known position references missing level {}",
                    entity.id, position.level
                ));
            }
            let level = snapshot
                .levels
                .iter()
                .find(|level| level.id == position.level)
                .ok_or_else(|| {
                    format!(
                        "entity {} last-known position references missing level {}",
                        entity.id, position.level
                    )
                })?;
            last_known_position_within_level(position, level, entity.id)?;
        }

        for status in &entity.active_statuses {
            match status {
                StatusEffect::Poisoned { remaining } | StatusEffect::Stunned { remaining } => {
                    if *remaining == 0 {
                        return Err(format!(
                            "entity {} contains a zero-duration status",
                            entity.id
                        ));
                    }
                }
            }
        }
    }

    for entity in &snapshot.entities {
        if let Some(owner) = entity.carried_by {
            if !entity.item {
                return Err(format!(
                    "entity {} has carried_by but is not marked as an item",
                    entity.id
                ));
            }
            match item_owners.get(&entity.id) {
                Some(listed_owner) if *listed_owner == owner => {}
                Some(listed_owner) => {
                    return Err(format!(
                        "item {} carried_by {} disagrees with inventory owner {}",
                        entity.id, owner, listed_owner
                    ));
                }
                None => {
                    return Err(format!(
                        "item {} carried_by {} is not listed in that inventory",
                        entity.id, owner
                    ));
                }
            }
        }
    }

    for work in snapshot.simulation_driver.driver.backlog.entries() {
        if work.id.raw() == 0 {
            return Err("simulation driver backlog contains invalid domain work id 0".to_string());
        }
        if work.cadence == Cadence::Tactical {
            return Err("simulation driver backlog must not contain tactical work".to_string());
        }
    }

    for event in &snapshot.simulation_driver.event_log {
        if event.id.raw() == 0 {
            return Err(
                "simulation driver event log contains invalid domain work id 0".to_string(),
            );
        }
        if event.cadence == Cadence::Tactical {
            return Err("simulation driver event log must not contain tactical work".to_string());
        }
    }

    for action in &snapshot.pending_actions {
        if !actor_ids.contains(&action.actor) {
            return Err(format!(
                "pending action references missing actor {}",
                action.actor
            ));
        }
        validate_action_kind_references(&action.kind, &actor_ids, &item_ids, &level_ids)?;
    }

    for effect in &snapshot.pending_effects {
        validate_effect_references(effect, &actor_ids, &level_ids)?;
    }

    for entry in &snapshot.timeline {
        if !actor_ids.contains(&entry.actor) {
            return Err(format!("timeline references missing actor {}", entry.actor));
        }
    }

    if let Some(current_actor) = snapshot.current_actor
        && !actor_ids.contains(&current_actor)
    {
        return Err(format!(
            "current actor references missing entity {}",
            current_actor
        ));
    }

    Ok(())
}

fn validate_action_kind_references(
    kind: &ActionKindSnapshot,
    actor_ids: &HashSet<u64>,
    item_ids: &HashSet<u64>,
    level_ids: &HashSet<u32>,
) -> SnapshotResult<()> {
    match kind {
        ActionKindSnapshot::Wait
        | ActionKindSnapshot::Move { .. }
        | ActionKindSnapshot::Descend
        | ActionKindSnapshot::Ascend => Ok(()),
        ActionKindSnapshot::Melee { target } => {
            if actor_ids.contains(target) {
                Ok(())
            } else {
                Err(format!("action references missing entity {}", target))
            }
        }
        ActionKindSnapshot::PickUp { item } | ActionKindSnapshot::Drop { item } => {
            if item_ids.contains(item) {
                Ok(())
            } else {
                Err(format!("action references missing item {}", item))
            }
        }
        ActionKindSnapshot::UseItem { item, target } => {
            if !item_ids.contains(item) {
                return Err(format!("action references missing item {}", item));
            }
            match target {
                ActionTargetSnapshot::SelfTarget => Ok(()),
                ActionTargetSnapshot::Entity(id) => {
                    if actor_ids.contains(id) {
                        Ok(())
                    } else {
                        Err(format!("action references missing entity {}", id))
                    }
                }
                ActionTargetSnapshot::Cell { level, .. } => {
                    if level_ids.contains(level) {
                        Ok(())
                    } else {
                        Err(format!("action references missing level {}", level))
                    }
                }
            }
        }
    }
}

fn validate_effect_references(
    effect: &EffectSnapshot,
    actor_ids: &HashSet<u64>,
    level_ids: &HashSet<u32>,
) -> SnapshotResult<()> {
    match effect {
        EffectSnapshot::Damage { source, target, .. } => {
            if !actor_ids.contains(target) {
                return Err(format!("effect references missing target {}", target));
            }
            if let Some(source) = source
                && !actor_ids.contains(source)
            {
                return Err(format!("effect references missing source {}", source));
            }
            Ok(())
        }
        EffectSnapshot::Heal { target, .. } | EffectSnapshot::ApplyStatus { target, .. } => {
            if actor_ids.contains(target) {
                Ok(())
            } else {
                Err(format!("effect references missing target {}", target))
            }
        }
        EffectSnapshot::Teleport {
            target,
            destination,
        } => {
            if !actor_ids.contains(target) {
                return Err(format!("effect references missing target {}", target));
            }
            if level_ids.contains(&destination.level) {
                Ok(())
            } else {
                Err(format!(
                    "effect references missing destination level {}",
                    destination.level
                ))
            }
        }
    }
}

fn validate_restore_support(snapshot: &GameSnapshot) -> SnapshotResult<()> {
    if snapshot.levels.len() != 1 {
        return Err("restore currently supports exactly one level".to_string());
    }

    Ok(())
}

fn position_within_level(
    position: &SavedPosition,
    level: &LevelSnapshot,
    entity_id: u64,
    context: &str,
) -> SnapshotResult<()> {
    if position.x < 0 || position.y < 0 {
        return Err(format!(
            "entity {} {} references negative coordinates",
            entity_id, context
        ));
    }

    let x = position.x as u32;
    let y = position.y as u32;
    if x >= level.width || y >= level.height {
        return Err(format!(
            "entity {} {} references out-of-bounds position",
            entity_id, context
        ));
    }

    Ok(())
}

fn last_known_position_within_level(
    position: &SavedLastKnownPlayerPosition,
    level: &LevelSnapshot,
    entity_id: u64,
) -> SnapshotResult<()> {
    if position.x < 0 || position.y < 0 {
        return Err(format!(
            "entity {} last-known position references negative coordinates",
            entity_id
        ));
    }

    let x = position.x as u32;
    let y = position.y as u32;
    if x >= level.width || y >= level.height {
        return Err(format!(
            "entity {} last-known position references out-of-bounds position",
            entity_id
        ));
    }

    Ok(())
}

pub fn snapshot_world(world: &World) -> SnapshotResult<GameSnapshot> {
    let map = world
        .get_resource::<LevelMap>()
        .ok_or_else(|| "missing level map resource".to_string())?;
    let rng = world
        .get_resource::<RandomStreams>()
        .cloned()
        .ok_or_else(|| "missing random streams resource".to_string())?;
    let allocator = world
        .get_resource::<PersistentIdAllocator>()
        .map(|allocator| PersistentIdAllocatorSnapshot {
            next_available: allocator.next_available(),
        })
        .ok_or_else(|| "missing persistent id allocator resource".to_string())?;
    let clock = world
        .get_resource::<TurnClock>()
        .ok_or_else(|| "missing turn clock resource".to_string())?;
    let current_actor = world
        .get_resource::<CurrentActor>()
        .ok_or_else(|| "missing current actor resource".to_string())?
        .0;
    let action_queue = world
        .get_resource::<ActionQueue>()
        .ok_or_else(|| "missing action queue resource".to_string())?;
    let effect_queue = world
        .get_resource::<EffectQueue>()
        .ok_or_else(|| "missing effect queue resource".to_string())?;
    let decision = world
        .get_resource::<crate::action::resolver::ActionDecision>()
        .ok_or_else(|| "missing action decision resource".to_string())?;
    let simulation_status = world
        .get_resource::<SimulationStatus>()
        .copied()
        .ok_or_else(|| "missing simulation status resource".to_string())?;

    let mut ids = HashMap::new();
    let mut stable_actors = HashSet::new();
    let mut stable_items = HashSet::new();
    let stable_index = world.get_resource::<StableEntityIndex>();
    for entity in world.iter_entities() {
        let durable = entity.contains::<Actor>() || entity.contains::<Item>();
        let persistent_id = entity.get::<PersistentId>();

        if durable && persistent_id.is_none() {
            return Err(format!(
                "durable entity {:?} is missing a persistent id",
                entity.id()
            ));
        }

        if entity.contains::<Actor>() && entity.get::<StableActorId>().is_none() {
            return Err(format!(
                "durable actor {:?} is missing a stable actor id",
                entity.id()
            ));
        }
        if entity.contains::<Item>() && entity.get::<StableItemId>().is_none() {
            return Err(format!(
                "durable item {:?} is missing a stable item id",
                entity.id()
            ));
        }

        if let Some(id) = persistent_id {
            if id.0 == 0 {
                return Err(format!(
                    "persistent id 0 is invalid for entity {:?}",
                    entity.id()
                ));
            }
            if durable && entity.get::<PrototypeId>().is_none() {
                return Err(format!(
                    "durable entity {:?} is missing a prototype id",
                    entity.id()
                ));
            }
            if let Some(stable) = entity.get::<StableActorId>() {
                if !stable_actors.insert(stable.0) {
                    return Err(format!("duplicate stable actor id {}", stable.0.raw()));
                }
                if stable.0.raw() != id.0 {
                    return Err(format!(
                        "stable actor id {} disagrees with persistent id {} on entity {:?}",
                        stable.0.raw(),
                        id.0,
                        entity.id()
                    ));
                }
                if let Some(index) = stable_index {
                    let indexed = index.actor(stable.0).ok_or_else(|| {
                        format!(
                            "stable actor id {} missing from stable index",
                            stable.0.raw()
                        )
                    })?;
                    if indexed != entity.id() {
                        return Err(format!(
                            "stable actor id {} points to {:?} but entity {:?} carries it",
                            stable.0.raw(),
                            indexed,
                            entity.id()
                        ));
                    }
                }
            }
            if let Some(stable) = entity.get::<StableItemId>() {
                if !stable_items.insert(stable.0) {
                    return Err(format!("duplicate stable item id {}", stable.0.raw()));
                }
                if stable.0.raw() != id.0 {
                    return Err(format!(
                        "stable item id {} disagrees with persistent id {} on entity {:?}",
                        stable.0.raw(),
                        id.0,
                        entity.id()
                    ));
                }
                if let Some(index) = stable_index {
                    let indexed = index.item(stable.0).ok_or_else(|| {
                        format!(
                            "stable item id {} missing from stable index",
                            stable.0.raw()
                        )
                    })?;
                    if indexed != entity.id() {
                        return Err(format!(
                            "stable item id {} points to {:?} but entity {:?} carries it",
                            stable.0.raw(),
                            indexed,
                            entity.id()
                        ));
                    }
                }
            }
            if ids.insert(entity.id(), id.0).is_some() {
                return Err(format!("duplicate persistent id {}", id.0));
            }
        }
    }

    let max_entity_id = ids.values().copied().max().unwrap_or(0);
    if allocator.next_available <= max_entity_id {
        return Err(format!(
            "persistent id allocator next_available {} must exceed max entity id {}",
            allocator.next_available, max_entity_id
        ));
    }

    let mut entities = Vec::new();
    for entity in world.iter_entities() {
        if !entity.contains::<PersistentId>() {
            continue;
        }
        entities.push(build_entity_snapshot(entity.id(), &ids, &entity)?);
    }
    entities.sort_by_key(|entity| entity.id);

    let current_actor = current_actor.map(|actor| actor.raw());

    let mut timeline = Vec::new();
    for entry in clock.timeline.iter() {
        timeline.push(ScheduledActorSnapshot {
            next_tick: entry.0.next_tick,
            sequence: entry.0.sequence,
            actor: entry.0.actor.raw(),
        });
    }
    timeline.sort_by_key(|entry| (entry.next_tick, entry.sequence, entry.actor));

    let mut pending_actions = Vec::new();
    for action in &action_queue.actions {
        pending_actions.push(ActionSnapshot {
            actor: action.actor.raw(),
            kind: action_kind_to_snapshot(&action.kind)?,
        });
    }

    let mut pending_effects = Vec::new();
    for effect in &effect_queue.0 {
        pending_effects.push(effect_to_snapshot(effect)?);
    }

    let mut simulation_driver = world
        .get_resource::<SimulationDriverState>()
        .cloned()
        .ok_or_else(|| "missing simulation driver resource".to_string())?;
    simulation_driver
        .driver
        .backlog
        .retain_where(|work| work.cadence != Cadence::Tactical);
    simulation_driver.driver.budget = Default::default();
    simulation_driver.driver.progress = Default::default();

    if !matches!(
        *decision,
        crate::action::resolver::ActionDecision::Idle
            | crate::action::resolver::ActionDecision::WaitingForPlayer
    ) {
        return Err("snapshot requires an idle or waiting action decision".to_string());
    }
    let simulation_status = SimulationStatusSnapshot::from(simulation_status);

    let levels = vec![build_level_snapshot(map)];
    validate_snapshot_shape(&GameSnapshot {
        version: super::migration::CURRENT_SAVE_VERSION,
        root_seed: rng.seed,
        current_level: levels[0].id,
        current_tick: clock.current_tick,
        next_sequence: clock.next_sequence,
        current_actor,
        simulation_status: simulation_status.clone(),
        persistent_ids: allocator.clone(),
        levels: levels.clone(),
        entities: entities.clone(),
        timeline: timeline.clone(),
        pending_actions: pending_actions.clone(),
        pending_effects: pending_effects.clone(),
        simulation_driver: simulation_driver.clone(),
        rng: rng.snapshot(),
    })?;

    Ok(GameSnapshot {
        version: super::migration::CURRENT_SAVE_VERSION,
        root_seed: rng.seed,
        current_level: levels[0].id,
        current_tick: clock.current_tick,
        next_sequence: clock.next_sequence,
        current_actor,
        simulation_status,
        persistent_ids: allocator,
        levels,
        entities,
        timeline,
        pending_actions,
        pending_effects,
        simulation_driver,
        rng: rng.snapshot(),
    })
}

fn clear_simulation_state(world: &mut World) {
    let entities: Vec<Entity> = world
        .iter_entities()
        .filter(|entity| entity.contains::<PersistentId>())
        .map(|entity| entity.id())
        .collect();
    for entity in entities {
        let _ = world.despawn(entity);
    }

    world.remove_resource::<LevelMap>();
    world.remove_resource::<SpatialIndex>();
    world.remove_resource::<ActionQueue>();
    world.remove_resource::<EffectQueue>();
    world.remove_resource::<TurnClock>();
    world.remove_resource::<CurrentActor>();
    world.remove_resource::<StableEntityIndex>();
    world.remove_resource::<SimulationDriverState>();
    world.remove_resource::<SimulationStatus>();
    world.remove_resource::<RandomStreams>();
    world.remove_resource::<PersistentIdAllocator>();
}

fn rebuild_spatial_and_fov(world: &mut World) -> SnapshotResult<()> {
    let mut spatial = SpatialIndex::default();
    let mut query = world.query::<(
        Entity,
        &GridPosition,
        Option<&BlocksMovement>,
        Option<&BlocksSight>,
        Option<&PersistentId>,
        Option<&StableActorId>,
        Option<&StableItemId>,
    )>();
    for (
        entity,
        position,
        blocks_movement,
        blocks_sight,
        persistent_id,
        stable_actor,
        stable_item,
    ) in query.iter(world)
    {
        spatial.insert_occupant(
            entity,
            *position,
            stable_actor,
            stable_item,
            persistent_id,
            blocks_movement.is_some(),
            blocks_sight.is_some(),
        );
    }
    world.insert_resource(spatial);

    let spatial = world
        .get_resource::<SpatialIndex>()
        .cloned()
        .ok_or_else(|| "missing spatial index after rebuild".to_string())?;
    let mut player_query = world.query_filtered::<(&GridPosition, &Vision), With<Player>>();
    let player_position = player_query
        .iter(world)
        .next()
        .map(|(position, vision)| (*position, *vision));

    if let Some((player_position, vision)) = player_position
        && let Some(mut map) = world.get_resource_mut::<LevelMap>()
    {
        recalculate_fov_for_player(&mut map, &spatial, player_position, vision.range);
    }

    Ok(())
}

pub fn restore_world(world: &mut World, snapshot: &GameSnapshot) -> SnapshotResult<()> {
    validate_snapshot_shape(snapshot)?;
    validate_restore_support(snapshot)?;

    let level = &snapshot.levels[0];
    let mut map = LevelMap::with_id(LevelId(level.id), level.width, level.height, TileKind::Wall);
    if level.tiles.len() != map.tiles.len() {
        return Err("snapshot tile count did not match level dimensions".to_string());
    }
    for (tile, saved) in map.tiles.iter_mut().zip(&level.tiles) {
        tile.kind = saved.kind;
        tile.explored = saved.explored;
        tile.visible = saved.visible;
    }

    let mut allocator = PersistentIdAllocator::default();
    allocator.set_next_available(snapshot.persistent_ids.next_available);

    clear_simulation_state(world);

    world.insert_resource(map);
    world.insert_resource(RandomStreams::from_snapshot(&snapshot.rng));
    world.insert_resource(allocator);
    world.insert_resource(ActionQueue::default());
    world.insert_resource(EffectQueue::default());
    world.insert_resource(crate::action::resolver::ActionDecision::default());
    world.insert_resource(crate::action::resolver::ActionOutcomeLog::default());
    world.insert_resource(TurnClock {
        current_tick: snapshot.current_tick,
        next_sequence: snapshot.next_sequence,
        timeline: std::collections::BinaryHeap::new(),
    });
    world.insert_resource(CurrentActor::default());
    world.insert_resource(StableEntityIndex::default());
    world.insert_resource(SimulationDriverState::default());
    world.insert_resource(SimulationStatus::from(snapshot.simulation_status.clone()));

    let mut entity_map = HashMap::new();
    for entity in &snapshot.entities {
        let mut spawned = world.spawn_empty();
        spawned.insert(PersistentId(entity.id));
        spawned.insert(PrototypeId(entity.prototype.clone()));
        if entity.actor {
            spawned.insert(Actor);
        }
        if entity.player {
            spawned.insert(Player);
        }
        if entity.monster {
            spawned.insert(Monster);
        }
        if entity.item {
            spawned.insert(Item);
        }
        if entity.blocks_movement {
            spawned.insert(BlocksMovement);
        }
        if entity.blocks_sight {
            spawned.insert(BlocksSight);
        }
        if entity.hostile_to_player {
            spawned.insert(HostileToPlayer);
        }
        if let Some(position) = &entity.position {
            spawned.insert(saved_to_position(position));
        }
        if let Some(health) = entity.health {
            spawned.insert(Health {
                current: health.current,
                maximum: health.maximum,
            });
        }
        if let Some(stats) = entity.combat_stats {
            spawned.insert(CombatStats {
                power: stats.power,
                defense: stats.defense,
            });
        }
        if let Some(vision) = entity.vision {
            spawned.insert(Vision {
                range: vision.range,
            });
        }
        if let Some(speed) = entity.action_speed {
            spawned.insert(ActionSpeed {
                ticks_per_action: speed.ticks_per_action,
            });
        }
        if entity.actor {
            let actor_id = sim_core::ActorId::new(entity.id)
                .ok_or_else(|| format!("invalid actor id {}", entity.id))?;
            spawned.insert(StableActorId(actor_id));
        }
        if entity.item {
            let item_id = sim_core::ItemId::new(entity.id)
                .ok_or_else(|| format!("invalid item id {}", entity.id))?;
            spawned.insert(StableItemId(item_id));
        }
        if entity.actor {
            spawned.insert(ActiveStatuses::default());
        }
        let entity_id = spawned.id();
        entity_map.insert(entity.id, entity_id);
    }

    for entity in &snapshot.entities {
        let entity_id = *entity_map
            .get(&entity.id)
            .ok_or_else(|| format!("missing restored entity {}", entity.id))?;
        let mut entity_mut = world.entity_mut(entity_id);

        if let Some(owner) = entity.carried_by {
            entity_mut.insert(CarriedBy(
                sim_core::ActorId::new(owner)
                    .ok_or_else(|| format!("invalid actor id {}", owner))?,
            ));
        }

        if let Some(inventory) = &entity.inventory {
            let mut items = Vec::with_capacity(inventory.items.len());
            for item in &inventory.items {
                items.push(
                    sim_core::ItemId::new(*item)
                        .ok_or_else(|| format!("invalid item id {}", item))?,
                );
            }
            entity_mut.insert(Inventory {
                capacity: inventory.capacity,
                items,
            });
        }

        if let Some(goal) = &entity.ai_goal {
            entity_mut.insert(ai_goal_from_snapshot(goal)?);
        }

        if let Some(position) = &entity.last_known_player_position {
            entity_mut.insert(crate::actor::components::LastKnownPlayerPosition {
                level: LevelId(position.level),
                cell: IVec2::new(position.x, position.y),
                observed_at: position.observed_at,
            });
        }
        if !entity.active_statuses.is_empty() {
            entity_mut.insert(ActiveStatuses(entity.active_statuses.clone()));
        }
    }

    if let Some(mut index) = world.get_resource_mut::<StableEntityIndex>() {
        index.clear();
        for entity in &snapshot.entities {
            let entity_id = *entity_map
                .get(&entity.id)
                .ok_or_else(|| format!("missing restored entity {}", entity.id))?;
            if entity.actor {
                let actor_id = sim_core::ActorId::new(entity.id)
                    .ok_or_else(|| format!("invalid actor id {}", entity.id))?;
                index.insert_actor(actor_id, entity_id);
            }
            if entity.item {
                let item_id = sim_core::ItemId::new(entity.id)
                    .ok_or_else(|| format!("invalid item id {}", entity.id))?;
                index.insert_item(item_id, entity_id);
            }
        }
    }

    if let Some(current_actor) = snapshot.current_actor {
        world.insert_resource(sim_core::schedule::CurrentActor(Some(
            sim_core::ActorId::new(current_actor)
                .ok_or_else(|| format!("invalid actor id {}", current_actor))?,
        )));
    }

    if let Some(mut clock) = world.get_resource_mut::<TurnClock>() {
        for entry in &snapshot.timeline {
            clock.timeline.push(std::cmp::Reverse(ScheduledActor {
                next_tick: entry.next_tick,
                sequence: entry.sequence,
                actor: sim_core::ActorId::new(entry.actor)
                    .ok_or_else(|| format!("invalid actor id {}", entry.actor))?,
            }));
        }
    }

    if let Some(mut driver) = world.get_resource_mut::<SimulationDriverState>() {
        let mut restored_driver = snapshot.simulation_driver.clone();
        restored_driver.driver.budget = Default::default();
        restored_driver.driver.progress = Default::default();
        restored_driver
            .driver
            .backlog
            .retain_where(|work| work.cadence != Cadence::Tactical);
        *driver = restored_driver;
    }

    if let Some(mut queue) = world.get_resource_mut::<ActionQueue>() {
        for action in &snapshot.pending_actions {
            queue.actions.push_back(Action {
                actor: sim_core::ActorId::new(action.actor)
                    .ok_or_else(|| format!("invalid actor id {}", action.actor))?,
                kind: action_kind_from_snapshot(&action.kind)?,
            });
        }
    }

    if let Some(mut effects) = world.get_resource_mut::<EffectQueue>() {
        for effect in &snapshot.pending_effects {
            effects.0.push_back(effect_from_snapshot(effect)?);
        }
    }

    rebuild_spatial_and_fov(world)?;
    Ok(())
}

pub fn snapshot_digest(snapshot: &GameSnapshot) -> SnapshotResult<String> {
    let serialized = ron::ser::to_string(snapshot).map_err(|err| err.to_string())?;
    Ok(blake3::hash(serialized.as_bytes()).to_hex().to_string())
}

pub fn snapshot_bytes(snapshot: &GameSnapshot) -> SnapshotResult<Vec<u8>> {
    ron::ser::to_string(snapshot)
        .map(|text| text.into_bytes())
        .map_err(|err| err.to_string())
}

pub fn digest_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

pub fn snapshot_from_bytes(bytes: &[u8]) -> SnapshotResult<SnapshotFile> {
    match ron::de::from_bytes::<GameSnapshot>(bytes) {
        Ok(snapshot) => Ok(SnapshotFile::Current(snapshot)),
        Err(current_err) => match ron::de::from_bytes::<LegacyGameSnapshotV2>(bytes) {
            Ok(snapshot) => Ok(SnapshotFile::V2(snapshot)),
            Err(v2_err) => match ron::de::from_bytes::<LegacyGameSnapshotV1>(bytes) {
                Ok(snapshot) => Ok(SnapshotFile::V1(snapshot)),
                Err(v1_err) => Err(format!(
                    "failed to deserialize current snapshot: {}; v2 snapshot: {}; legacy snapshot: {}",
                    current_err, v2_err, v1_err
                )),
            },
        },
    }
}

pub fn snapshot_to_bytes(snapshot: &GameSnapshot) -> SnapshotResult<Vec<u8>> {
    snapshot_bytes(snapshot)
}

pub fn snapshot_from_text(text: &str) -> SnapshotResult<SnapshotFile> {
    match ron::from_str::<GameSnapshot>(text) {
        Ok(snapshot) => Ok(SnapshotFile::Current(snapshot)),
        Err(current_err) => match ron::from_str::<LegacyGameSnapshotV2>(text) {
            Ok(snapshot) => Ok(SnapshotFile::V2(snapshot)),
            Err(v2_err) => match ron::from_str::<LegacyGameSnapshotV1>(text) {
                Ok(snapshot) => Ok(SnapshotFile::V1(snapshot)),
                Err(v1_err) => Err(format!(
                    "failed to deserialize current snapshot: {}; v2 snapshot: {}; legacy snapshot: {}",
                    current_err, v2_err, v1_err
                )),
            },
        },
    }
}

pub fn snapshot_to_text(snapshot: &GameSnapshot) -> SnapshotResult<String> {
    ron::ser::to_string(snapshot).map_err(|err| err.to_string())
}

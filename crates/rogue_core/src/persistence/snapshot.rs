use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use serde::{Deserialize, Serialize};

use crate::action::intent::{Action, ActionKind, ActionTarget};
use crate::action::queue::ActionQueue;
use crate::actor::combat::{DamageKind, StatusEffect};
use crate::actor::components::{
    ActionSpeed, ActiveStatuses, Actor, AiGoal, BlocksMovement, BlocksSight, CombatStats, Health,
    HostileToPlayer, Monster, PersistentId, PersistentIdAllocator, Player, PrototypeId, Vision,
};
use crate::item::components::{CarriedBy, Inventory, Item};
use crate::item::effects::{Effect, EffectQueue};
use crate::simulation::SimulationStatus;
use crate::time::clock::{CurrentActor, ScheduledActor, TurnClock};
use crate::world::fov::recalculate_fov_for_player;
use crate::world::map::{GridPosition, LevelId, LevelMap};
use crate::world::spatial::SpatialIndex;
use crate::world::tile::{Tile, TileKind};

use super::migration::{LegacyGameSnapshotV1, SnapshotFile};
use super::rng::RandomStreams;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RandomSnapshot {
    pub seed: u64,
    pub generation_state: u64,
    pub combat_state: u64,
    pub loot_state: u64,
    pub ai_state: u64,
}

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

fn action_target_to_snapshot(
    target: &ActionTarget,
    ids: &HashMap<Entity, u64>,
) -> SnapshotResult<ActionTargetSnapshot> {
    match target {
        ActionTarget::SelfTarget => Ok(ActionTargetSnapshot::SelfTarget),
        ActionTarget::Entity(entity) => Ok(ActionTargetSnapshot::Entity(pid_of(*entity, ids)?)),
        ActionTarget::Cell { level, position } => Ok(ActionTargetSnapshot::Cell {
            level: level.0,
            x: position.x,
            y: position.y,
        }),
    }
}

fn action_target_from_snapshot(
    target: &ActionTargetSnapshot,
    entities: &HashMap<u64, Entity>,
) -> SnapshotResult<ActionTarget> {
    match target {
        ActionTargetSnapshot::SelfTarget => Ok(ActionTarget::SelfTarget),
        ActionTargetSnapshot::Entity(id) => {
            Ok(ActionTarget::Entity(*entities.get(id).ok_or_else(
                || format!("missing entity for persistent id {}", id),
            )?))
        }
        ActionTargetSnapshot::Cell { level, x, y } => Ok(ActionTarget::Cell {
            level: LevelId(*level),
            position: IVec2::new(*x, *y),
        }),
    }
}

fn action_kind_to_snapshot(
    kind: &ActionKind,
    ids: &HashMap<Entity, u64>,
) -> SnapshotResult<ActionKindSnapshot> {
    Ok(match kind {
        ActionKind::Wait => ActionKindSnapshot::Wait,
        ActionKind::Move { delta } => ActionKindSnapshot::Move {
            dx: delta.x,
            dy: delta.y,
        },
        ActionKind::Melee { target } => ActionKindSnapshot::Melee {
            target: pid_of(*target, ids)?,
        },
        ActionKind::PickUp { item } => ActionKindSnapshot::PickUp {
            item: pid_of(*item, ids)?,
        },
        ActionKind::Drop { item } => ActionKindSnapshot::Drop {
            item: pid_of(*item, ids)?,
        },
        ActionKind::UseItem { item, target } => ActionKindSnapshot::UseItem {
            item: pid_of(*item, ids)?,
            target: action_target_to_snapshot(target, ids)?,
        },
        ActionKind::Descend => ActionKindSnapshot::Descend,
        ActionKind::Ascend => ActionKindSnapshot::Ascend,
    })
}

fn action_kind_from_snapshot(
    kind: &ActionKindSnapshot,
    entities: &HashMap<u64, Entity>,
) -> SnapshotResult<ActionKind> {
    Ok(match kind {
        ActionKindSnapshot::Wait => ActionKind::Wait,
        ActionKindSnapshot::Move { dx, dy } => ActionKind::Move {
            delta: IVec2::new(*dx, *dy),
        },
        ActionKindSnapshot::Melee { target } => ActionKind::Melee {
            target: *entities
                .get(target)
                .ok_or_else(|| format!("missing entity for persistent id {}", target))?,
        },
        ActionKindSnapshot::PickUp { item } => ActionKind::PickUp {
            item: *entities
                .get(item)
                .ok_or_else(|| format!("missing entity for persistent id {}", item))?,
        },
        ActionKindSnapshot::Drop { item } => ActionKind::Drop {
            item: *entities
                .get(item)
                .ok_or_else(|| format!("missing entity for persistent id {}", item))?,
        },
        ActionKindSnapshot::UseItem { item, target } => ActionKind::UseItem {
            item: *entities
                .get(item)
                .ok_or_else(|| format!("missing entity for persistent id {}", item))?,
            target: action_target_from_snapshot(target, entities)?,
        },
        ActionKindSnapshot::Descend => ActionKind::Descend,
        ActionKindSnapshot::Ascend => ActionKind::Ascend,
    })
}

fn ai_goal_to_snapshot(
    goal: &AiGoal,
    ids: &HashMap<Entity, u64>,
) -> SnapshotResult<AiGoalSnapshot> {
    Ok(match goal {
        AiGoal::Idle => AiGoalSnapshot::Idle,
        AiGoal::Wander => AiGoalSnapshot::Wander,
        AiGoal::Investigate(position) => AiGoalSnapshot::Investigate(position_to_saved(*position)),
        AiGoal::Chase(entity) => AiGoalSnapshot::Chase(pid_of(*entity, ids)?),
        AiGoal::Flee(entity) => AiGoalSnapshot::Flee(pid_of(*entity, ids)?),
    })
}

fn ai_goal_from_snapshot(
    goal: &AiGoalSnapshot,
    entities: &HashMap<u64, Entity>,
) -> SnapshotResult<AiGoal> {
    Ok(match goal {
        AiGoalSnapshot::Idle => AiGoal::Idle,
        AiGoalSnapshot::Wander => AiGoal::Wander,
        AiGoalSnapshot::Investigate(position) => AiGoal::Investigate(saved_to_position(position)),
        AiGoalSnapshot::Chase(id) => AiGoal::Chase(
            *entities
                .get(id)
                .ok_or_else(|| format!("missing entity for persistent id {}", id))?,
        ),
        AiGoalSnapshot::Flee(id) => AiGoal::Flee(
            *entities
                .get(id)
                .ok_or_else(|| format!("missing entity for persistent id {}", id))?,
        ),
    })
}

fn effect_to_snapshot(
    effect: &Effect,
    ids: &HashMap<Entity, u64>,
) -> SnapshotResult<EffectSnapshot> {
    Ok(match effect {
        Effect::Damage {
            source,
            target,
            amount,
            kind,
        } => EffectSnapshot::Damage {
            source: match source {
                Some(entity) => Some(pid_of(*entity, ids)?),
                None => None,
            },
            target: pid_of(*target, ids)?,
            amount: *amount,
            kind: *kind,
        },
        Effect::Heal { target, amount } => EffectSnapshot::Heal {
            target: pid_of(*target, ids)?,
            amount: *amount,
        },
        Effect::Teleport {
            target,
            destination,
        } => EffectSnapshot::Teleport {
            target: pid_of(*target, ids)?,
            destination: position_to_saved(*destination),
        },
        Effect::ApplyStatus { target, status } => EffectSnapshot::ApplyStatus {
            target: pid_of(*target, ids)?,
            status: *status,
        },
    })
}

fn effect_from_snapshot(
    effect: &EffectSnapshot,
    entities: &HashMap<u64, Entity>,
) -> SnapshotResult<Effect> {
    Ok(match effect {
        EffectSnapshot::Damage {
            source,
            target,
            amount,
            kind,
        } => Effect::Damage {
            source: match source {
                Some(id) => Some(
                    *entities
                        .get(id)
                        .ok_or_else(|| format!("missing entity for persistent id {}", id))?,
                ),
                None => None,
            },
            target: *entities
                .get(target)
                .ok_or_else(|| format!("missing entity for persistent id {}", target))?,
            amount: *amount,
            kind: *kind,
        },
        EffectSnapshot::Heal { target, amount } => Effect::Heal {
            target: *entities
                .get(target)
                .ok_or_else(|| format!("missing entity for persistent id {}", target))?,
            amount: *amount,
        },
        EffectSnapshot::Teleport {
            target,
            destination,
        } => Effect::Teleport {
            target: *entities
                .get(target)
                .ok_or_else(|| format!("missing entity for persistent id {}", target))?,
            destination: saved_to_position(destination),
        },
        EffectSnapshot::ApplyStatus { target, status } => Effect::ApplyStatus {
            target: *entities
                .get(target)
                .ok_or_else(|| format!("missing entity for persistent id {}", target))?,
            status: *status,
        },
    })
}

fn build_level_snapshot(map: &LevelMap) -> LevelSnapshot {
    LevelSnapshot {
        id: 0,
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
                items.push(pid_of(*item, ids)?);
            }
            Ok::<SavedInventory, String>(SavedInventory {
                capacity: inventory.capacity,
                items,
            })
        })
        .transpose()?;
    let carried_by = entity_ref
        .get::<CarriedBy>()
        .map(|carried| pid_of(carried.0, ids))
        .transpose()?;
    let ai_goal = entity_ref
        .get::<AiGoal>()
        .map(|goal| ai_goal_to_snapshot(goal, ids))
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

    let entity_ids: HashSet<u64> = snapshot.entities.iter().map(|entity| entity.id).collect();
    let level_ids: HashSet<u32> = snapshot.levels.iter().map(|level| level.id).collect();

    if level_ids.len() != snapshot.levels.len() {
        return Err("duplicate level ids are not allowed".to_string());
    }

    if snapshot.levels.is_empty() {
        return Err("snapshot does not contain any levels".to_string());
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
        if level.tiles.len() != level.width as usize * level.height as usize {
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
        if let Some(position) = &entity.position {
            if !level_ids.contains(&position.level) {
                return Err(format!(
                    "entity {} references unknown level {}",
                    entity.id, position.level
                ));
            }
        }

        if let Some(inventory) = &entity.inventory {
            for item in &inventory.items {
                if !entity_ids.contains(item) {
                    return Err(format!(
                        "entity {} inventory references missing item {}",
                        entity.id, item
                    ));
                }
            }
        }

        if let Some(owner) = entity.carried_by
            && !entity_ids.contains(&owner)
        {
            return Err(format!(
                "entity {} carried_by references missing entity {}",
                entity.id, owner
            ));
        }

        if let Some(goal) = &entity.ai_goal {
            match goal {
                AiGoalSnapshot::Chase(id) | AiGoalSnapshot::Flee(id) => {
                    if !entity_ids.contains(id) {
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
                }
                AiGoalSnapshot::Idle | AiGoalSnapshot::Wander => {}
            }
        }

        if let Some(position) = &entity.last_known_player_position
            && !level_ids.contains(&position.level)
        {
            return Err(format!(
                "entity {} last-known position references missing level {}",
                entity.id, position.level
            ));
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

    for action in &snapshot.pending_actions {
        if !entity_ids.contains(&action.actor) {
            return Err(format!(
                "pending action references missing actor {}",
                action.actor
            ));
        }
        validate_action_kind_references(&action.kind, &entity_ids, &level_ids)?;
    }

    for effect in &snapshot.pending_effects {
        validate_effect_references(effect, &entity_ids, &level_ids)?;
    }

    for entry in &snapshot.timeline {
        if !entity_ids.contains(&entry.actor) {
            return Err(format!("timeline references missing actor {}", entry.actor));
        }
    }

    if let Some(current_actor) = snapshot.current_actor
        && !entity_ids.contains(&current_actor)
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
    entity_ids: &HashSet<u64>,
    level_ids: &HashSet<u32>,
) -> SnapshotResult<()> {
    match kind {
        ActionKindSnapshot::Wait
        | ActionKindSnapshot::Move { .. }
        | ActionKindSnapshot::Descend
        | ActionKindSnapshot::Ascend => Ok(()),
        ActionKindSnapshot::Melee { target }
        | ActionKindSnapshot::PickUp { item: target }
        | ActionKindSnapshot::Drop { item: target } => {
            if entity_ids.contains(target) {
                Ok(())
            } else {
                Err(format!("action references missing entity {}", target))
            }
        }
        ActionKindSnapshot::UseItem { item, target } => {
            if !entity_ids.contains(item) {
                return Err(format!("action references missing item {}", item));
            }
            match target {
                ActionTargetSnapshot::SelfTarget => Ok(()),
                ActionTargetSnapshot::Entity(id) => {
                    if entity_ids.contains(id) {
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
    entity_ids: &HashSet<u64>,
    level_ids: &HashSet<u32>,
) -> SnapshotResult<()> {
    match effect {
        EffectSnapshot::Damage { source, target, .. } => {
            if !entity_ids.contains(target) {
                return Err(format!("effect references missing target {}", target));
            }
            if let Some(source) = source
                && !entity_ids.contains(source)
            {
                return Err(format!("effect references missing source {}", source));
            }
            Ok(())
        }
        EffectSnapshot::Heal { target, .. } | EffectSnapshot::ApplyStatus { target, .. } => {
            if entity_ids.contains(target) {
                Ok(())
            } else {
                Err(format!("effect references missing target {}", target))
            }
        }
        EffectSnapshot::Teleport {
            target,
            destination,
        } => {
            if !entity_ids.contains(target) {
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
    for entity in world.iter_entities() {
        let durable = entity.contains::<Actor>() || entity.contains::<Item>();
        let persistent_id = entity.get::<PersistentId>();

        if durable && persistent_id.is_none() {
            return Err(format!(
                "durable entity {:?} is missing a persistent id",
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

    let current_actor = current_actor.map(|actor| pid_of(actor, &ids)).transpose()?;

    let mut timeline = Vec::new();
    for entry in clock.timeline.iter() {
        timeline.push(ScheduledActorSnapshot {
            next_tick: entry.0.next_tick,
            sequence: entry.0.sequence,
            actor: pid_of(entry.0.actor, &ids)?,
        });
    }
    timeline.sort_by_key(|entry| (entry.next_tick, entry.sequence, entry.actor));

    let mut pending_actions = Vec::new();
    for action in &action_queue.actions {
        pending_actions.push(ActionSnapshot {
            actor: pid_of(action.actor, &ids)?,
            kind: action_kind_to_snapshot(&action.kind, &ids)?,
        });
    }

    let mut pending_effects = Vec::new();
    for effect in &effect_queue.0 {
        pending_effects.push(effect_to_snapshot(effect, &ids)?);
    }

    if !matches!(*decision, crate::action::resolver::ActionDecision::Idle) {
        return Err("snapshot requires an idle action decision".to_string());
    }
    if current_actor.is_some()
        || !action_queue.actions.is_empty()
        || !effect_queue.0.is_empty()
        || simulation_status != SimulationStatus::WaitingForPlayer
            && simulation_status != SimulationStatus::GameOver
    {
        return Err("snapshot requires a stable save boundary".to_string());
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
    )>();
    for (entity, position, blocks_movement, blocks_sight) in query.iter(world) {
        let key = (position.level, position.cell);
        spatial.occupants.entry(key).or_default().push(entity);
        if blocks_movement.is_some() {
            spatial.movement_blockers.insert(key);
        }
        if blocks_sight.is_some() {
            spatial.sight_blockers.insert(key);
        }
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
    let mut map = LevelMap::new(level.width, level.height, TileKind::Wall);
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
                *entity_map
                    .get(&owner)
                    .ok_or_else(|| format!("missing carried_by entity {}", owner))?,
            ));
        }

        if let Some(inventory) = &entity.inventory {
            let mut items = Vec::with_capacity(inventory.items.len());
            for item in &inventory.items {
                items.push(
                    *entity_map
                        .get(item)
                        .ok_or_else(|| format!("missing inventory item {}", item))?,
                );
            }
            entity_mut.insert(Inventory {
                capacity: inventory.capacity,
                items,
            });
        }

        if let Some(goal) = &entity.ai_goal {
            entity_mut.insert(ai_goal_from_snapshot(goal, &entity_map)?);
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

    if let Some(current_actor) = snapshot.current_actor {
        let entity = *entity_map
            .get(&current_actor)
            .ok_or_else(|| format!("missing current actor {}", current_actor))?;
        world.insert_resource(CurrentActor(Some(entity)));
    }

    if let Some(mut clock) = world.get_resource_mut::<TurnClock>() {
        for entry in &snapshot.timeline {
            clock.timeline.push(std::cmp::Reverse(ScheduledActor {
                next_tick: entry.next_tick,
                sequence: entry.sequence,
                actor: *entity_map
                    .get(&entry.actor)
                    .ok_or_else(|| format!("missing scheduled actor {}", entry.actor))?,
            }));
        }
    }

    if let Some(mut queue) = world.get_resource_mut::<ActionQueue>() {
        for action in &snapshot.pending_actions {
            queue.actions.push_back(Action {
                actor: *entity_map
                    .get(&action.actor)
                    .ok_or_else(|| format!("missing queued actor {}", action.actor))?,
                kind: action_kind_from_snapshot(&action.kind, &entity_map)?,
            });
        }
    }

    if let Some(mut effects) = world.get_resource_mut::<EffectQueue>() {
        for effect in &snapshot.pending_effects {
            effects
                .0
                .push_back(effect_from_snapshot(effect, &entity_map)?);
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
        Err(current_err) => match ron::de::from_bytes::<LegacyGameSnapshotV1>(bytes) {
            Ok(snapshot) => Ok(SnapshotFile::V1(snapshot)),
            Err(legacy_err) => Err(format!(
                "failed to deserialize current snapshot: {}; legacy snapshot: {}",
                current_err, legacy_err
            )),
        },
    }
}

pub fn snapshot_to_bytes(snapshot: &GameSnapshot) -> SnapshotResult<Vec<u8>> {
    snapshot_bytes(snapshot)
}

pub fn snapshot_from_text(text: &str) -> SnapshotResult<SnapshotFile> {
    match ron::from_str::<GameSnapshot>(text) {
        Ok(snapshot) => Ok(SnapshotFile::Current(snapshot)),
        Err(current_err) => match ron::from_str::<LegacyGameSnapshotV1>(text) {
            Ok(snapshot) => Ok(SnapshotFile::V1(snapshot)),
            Err(legacy_err) => Err(format!(
                "failed to deserialize current snapshot: {}; legacy snapshot: {}",
                current_err, legacy_err
            )),
        },
    }
}

pub fn snapshot_to_text(snapshot: &GameSnapshot) -> SnapshotResult<String> {
    ron::ser::to_string(snapshot).map_err(|err| err.to_string())
}

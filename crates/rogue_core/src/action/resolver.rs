use bevy_ecs::prelude::*;
use std::collections::VecDeque;

use crate::action::intent::{Action, ActionKind, ActionTarget};
use crate::action::queue::ActionQueue;
use crate::action::schedule::action_cost;
use crate::actor::combat::{DamageKind, melee_damage};
use crate::actor::components::{
    ActionSpeed, BlocksMovement, CombatStats, Health, Monster, Player, PrototypeId,
};
use crate::item::components::{CarriedBy, Inventory, Item};
use crate::item::effects::{Effect, EffectQueue};
use crate::persistence::rng::RandomStreams;
use crate::simulation::SimulationStatus;
use crate::time::clock::TurnClock;
use crate::world::fov::line_of_sight;
use crate::world::map::GridPosition;
use crate::world::map::{LevelId, LevelMap};
use crate::world::spatial::SpatialIndex;
use crate::world::tile::TileKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionFailure {
    Blocked,
    InvalidTarget,
    OutOfRange,
    MissingItem,
    InventoryFull,
    ActorUnavailable,
    Unsupported,
}

#[derive(Resource, Debug, Clone)]
pub enum ActionDecision {
    Idle,
    WaitingForPlayer,
    Ready(Action),
    Failed {
        action: Action,
        failure: ActionFailure,
    },
}

impl Default for ActionDecision {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone)]
pub enum ActionOutcome {
    Resolved(Action),
    Failed {
        action: Action,
        failure: ActionFailure,
    },
}

#[derive(Resource, Debug, Default, Clone)]
pub struct ActionOutcomeLog {
    pub outcomes: VecDeque<ActionOutcome>,
}

impl ActionOutcomeLog {
    pub fn push(&mut self, outcome: ActionOutcome) {
        self.outcomes.push_back(outcome);
    }

    pub fn latest(&self) -> Option<&ActionOutcome> {
        self.outcomes.back()
    }
}

fn action_ticks_from_base(base: u64, speed: u64) -> u64 {
    base.saturating_mul(speed.max(1)) / 100
}

fn action_ticks(kind: &ActionKind, speed: u64) -> u64 {
    action_ticks_from_base(action_cost(kind), speed)
}

fn scheduled_speed(speeds: &Query<'_, '_, &ActionSpeed>, actor: Entity) -> u64 {
    speeds
        .get(actor)
        .map(|speed| speed.ticks_per_action)
        .unwrap_or(100)
}

fn actor_flags(
    positions: &Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&Player>,
            Option<&Monster>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::HostileToPlayer>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
        ),
    >,
    entity: Entity,
) -> Option<(
    GridPosition,
    bool,
    bool,
    Option<Health>,
    bool,
    bool,
    Option<CombatStats>,
)> {
    positions.get(entity).ok().map(
        |(
            _,
            position,
            player,
            monster,
            health,
            blocks_movement,
            hostile,
            stats,
            _item,
            _carried_by,
            _prototype,
        )| {
            (
                *position,
                player.is_some(),
                monster.is_some(),
                health.copied(),
                blocks_movement.is_some(),
                hostile.is_some(),
                stats.copied(),
            )
        },
    )
}

fn can_attack(
    attacker_is_player: bool,
    attacker_is_hostile_to_player: bool,
    target_is_player: bool,
    target_is_monster: bool,
    target_is_hostile_to_player: bool,
) -> bool {
    if attacker_is_player {
        return target_is_monster && target_is_hostile_to_player;
    }

    attacker_is_hostile_to_player && target_is_player
}

fn find_valid_move_collision(
    action_actor: Entity,
    destination: GridPosition,
    attacker_is_player: bool,
    attacker_is_hostile_to_player: bool,
    positions: &Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&Player>,
            Option<&Monster>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::HostileToPlayer>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
        ),
    >,
    spatial: &SpatialIndex,
) -> Option<Entity> {
    let mut candidate = None;
    let mut has_blocker = false;

    for occupant in spatial.occupants_at(destination.level, destination.cell) {
        if occupant == action_actor {
            continue;
        }

        let Ok((
            entity,
            _,
            occupant_player,
            occupant_monster,
            occupant_health,
            occupant_blocks_movement,
            occupant_hostile,
            occupant_stats,
            _item,
            _carried_by,
            _prototype,
        )) = positions.get(occupant)
        else {
            continue;
        };

        if occupant_health.is_some_and(|health| health.current <= 0) {
            continue;
        }

        if occupant_blocks_movement.is_none() {
            continue;
        }

        has_blocker = true;

        if can_attack(
            attacker_is_player,
            attacker_is_hostile_to_player,
            occupant_player.is_some(),
            occupant_monster.is_some(),
            occupant_hostile.is_some(),
        ) && occupant_stats.is_some()
        {
            candidate = Some(entity);
        } else {
            return None;
        }
    }

    if has_blocker { candidate } else { None }
}

pub fn validate_action(
    mut queue: ResMut<'_, ActionQueue>,
    current_actor: Option<Res<'_, crate::time::clock::CurrentActor>>,
    mut decision: ResMut<'_, ActionDecision>,
    mut status: ResMut<'_, SimulationStatus>,
    map: Res<'_, LevelMap>,
    spatial: Res<'_, SpatialIndex>,
    inventories: Query<'_, '_, &Inventory>,
    positions: Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&Player>,
            Option<&Monster>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::HostileToPlayer>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
        ),
    >,
) {
    *decision = ActionDecision::Idle;

    let Some(current_actor) = current_actor else {
        return;
    };
    let Some(current_actor) = current_actor.0 else {
        return;
    };

    let Some((_, current_is_player, _, _, _, current_is_hostile_to_player, _)) =
        actor_flags(&positions, current_actor)
    else {
        *decision = ActionDecision::Failed {
            action: Action {
                actor: current_actor,
                kind: ActionKind::Wait,
            },
            failure: ActionFailure::ActorUnavailable,
        };
        return;
    };

    let Some(action) = queue.take_for_actor(current_actor) else {
        if current_is_player {
            *status = SimulationStatus::WaitingForPlayer;
            *decision = ActionDecision::WaitingForPlayer;
        } else {
            let action = Action {
                actor: current_actor,
                kind: ActionKind::Wait,
            };
            *decision = ActionDecision::Failed {
                action: action.clone(),
                failure: ActionFailure::ActorUnavailable,
            };
        }
        debug_assert!(
            !current_is_player || !current_is_hostile_to_player,
            "player actor should not be marked hostile-to-player"
        );
        return;
    };

    let validation = validate_action_kind(
        &action,
        &map,
        &spatial,
        current_is_player,
        current_is_hostile_to_player,
        &positions,
        &inventories,
    );
    *decision = match validation {
        Ok(()) => ActionDecision::Ready(action.clone()),
        Err(failure) => ActionDecision::Failed {
            action: action.clone(),
            failure,
        },
    };
}

pub fn resolve_action(
    mut commands: Commands<'_, '_>,
    mut effects: ResMut<'_, EffectQueue>,
    mut map: ResMut<'_, LevelMap>,
    spatial: Res<'_, SpatialIndex>,
    positions: Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&Player>,
            Option<&Monster>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::HostileToPlayer>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
        ),
    >,
    speeds: Query<'_, '_, &ActionSpeed>,
    mut inventories: Query<'_, '_, &mut Inventory>,
    current_actor: Option<Res<'_, crate::time::clock::CurrentActor>>,
    mut turn_clock: ResMut<'_, TurnClock>,
    mut decision: ResMut<'_, ActionDecision>,
    mut outcomes: ResMut<'_, ActionOutcomeLog>,
    mut rng: ResMut<'_, RandomStreams>,
) {
    let Some(current_actor) = current_actor else {
        return;
    };
    let Some(current_actor) = current_actor.0 else {
        return;
    };

    let decision = std::mem::replace(&mut *decision, ActionDecision::Idle);
    let (action, failure) = match decision {
        ActionDecision::Ready(action) => (action, None),
        ActionDecision::Failed { action, failure } => (action, Some(failure)),
        ActionDecision::WaitingForPlayer | ActionDecision::Idle => {
            return;
        }
    };

    if action.actor != current_actor {
        debug_assert_eq!(
            action.actor, current_actor,
            "validated action did not belong to the current actor"
        );
        return;
    }

    let actor_speed = scheduled_speed(&speeds, action.actor);
    let should_reschedule = !matches!(failure, Some(ActionFailure::ActorUnavailable));

    match action.kind.clone() {
        ActionKind::Wait => {
            if should_reschedule {
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
            }
        }
        ActionKind::Move { delta } => {
            let Some((
                actor_position,
                attacker_is_player,
                _,
                _,
                _,
                attacker_is_hostile_to_player,
                attacker_stats,
            )) = actor_flags(&positions, action.actor)
            else {
                if should_reschedule {
                    schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
                }
                outcomes.push(match failure {
                    Some(failure) => ActionOutcome::Failed {
                        action: action.clone(),
                        failure,
                    },
                    None => ActionOutcome::Failed {
                        action: action.clone(),
                        failure: ActionFailure::ActorUnavailable,
                    },
                });
                return;
            };

            let destination = GridPosition {
                level: actor_position.level,
                cell: actor_position.cell + delta,
            };

            if !map.in_bounds(destination.cell) {
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
            } else {
                match map.tile(destination.cell).map(|tile| tile.kind) {
                    Some(TileKind::Wall) | None => {
                        schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
                    }
                    Some(TileKind::ClosedDoor) => {
                        if let Some(tile) = map.tile_mut(destination.cell) {
                            tile.kind = TileKind::OpenDoor;
                        }
                        schedule_actor_with_base(&mut turn_clock, action.actor, 80, actor_speed);
                    }
                    Some(TileKind::Floor)
                    | Some(TileKind::OpenDoor)
                    | Some(TileKind::StairsDown)
                    | Some(TileKind::StairsUp) => {
                        if let Some(target) = find_valid_move_collision(
                            action.actor,
                            destination,
                            attacker_is_player,
                            attacker_is_hostile_to_player,
                            &positions,
                            &spatial,
                        ) {
                            if let Some((
                                _,
                                target_position,
                                _,
                                _,
                                _,
                                _,
                                _,
                                target_stats,
                                _,
                                _,
                                _,
                            )) = positions.get(target).ok()
                            {
                                if let (Some(attacker_stats), Some(target_stats)) =
                                    (attacker_stats, target_stats.copied())
                                {
                                    effects.0.push_back(Effect::Damage {
                                        source: Some(action.actor),
                                        target,
                                        amount: melee_damage(
                                            attacker_stats,
                                            target_stats,
                                            Some(&mut rng),
                                        ),
                                        kind: DamageKind::Melee,
                                    });
                                }

                                debug_assert_eq!(
                                    target_position.level, actor_position.level,
                                    "movement collision target must share the actor level"
                                );
                                schedule_actor(
                                    &mut turn_clock,
                                    action.actor,
                                    &ActionKind::Melee { target },
                                    actor_speed,
                                );
                            } else {
                                schedule_actor(
                                    &mut turn_clock,
                                    action.actor,
                                    &action.kind,
                                    actor_speed,
                                );
                            }
                        } else if spatial
                            .occupants_at(actor_position.level, destination.cell)
                            .all(|occupant| {
                                positions.get(occupant).map_or(
                                    true,
                                    |(_, _, _, _, _, blocks_movement, _, _, _, _, _)| {
                                        blocks_movement.is_none()
                                    },
                                )
                            })
                        {
                            commands.entity(action.actor).insert(GridPosition {
                                level: actor_position.level,
                                cell: destination.cell,
                            });
                            schedule_actor(
                                &mut turn_clock,
                                action.actor,
                                &action.kind,
                                actor_speed,
                            );
                        } else {
                            schedule_actor(
                                &mut turn_clock,
                                action.actor,
                                &action.kind,
                                actor_speed,
                            );
                        }
                    }
                }
            }
        }
        ActionKind::Melee { target } => {
            let Some((
                actor_position,
                attacker_is_player,
                _,
                _,
                _,
                attacker_is_hostile_to_player,
                attacker_stats,
            )) = actor_flags(&positions, action.actor)
            else {
                if should_reschedule {
                    schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
                }
                outcomes.push(match failure {
                    Some(failure) => ActionOutcome::Failed {
                        action: action.clone(),
                        failure,
                    },
                    None => ActionOutcome::Failed {
                        action: action.clone(),
                        failure: ActionFailure::ActorUnavailable,
                    },
                });
                return;
            };
            let Some((
                target_position,
                target_is_player,
                target_is_monster,
                target_health,
                _,
                target_is_hostile_to_player,
                target_stats,
            )) = target_flags(&positions, target)
            else {
                if should_reschedule {
                    schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
                }
                outcomes.push(match failure {
                    Some(failure) => ActionOutcome::Failed {
                        action: action.clone(),
                        failure,
                    },
                    None => ActionOutcome::Failed {
                        action: action.clone(),
                        failure: ActionFailure::ActorUnavailable,
                    },
                });
                return;
            };

            let valid = target_health
                .map(|health| health.current > 0)
                .unwrap_or(false)
                && actor_position.level == target_position.level
                && (target_position.cell - actor_position.cell)
                    .x
                    .abs()
                    .max((target_position.cell - actor_position.cell).y.abs())
                    == 1
                && line_of_sight(
                    &map,
                    &spatial,
                    actor_position.level,
                    actor_position.cell,
                    target_position.cell,
                )
                && can_attack(
                    attacker_is_player,
                    attacker_is_hostile_to_player,
                    target_is_player,
                    target_is_monster,
                    target_is_hostile_to_player,
                );

            if valid {
                if let (Some(attacker_stats), Some(target_stats)) = (attacker_stats, target_stats) {
                    effects.0.push_back(Effect::Damage {
                        source: Some(action.actor),
                        target,
                        amount: melee_damage(attacker_stats, target_stats, Some(&mut rng)),
                        kind: DamageKind::Melee,
                    });
                }
            }

            if should_reschedule {
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
            }
        }
        ActionKind::PickUp { item } => {
            let Some(mut inventory) = inventories.get_mut(action.actor).ok() else {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::MissingItem,
                });
                return;
            };
            if inventory.is_full() {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::InventoryFull,
                });
                return;
            }
            if !inventory.items.contains(&item) {
                inventory.items.push(item);
            }
            commands.entity(item).insert(CarriedBy(action.actor));
            if should_reschedule {
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
            }
        }
        ActionKind::Drop { item } => {
            let Some(mut inventory) = inventories.get_mut(action.actor).ok() else {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::MissingItem,
                });
                return;
            };
            let Some(index) = inventory.items.iter().position(|entry| *entry == item) else {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::MissingItem,
                });
                return;
            };
            inventory.items.remove(index);
            if let Some((_, actor_position, _, _, _, _, _, _, _, _, _)) =
                positions.get(action.actor).ok()
            {
                commands.entity(item).insert(*actor_position);
            }
            commands.entity(item).remove::<CarriedBy>();
            if should_reschedule {
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
            }
        }
        ActionKind::UseItem { item, target } => {
            let Some(mut inventory) = inventories.get_mut(action.actor).ok() else {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::MissingItem,
                });
                return;
            };
            let Some(index) = inventory.items.iter().position(|entry| *entry == item) else {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::MissingItem,
                });
                return;
            };
            let prototype = positions
                .get(item)
                .ok()
                .and_then(|(_, _, _, _, _, _, _, _, _, _, prototype)| {
                    prototype.map(|prototype| prototype.0.clone())
                })
                .unwrap_or_else(|| "unknown".to_string());
            inventory.items.remove(index);
            commands.entity(item).despawn();

            let use_target_is_self = match target {
                ActionTarget::SelfTarget => true,
                ActionTarget::Entity(entity) => entity == action.actor,
                _ => false,
            };
            if use_target_is_self {
                let amount = if prototype == "healing_potion" { 3 } else { 1 };
                effects.0.push_back(Effect::Heal {
                    target: action.actor,
                    amount,
                });
            }

            if should_reschedule {
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
            }
        }
        ActionKind::Descend | ActionKind::Ascend => {
            if should_reschedule {
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
            }
        }
    }

    outcomes.push(match failure {
        Some(failure) => ActionOutcome::Failed { action, failure },
        None => ActionOutcome::Resolved(action),
    });
}

fn target_flags(
    positions: &Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&Player>,
            Option<&Monster>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::HostileToPlayer>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
        ),
    >,
    entity: Entity,
) -> Option<(
    GridPosition,
    bool,
    bool,
    Option<Health>,
    bool,
    bool,
    Option<CombatStats>,
)> {
    positions.get(entity).ok().map(
        |(
            _,
            position,
            player,
            monster,
            health,
            blocks_movement,
            hostile,
            stats,
            _item,
            _carried_by,
            _prototype,
        )| {
            (
                *position,
                player.is_some(),
                monster.is_some(),
                health.copied(),
                blocks_movement.is_some(),
                hostile.is_some(),
                stats.copied(),
            )
        },
    )
}

fn schedule_actor(turn_clock: &mut TurnClock, actor: Entity, kind: &ActionKind, actor_speed: u64) {
    turn_clock.schedule_after(actor, action_ticks(kind, actor_speed));
}

fn schedule_actor_with_base(
    turn_clock: &mut TurnClock,
    actor: Entity,
    base: u64,
    actor_speed: u64,
) {
    turn_clock.schedule_after(actor, action_ticks_from_base(base, actor_speed));
}

fn validate_action_kind(
    action: &Action,
    map: &LevelMap,
    spatial: &SpatialIndex,
    attacker_is_player: bool,
    attacker_is_hostile_to_player: bool,
    positions: &Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&Player>,
            Option<&Monster>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::HostileToPlayer>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
        ),
    >,
    inventories: &Query<'_, '_, &Inventory>,
) -> Result<(), ActionFailure> {
    match &action.kind {
        ActionKind::Wait => Ok(()),
        ActionKind::Move { delta } => {
            let Some((actor_position, _, _, _, _, _, _)) = actor_flags(positions, action.actor)
            else {
                return Err(ActionFailure::ActorUnavailable);
            };

            let destination = GridPosition {
                level: actor_position.level,
                cell: actor_position.cell + *delta,
            };

            if !map.in_bounds(destination.cell) {
                return Err(ActionFailure::Blocked);
            }

            match map.tile(destination.cell).map(|tile| tile.kind) {
                Some(TileKind::Wall) | None => return Err(ActionFailure::Blocked),
                Some(TileKind::ClosedDoor) => return Ok(()),
                Some(TileKind::Floor)
                | Some(TileKind::OpenDoor)
                | Some(TileKind::StairsDown)
                | Some(TileKind::StairsUp) => {}
            }

            if find_valid_move_collision(
                action.actor,
                destination,
                attacker_is_player,
                attacker_is_hostile_to_player,
                positions,
                spatial,
            )
            .is_some()
            {
                return Ok(());
            }

            let has_blocker = spatial
                .occupants_at(actor_position.level, destination.cell)
                .any(|occupant| {
                    positions.get(occupant).is_ok_and(
                        |(_, _, _, _, _, blocks_movement, _, _, _, _, _)| blocks_movement.is_some(),
                    )
                });

            if has_blocker {
                Err(ActionFailure::Blocked)
            } else {
                Ok(())
            }
        }
        ActionKind::Melee { target } => {
            let Some((actor_position, _, _, _, _, _, attacker_stats)) =
                actor_flags(positions, action.actor)
            else {
                return Err(ActionFailure::ActorUnavailable);
            };
            let Some((
                target_position,
                target_is_player,
                target_is_monster,
                target_health,
                _,
                target_is_hostile_to_player,
                target_stats,
            )) = target_flags(positions, *target)
            else {
                return Err(ActionFailure::InvalidTarget);
            };

            if target_health.is_none_or(|health| health.current <= 0)
                || attacker_stats.is_none()
                || target_stats.is_none()
            {
                return Err(ActionFailure::InvalidTarget);
            }

            if actor_position.level != target_position.level {
                return Err(ActionFailure::InvalidTarget);
            }

            let delta = target_position.cell - actor_position.cell;
            if delta.x.abs().max(delta.y.abs()) != 1 {
                return Err(ActionFailure::OutOfRange);
            }

            if !line_of_sight(
                map,
                &spatial,
                actor_position.level,
                actor_position.cell,
                target_position.cell,
            ) {
                return Err(ActionFailure::Blocked);
            }

            if !can_attack(
                attacker_is_player,
                attacker_is_hostile_to_player,
                target_is_player,
                target_is_monster,
                target_is_hostile_to_player,
            ) {
                return Err(ActionFailure::InvalidTarget);
            }

            Ok(())
        }
        ActionKind::PickUp { item } => {
            let Some((_, actor_position, _, _, _, _, _, _, _, _, _)) =
                positions.get(action.actor).ok()
            else {
                return Err(ActionFailure::ActorUnavailable);
            };
            let Some((_, item_position, _, _, _, _, _, _, Some(_), carried_by, _)) =
                positions.get(*item).ok()
            else {
                return Err(ActionFailure::InvalidTarget);
            };

            if carried_by.is_some() {
                return Err(ActionFailure::MissingItem);
            }

            let Some(inventory) = inventories.get(action.actor).ok() else {
                return Err(ActionFailure::MissingItem);
            };
            if inventory.is_full() {
                return Err(ActionFailure::InventoryFull);
            }

            if actor_position.level != item_position.level
                || actor_position.cell != item_position.cell
            {
                return Err(ActionFailure::InvalidTarget);
            }

            Ok(())
        }
        ActionKind::Drop { item } => {
            let Some((_, actor_position, _, _, _, _, _, _, _, _, _)) =
                positions.get(action.actor).ok()
            else {
                return Err(ActionFailure::ActorUnavailable);
            };
            let Some((_, item_position, _, _, _, _, _, _, Some(_), carried_by, _)) =
                positions.get(*item).ok()
            else {
                return Err(ActionFailure::InvalidTarget);
            };

            if carried_by.map(|carried| carried.0) != Some(action.actor) {
                return Err(ActionFailure::MissingItem);
            }
            if inventories.get(action.actor).is_err() {
                return Err(ActionFailure::MissingItem);
            }
            if actor_position.level != item_position.level {
                return Err(ActionFailure::InvalidTarget);
            }
            Ok(())
        }
        ActionKind::UseItem { item, target } => {
            let Some((_, _, _, _, _, _, _, _, _, _, _)) = positions.get(action.actor).ok() else {
                return Err(ActionFailure::ActorUnavailable);
            };
            let Some((_, _, _, _, _, _, _, _, Some(_), carried_by, _)) = positions.get(*item).ok()
            else {
                return Err(ActionFailure::InvalidTarget);
            };
            if inventories.get(action.actor).is_err() {
                return Err(ActionFailure::MissingItem);
            }
            if carried_by.map(|carried| carried.0) != Some(action.actor) {
                return Err(ActionFailure::MissingItem);
            }
            match target {
                ActionTarget::SelfTarget => Ok(()),
                ActionTarget::Entity(entity) if *entity == action.actor => Ok(()),
                _ => Err(ActionFailure::InvalidTarget),
            }
        }
        ActionKind::Descend | ActionKind::Ascend => Err(ActionFailure::Unsupported),
    }
}

pub fn select_next_action(_queue: Res<'_, ActionQueue>) {}

pub fn resolve_action_target(target: ActionTarget) -> Option<LevelId> {
    match target {
        ActionTarget::SelfTarget => None,
        ActionTarget::Entity(_) => None,
        ActionTarget::Cell { level, .. } => Some(level),
    }
}

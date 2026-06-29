use bevy_ecs::prelude::*;

use crate::action::intent::{Action, ActionKind, ActionTarget};
use crate::action::queue::ActionQueue;
use crate::action::schedule::action_cost;
use crate::actor::combat::{DamageKind, melee_damage};
use crate::actor::components::{ActionSpeed, BlocksMovement, CombatStats, Health, Monster, Player};
use crate::item::effects::{Effect, EffectQueue};
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
        |(_, position, player, monster, health, blocks_movement, hostile, stats)| {
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

fn is_hostile_target(
    attacker_is_player: bool,
    target_is_player: bool,
    target_is_monster: bool,
    target_is_hostile_to_player: bool,
) -> bool {
    if attacker_is_player {
        return target_is_monster && target_is_hostile_to_player;
    }

    target_is_player
}

fn find_valid_move_collision(
    action_actor: Entity,
    destination: GridPosition,
    attacker_is_player: bool,
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

        if is_hostile_target(
            attacker_is_player,
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

    loop {
        let Some(front) = queue.actions.front() else {
            *status = SimulationStatus::WaitingForPlayer;
            debug_assert!(
                actor_flags(&positions, current_actor)
                    .map(|(_, is_player, _, _, _, _, _)| is_player)
                    .unwrap_or(false),
                "scheduled actor had no available action"
            );
            *decision = ActionDecision::WaitingForPlayer;
            return;
        };

        if front.actor != current_actor {
            let stale = queue.pop().expect("front action must exist");
            debug_assert_ne!(
                stale.actor, current_actor,
                "discarded queued action for the wrong actor"
            );
            continue;
        }

        let action = queue.pop().expect("validated front action");
        let validation = validate_action_kind(&action, &map, &spatial, &positions);
        *decision = match validation {
            Ok(()) => ActionDecision::Ready(action),
            Err(failure) => ActionDecision::Failed { action, failure },
        };
        return;
    }
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
        ),
    >,
    speeds: Query<'_, '_, &ActionSpeed>,
    current_actor: Option<Res<'_, crate::time::clock::CurrentActor>>,
    mut turn_clock: ResMut<'_, TurnClock>,
    mut decision: ResMut<'_, ActionDecision>,
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

    match action.kind.clone() {
        ActionKind::Wait => {
            schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
        }
        ActionKind::Move { delta } => {
            let Some((actor_position, attacker_is_player, _, _, _, _, attacker_stats)) =
                actor_flags(&positions, action.actor)
            else {
                return;
            };

            let destination = GridPosition {
                level: actor_position.level,
                cell: actor_position.cell + delta,
            };

            if !map.in_bounds(destination.cell) {
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
                return;
            }

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
                        &positions,
                        &spatial,
                    ) {
                        if let Some((_, target_position, _, _, _, _, _, target_stats)) =
                            positions.get(target).ok()
                        {
                            if let (Some(attacker_stats), Some(target_stats)) =
                                (attacker_stats, target_stats.copied())
                            {
                                effects.0.push_back(Effect::Damage {
                                    source: Some(action.actor),
                                    target,
                                    amount: melee_damage(attacker_stats, target_stats),
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
                            positions
                                .get(occupant)
                                .map_or(true, |(_, _, _, _, _, blocks_movement, _, _)| {
                                    blocks_movement.is_none()
                                })
                        })
                    {
                        commands.entity(action.actor).insert(GridPosition {
                            level: actor_position.level,
                            cell: destination.cell,
                        });
                        schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
                    } else {
                        schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
                    }
                }
            }
        }
        ActionKind::Melee { target } => {
            let Some((actor_position, attacker_is_player, _, _, _, _, attacker_stats)) =
                actor_flags(&positions, action.actor)
            else {
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
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
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
                && line_of_sight(&map, actor_position.cell, target_position.cell)
                && is_hostile_target(
                    attacker_is_player,
                    target_is_player,
                    target_is_monster,
                    target_is_hostile_to_player,
                );

            if valid {
                if let (Some(attacker_stats), Some(target_stats)) = (attacker_stats, target_stats) {
                    effects.0.push_back(Effect::Damage {
                        source: Some(action.actor),
                        target,
                        amount: melee_damage(attacker_stats, target_stats),
                        kind: DamageKind::Melee,
                    });
                }
            }

            let _ = failure;
            schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
        }
        ActionKind::PickUp { .. }
        | ActionKind::Drop { .. }
        | ActionKind::UseItem { .. }
        | ActionKind::Descend
        | ActionKind::Ascend => {
            let _ = failure;
            schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
        }
    }
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
        |(_, position, player, monster, health, blocks_movement, hostile, stats)| {
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
        ),
    >,
) -> Result<(), ActionFailure> {
    match &action.kind {
        ActionKind::Wait => Ok(()),
        ActionKind::Move { delta } => {
            let Some((actor_position, attacker_is_player, _, _, _, _, _)) =
                actor_flags(positions, action.actor)
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
                positions,
                spatial,
            )
            .is_some()
            {
                return Ok(());
            }

            let has_blocker =
                spatial
                    .occupants_at(actor_position.level, destination.cell)
                    .any(|occupant| {
                        positions.get(occupant).is_ok_and(
                            |(_, _, _, _, _, blocks_movement, _, _)| blocks_movement.is_some(),
                        )
                    });

            if has_blocker {
                Err(ActionFailure::Blocked)
            } else {
                Ok(())
            }
        }
        ActionKind::Melee { target } => {
            let Some((actor_position, attacker_is_player, _, _, _, _, attacker_stats)) =
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

            if !line_of_sight(map, actor_position.cell, target_position.cell) {
                return Err(ActionFailure::Blocked);
            }

            if !is_hostile_target(
                attacker_is_player,
                target_is_player,
                target_is_monster,
                target_is_hostile_to_player,
            ) {
                return Err(ActionFailure::InvalidTarget);
            }

            Ok(())
        }
        ActionKind::PickUp { .. }
        | ActionKind::Drop { .. }
        | ActionKind::UseItem { .. }
        | ActionKind::Descend
        | ActionKind::Ascend => Err(ActionFailure::Unsupported),
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

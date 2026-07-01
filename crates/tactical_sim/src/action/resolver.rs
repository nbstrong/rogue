use bevy_ecs::prelude::*;
use std::collections::VecDeque;

use crate::action::intent::{Action, ActionKind, ActionTarget};
use crate::action::queue::ActionQueue;
use crate::action::schedule::action_cost;
use crate::actor::combat::{DamageKind, melee_damage};
use crate::actor::components::{
    ActionSpeed, ActorId, BlocksMovement, CombatStats, ControlledActor, Health, HostileActor,
    PrototypeId, StableActorId, StableEntityIndex,
};
use crate::content::definitions::ItemUseEffect;
use crate::content::registry::ContentRegistry;
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
    AwaitingInput,
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

fn scheduled_speed(
    stable_index: &StableEntityIndex,
    speeds: &Query<'_, '_, &ActionSpeed>,
    actor: ActorId,
) -> u64 {
    let Some(actor_entity) = stable_index.actor(actor) else {
        return 100;
    };
    speeds
        .get(actor_entity)
        .map(|speed| speed.ticks_per_action)
        .unwrap_or(100)
}

fn actor_flags(
    stable_index: &StableEntityIndex,
    positions: &Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&ControlledActor>,
            Option<&HostileActor>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::Hostile>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
            Option<&StableActorId>,
        ),
    >,
    actor: ActorId,
) -> Option<(
    GridPosition,
    bool,
    bool,
    Option<Health>,
    bool,
    bool,
    Option<CombatStats>,
)> {
    let entity = stable_index.actor(actor)?;
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
            _stable_actor_id,
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
    attacker_is_controlled: bool,
    attacker_is_hostile: bool,
    target_is_controlled: bool,
    target_is_hostile_actor: bool,
    target_is_hostile: bool,
) -> bool {
    if attacker_is_controlled {
        return target_is_hostile_actor && target_is_hostile;
    }

    attacker_is_hostile && target_is_controlled
}

fn find_valid_move_collision(
    stable_index: &StableEntityIndex,
    action_actor: ActorId,
    destination: GridPosition,
    attacker_is_controlled: bool,
    attacker_is_hostile: bool,
    positions: &Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&ControlledActor>,
            Option<&HostileActor>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::Hostile>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
            Option<&StableActorId>,
        ),
    >,
    spatial: &SpatialIndex,
) -> Option<ActorId> {
    let mut candidate = None;
    let mut has_blocker = false;
    let Some(action_actor_entity) = stable_index.actor(action_actor) else {
        return None;
    };

    for occupant in spatial.occupants_at(destination.level, destination.cell) {
        if occupant == action_actor_entity {
            continue;
        }

        let Ok((
            _entity,
            _,
            occupant_controlled,
            occupant_hostile_actor,
            occupant_health,
            occupant_blocks_movement,
            occupant_hostile,
            occupant_stats,
            _item,
            _carried_by,
            _prototype,
            occupant_stable_id,
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
            attacker_is_controlled,
            attacker_is_hostile,
            occupant_controlled.is_some(),
            occupant_hostile_actor.is_some(),
            occupant_hostile.is_some(),
        ) && occupant_stats.is_some()
        {
            candidate = occupant_stable_id.map(|id| id.0);
        } else {
            return None;
        }
    }

    if has_blocker { candidate } else { None }
}

pub fn validate_action(
    mut queue: ResMut<'_, ActionQueue>,
    current_actor: Option<Res<'_, crate::time::clock::CurrentActor>>,
    stable_index: Res<'_, StableEntityIndex>,
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
            Option<&ControlledActor>,
            Option<&HostileActor>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::Hostile>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
            Option<&StableActorId>,
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

    let Some((_, current_is_controlled, _, _, _, current_is_hostile, _)) =
        actor_flags(&stable_index, &positions, current_actor)
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
        if current_is_controlled {
            *status = SimulationStatus::AwaitingInput;
            *decision = ActionDecision::AwaitingInput;
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
        debug_assert!(!current_is_controlled || !current_is_hostile);
        return;
    };

    let validation = validate_action_kind(
        &action,
        &map,
        &spatial,
        current_is_controlled,
        current_is_hostile,
        &stable_index,
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
    stable_index: Res<'_, StableEntityIndex>,
    positions: Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&ControlledActor>,
            Option<&HostileActor>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::Hostile>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
            Option<&StableActorId>,
        ),
    >,
    content: Option<Res<'_, ContentRegistry>>,
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
        ActionDecision::AwaitingInput | ActionDecision::Idle => {
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

    let actor_speed = scheduled_speed(&stable_index, &speeds, action.actor);
    let should_reschedule = !matches!(failure, Some(ActionFailure::ActorUnavailable));

    if let Some(failure) = failure {
        if should_reschedule {
            schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
        }
        outcomes.push(ActionOutcome::Failed { action, failure });
        return;
    }

    match action.kind.clone() {
        ActionKind::Wait => {
            if should_reschedule {
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
            }
        }
        ActionKind::Move { delta } => {
            let Some((
                actor_position,
                attacker_is_controlled,
                _,
                _,
                _,
                attacker_is_hostile,
                attacker_stats,
            )) = actor_flags(&stable_index, &positions, action.actor)
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
                            &stable_index,
                            action.actor,
                            destination,
                            attacker_is_controlled,
                            attacker_is_hostile,
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
                                _,
                            )) = stable_index
                                .actor(target)
                                .and_then(|entity| positions.get(entity).ok())
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
                                    |(_, _, _, _, _, blocks_movement, _, _, _, _, _, _)| {
                                        blocks_movement.is_none()
                                    },
                                )
                            })
                        {
                            if let Some(actor_entity) = stable_index.actor(action.actor) {
                                commands.entity(actor_entity).insert(GridPosition {
                                    level: actor_position.level,
                                    cell: destination.cell,
                                });
                            }
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
                attacker_is_controlled,
                _,
                _,
                _,
                attacker_is_hostile,
                attacker_stats,
            )) = actor_flags(&stable_index, &positions, action.actor)
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
                target_is_controlled,
                target_is_hostile_actor,
                target_health,
                _,
                target_is_hostile,
                target_stats,
            )) = target_flags(&stable_index, &positions, target)
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
                    attacker_is_controlled,
                    attacker_is_hostile,
                    target_is_controlled,
                    target_is_hostile_actor,
                    target_is_hostile,
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
            let Some(actor_entity) = stable_index.actor(action.actor) else {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::MissingItem,
                });
                return;
            };
            let Some(item_entity) = stable_index.item(item) else {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::MissingItem,
                });
                return;
            };
            let Some(mut inventory) = inventories.get_mut(actor_entity).ok() else {
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
            commands.entity(item_entity).insert(CarriedBy(action.actor));
            if should_reschedule {
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
            }
        }
        ActionKind::Drop { item } => {
            let Some(actor_entity) = stable_index.actor(action.actor) else {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::MissingItem,
                });
                return;
            };
            let Some(item_entity) = stable_index.item(item) else {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::MissingItem,
                });
                return;
            };
            let Some(mut inventory) = inventories.get_mut(actor_entity).ok() else {
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
            if let Some((_, actor_position, _, _, _, _, _, _, _, _, _, _)) =
                positions.get(actor_entity).ok()
            {
                commands.entity(item_entity).insert(*actor_position);
            }
            commands.entity(item_entity).remove::<CarriedBy>();
            if should_reschedule {
                schedule_actor(&mut turn_clock, action.actor, &action.kind, actor_speed);
            }
        }
        ActionKind::UseItem { item, target } => {
            let Some(actor_entity) = stable_index.actor(action.actor) else {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::MissingItem,
                });
                return;
            };
            let Some(item_entity) = stable_index.item(item) else {
                outcomes.push(ActionOutcome::Failed {
                    action: action.clone(),
                    failure: ActionFailure::MissingItem,
                });
                return;
            };
            let Some(mut inventory) = inventories.get_mut(actor_entity).ok() else {
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
            let item_prototype = positions.get(item_entity).ok().and_then(
                |(_, _, _, _, _, _, _, _, _, _, prototype, _)| {
                    prototype.as_ref().map(|prototype| prototype.0.clone())
                },
            );
            inventory.items.remove(index);
            commands.entity(item_entity).despawn();

            let use_target_is_self = match target {
                ActionTarget::SelfTarget => true,
                ActionTarget::Actor(entity) => entity == action.actor,
                _ => false,
            };
            if use_target_is_self {
                let amount = content
                    .as_ref()
                    .and_then(|registry| {
                        item_prototype.as_ref().and_then(|prototype| {
                            registry.items.get(prototype).and_then(|definition| {
                                definition.use_effect.as_ref().map(|effect| match effect {
                                    ItemUseEffect::Heal { amount } => *amount,
                                })
                            })
                        })
                    })
                    .unwrap_or(1);
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
    stable_index: &StableEntityIndex,
    positions: &Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&ControlledActor>,
            Option<&HostileActor>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::Hostile>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
            Option<&StableActorId>,
        ),
    >,
    target: ActorId,
) -> Option<(
    GridPosition,
    bool,
    bool,
    Option<Health>,
    bool,
    bool,
    Option<CombatStats>,
)> {
    let entity = stable_index.actor(target)?;
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
            _stable_actor_id,
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

fn schedule_actor(turn_clock: &mut TurnClock, actor: ActorId, kind: &ActionKind, actor_speed: u64) {
    turn_clock.schedule_after(actor, action_ticks(kind, actor_speed));
}

fn schedule_actor_with_base(
    turn_clock: &mut TurnClock,
    actor: ActorId,
    base: u64,
    actor_speed: u64,
) {
    turn_clock.schedule_after(actor, action_ticks_from_base(base, actor_speed));
}

fn validate_action_kind(
    action: &Action,
    map: &LevelMap,
    spatial: &SpatialIndex,
    attacker_is_controlled: bool,
    attacker_is_hostile: bool,
    stable_index: &StableEntityIndex,
    positions: &Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&ControlledActor>,
            Option<&HostileActor>,
            Option<&Health>,
            Option<&BlocksMovement>,
            Option<&crate::actor::components::Hostile>,
            Option<&CombatStats>,
            Option<&Item>,
            Option<&CarriedBy>,
            Option<&PrototypeId>,
            Option<&StableActorId>,
        ),
    >,
    inventories: &Query<'_, '_, &Inventory>,
) -> Result<(), ActionFailure> {
    match &action.kind {
        ActionKind::Wait => Ok(()),
        ActionKind::Move { delta } => {
            let Some((actor_position, _, _, _, _, _, _)) =
                actor_flags(stable_index, positions, action.actor)
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
                stable_index,
                action.actor,
                destination,
                attacker_is_controlled,
                attacker_is_hostile,
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
                        |(_, _, _, _, _, blocks_movement, _, _, _, _, _, _)| {
                            blocks_movement.is_some()
                        },
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
                actor_flags(stable_index, positions, action.actor)
            else {
                return Err(ActionFailure::ActorUnavailable);
            };
            let Some((
                target_position,
                target_is_controlled,
                target_is_hostile_actor,
                target_health,
                _,
                target_is_hostile,
                target_stats,
            )) = target_flags(stable_index, positions, *target)
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
                attacker_is_controlled,
                attacker_is_hostile,
                target_is_controlled,
                target_is_hostile_actor,
                target_is_hostile,
            ) {
                return Err(ActionFailure::InvalidTarget);
            }

            Ok(())
        }
        ActionKind::PickUp { item } => {
            let Some(actor_entity) = stable_index.actor(action.actor) else {
                return Err(ActionFailure::ActorUnavailable);
            };
            let Some(item_entity) = stable_index.item(*item) else {
                return Err(ActionFailure::InvalidTarget);
            };
            let Some((_, actor_position, _, _, _, _, _, _, _, _, _, _)) =
                positions.get(actor_entity).ok()
            else {
                return Err(ActionFailure::ActorUnavailable);
            };
            let Some((_, item_position, _, _, _, _, _, _, Some(_), carried_by, _, _)) =
                positions.get(item_entity).ok()
            else {
                return Err(ActionFailure::InvalidTarget);
            };

            if carried_by.is_some() {
                return Err(ActionFailure::MissingItem);
            }

            let Some(inventory) = inventories.get(actor_entity).ok() else {
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
            let Some(actor_entity) = stable_index.actor(action.actor) else {
                return Err(ActionFailure::ActorUnavailable);
            };
            let Some(item_entity) = stable_index.item(*item) else {
                return Err(ActionFailure::InvalidTarget);
            };
            let Some((_, actor_position, _, _, _, _, _, _, _, _, _, _)) =
                positions.get(actor_entity).ok()
            else {
                return Err(ActionFailure::ActorUnavailable);
            };
            let Some((_, item_position, _, _, _, _, _, _, Some(_), carried_by, _, _)) =
                positions.get(item_entity).ok()
            else {
                return Err(ActionFailure::InvalidTarget);
            };

            if carried_by.map(|carried| carried.0) != Some(action.actor) {
                return Err(ActionFailure::MissingItem);
            }
            if inventories.get(actor_entity).is_err() {
                return Err(ActionFailure::MissingItem);
            }
            if actor_position.level != item_position.level {
                return Err(ActionFailure::InvalidTarget);
            }
            Ok(())
        }
        ActionKind::UseItem { item, target } => {
            let Some(actor_entity) = stable_index.actor(action.actor) else {
                return Err(ActionFailure::ActorUnavailable);
            };
            let Some(item_entity) = stable_index.item(*item) else {
                return Err(ActionFailure::InvalidTarget);
            };
            let Some((_, _, _, _, _, _, _, _, _, _, _, _)) = positions.get(actor_entity).ok()
            else {
                return Err(ActionFailure::ActorUnavailable);
            };
            let Some((_, _, _, _, _, _, _, _, Some(_), carried_by, _, _)) =
                positions.get(item_entity).ok()
            else {
                return Err(ActionFailure::InvalidTarget);
            };
            if inventories.get(actor_entity).is_err() {
                return Err(ActionFailure::MissingItem);
            }
            if carried_by.map(|carried| carried.0) != Some(action.actor) {
                return Err(ActionFailure::MissingItem);
            }
            match target {
                ActionTarget::SelfTarget => Ok(()),
                ActionTarget::Actor(entity) if *entity == action.actor => Ok(()),
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
        ActionTarget::Actor(_) => None,
        ActionTarget::Cell { level, .. } => Some(level),
    }
}

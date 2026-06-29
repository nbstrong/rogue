use bevy_ecs::prelude::*;

use crate::action::intent::{ActionKind, ActionTarget};
use crate::action::queue::ActionQueue;
use crate::actor::combat::DamageKind;
use crate::actor::components::{BlocksMovement, CombatStats, Health, Monster, Player};
use crate::item::effects::{Effect, EffectQueue};
use crate::time::clock::TurnClock;
use crate::world::map::{LevelId, LevelMap};
use crate::world::spatial::SpatialIndex;
use crate::world::map::GridPosition;
use crate::{actor::components::ActionSpeed, world::tile::TileKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionFailure {
    Blocked,
    InvalidTarget,
    OutOfRange,
    MissingItem,
    InventoryFull,
    ActorUnavailable,
}

pub fn validate_action(
    queue: Res<'_, ActionQueue>,
    current_actor: Option<Res<'_, crate::time::clock::CurrentActor>>,
) {
    let Some(current_actor) = current_actor else {
        return;
    };
    let Some(current_actor) = current_actor.0 else {
        return;
    };

    let _valid = queue
        .actions
        .front()
        .is_some_and(|action| action.actor == current_actor);
}

pub fn resolve_action(
    mut commands: Commands<'_, '_>,
    mut queue: ResMut<'_, ActionQueue>,
    mut effects: ResMut<'_, EffectQueue>,
    mut map: ResMut<'_, LevelMap>,
    spatial: Res<'_, SpatialIndex>,
    positions: Query<'_, '_, (Entity, &GridPosition, Option<&BlocksMovement>, Option<&Player>, Option<&Monster>, Option<&CombatStats>, Option<&Health>)>,
    speeds: Query<'_, '_, &ActionSpeed>,
    current_actor: Option<Res<'_, crate::time::clock::CurrentActor>>,
    mut turn_clock: ResMut<'_, TurnClock>,
) {
    let Some(action) = queue.pop() else {
        return;
    };

    let Some(current_actor) = current_actor else {
        return;
    };
    let Some(current_actor) = current_actor.0 else {
        return;
    };

    if action.actor != current_actor {
        return;
    }

    let actor_speed = speeds
        .get(action.actor)
        .map(|speed| speed.ticks_per_action)
        .unwrap_or(100);

    match action.kind.clone() {
        ActionKind::Wait => {
            turn_clock.schedule_after(action.actor, actor_speed);
        }
        ActionKind::Move { delta } => {
            let Ok((_, position, _, _, _, _, _)) = positions.get(action.actor) else {
                return;
            };

            let destination = position.cell + delta;
            if !map.in_bounds(destination) {
                turn_clock.schedule_after(action.actor, actor_speed);
                return;
            }

            let tile = map.tile(destination).cloned();
            match tile.map(|tile| tile.kind) {
                Some(TileKind::Wall) => {
                    turn_clock.schedule_after(action.actor, actor_speed);
                    return;
                }
                Some(TileKind::ClosedDoor) => {
                    if let Some(tile) = map.tile_mut(destination) {
                        tile.kind = TileKind::OpenDoor;
                    }
                    turn_clock.schedule_after(action.actor, 80);
                }
                Some(TileKind::Floor) | Some(TileKind::OpenDoor) | Some(TileKind::StairsDown) | Some(TileKind::StairsUp) => {
                    if spatial
                        .occupants_at(position.level, destination)
                        .find(|occupant| occupant != &action.actor)
                        .is_some()
                    {
                        if let Some(target) = spatial
                            .occupants_at(position.level, destination)
                            .find(|occupant| occupant != &action.actor)
                        {
                            effects.0.push_back(Effect::Damage {
                                source: Some(action.actor),
                                target,
                                amount: 1,
                                kind: DamageKind::Melee,
                            });
                            turn_clock.schedule_after(action.actor, 140);
                        }
                    } else {
                        commands.entity(action.actor).insert(GridPosition {
                            level: position.level,
                            cell: destination,
                        });
                        turn_clock.schedule_after(action.actor, actor_speed);
                    }
                }
                None => {
                    turn_clock.schedule_after(action.actor, actor_speed);
                }
            }
        }
        ActionKind::Melee { target } => {
            effects.0.push_back(Effect::Damage {
                source: Some(action.actor),
                target,
                amount: 1,
                kind: DamageKind::Melee,
            });
            turn_clock.schedule_after(action.actor, 140);
        }
        ActionKind::PickUp { .. }
        | ActionKind::Drop { .. }
        | ActionKind::UseItem { .. }
        | ActionKind::Descend
        | ActionKind::Ascend => {
            turn_clock.schedule_after(action.actor, actor_speed);
        }
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

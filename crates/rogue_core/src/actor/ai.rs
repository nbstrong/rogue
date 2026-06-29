use bevy_ecs::prelude::*;
use bevy_math::IVec2;

use crate::action::intent::{Action, ActionKind};
use crate::action::queue::ActionQueue;
use crate::actor::components::{HostileToPlayer, Monster, Player};
use crate::time::clock::CurrentActor;
use crate::world::map::GridPosition;
use crate::world::spatial::SpatialIndex;

fn step_toward(from: IVec2, to: IVec2) -> IVec2 {
    IVec2::new((to.x - from.x).signum(), (to.y - from.y).signum())
}

pub fn generate_ai_action(
    mut queue: ResMut<'_, ActionQueue>,
    monsters: Query<
        '_,
        '_,
        (Entity, &GridPosition),
        (With<Monster>, With<HostileToPlayer>, Without<Player>),
    >,
    players: Query<'_, '_, (Entity, &GridPosition), With<Player>>,
    spatial: Res<'_, SpatialIndex>,
    current_actor: Option<Res<'_, CurrentActor>>,
) {
    let Some(current_actor) = current_actor else {
        return;
    };
    let Some(current_actor_entity) = current_actor.0 else {
        return;
    };
    let Some((player_entity, player_position)) = players.iter().next() else {
        return;
    };

    for (entity, position) in monsters.iter() {
        if entity != current_actor_entity {
            continue;
        }

        let delta = player_position.cell - position.cell;
        if delta.x.abs().max(delta.y.abs()) == 1 {
            queue.push(Action {
                actor: entity,
                kind: ActionKind::Melee {
                    target: player_entity,
                },
            });
        } else {
            let movement = step_toward(position.cell, player_position.cell);
            if movement != IVec2::ZERO {
                let destination = position.cell + movement;
                if spatial
                    .occupants_at(position.level, destination)
                    .find(|occupant| *occupant != entity)
                    .is_none()
                {
                    queue.push(Action {
                        actor: entity,
                        kind: ActionKind::Move { delta: movement },
                    });
                } else {
                    queue.push(Action {
                        actor: entity,
                        kind: ActionKind::Wait,
                    });
                }
            }
        }
    }
}

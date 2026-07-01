use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use tactical_sim::action::intent::{Action, ActionKind};
use tactical_sim::action::queue::ActionQueue;
use tactical_sim::actor::components::{
    ControlledActor, Hostile, HostileActor, StableActorId, StableEntityIndex,
};
use tactical_sim::persistence::rng::RandomStreams;
use tactical_sim::time::clock::CurrentActor;
use tactical_sim::world::map::GridPosition;
use tactical_sim::world::spatial::SpatialIndex;

fn step_toward(from: IVec2, to: IVec2) -> IVec2 {
    IVec2::new((to.x - from.x).signum(), (to.y - from.y).signum())
}

pub fn generate_ai_action(
    mut queue: ResMut<'_, ActionQueue>,
    monsters: Query<
        '_,
        '_,
        (&GridPosition, &StableActorId),
        (With<HostileActor>, With<Hostile>, Without<ControlledActor>),
    >,
    controlled_actors: Query<'_, '_, (&GridPosition, &StableActorId), With<ControlledActor>>,
    spatial: Res<'_, SpatialIndex>,
    stable_index: Res<'_, StableEntityIndex>,
    current_actor: Option<Res<'_, CurrentActor>>,
    mut rng: ResMut<'_, RandomStreams>,
) {
    let Some(current_actor) = current_actor else {
        return;
    };
    let Some(current_actor_id) = current_actor.0 else {
        return;
    };
    if queue.contains_actor(current_actor_id) {
        return;
    }
    let Some((controlled_position, controlled_stable_id)) = controlled_actors.iter().next() else {
        return;
    };
    let Some(_controlled_entity) = stable_index.actor(controlled_stable_id.0) else {
        return;
    };

    for (position, stable_id) in monsters.iter() {
        if stable_id.0 != current_actor_id {
            continue;
        }
        let Some(entity) = stable_index.actor(stable_id.0) else {
            continue;
        };

        let roll = rng.next_ai_u64();
        let delta = controlled_position.cell - position.cell;
        if delta.x.abs().max(delta.y.abs()) == 1 {
            queue.push(Action {
                actor: stable_id.0,
                kind: ActionKind::Melee {
                    target: controlled_stable_id.0,
                },
            });
        } else {
            let movement = step_toward(position.cell, controlled_position.cell);
            if movement != IVec2::ZERO {
                if roll & 1 == 0 {
                    let destination = position.cell + movement;
                    if spatial
                        .occupants_at(position.level, destination)
                        .find(|occupant| *occupant != entity)
                        .is_none()
                    {
                        queue.push(Action {
                            actor: stable_id.0,
                            kind: ActionKind::Move { delta: movement },
                        });
                    } else {
                        queue.push(Action {
                            actor: stable_id.0,
                            kind: ActionKind::Wait,
                        });
                    }
                } else {
                    queue.push(Action {
                        actor: stable_id.0,
                        kind: ActionKind::Wait,
                    });
                }
            }
        }
    }
}

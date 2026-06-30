use bevy_ecs::prelude::*;

use crate::action::queue::ActionQueue;
use crate::actor::components::{Player, StableActorId, StableEntityIndex};
use crate::simulation::SimulationStatus;
use crate::time::clock::{CurrentActor, TurnClock};

pub fn select_next_actor(
    mut status: ResMut<'_, SimulationStatus>,
    mut clock: ResMut<'_, TurnClock>,
    mut current_actor: ResMut<'_, CurrentActor>,
    stable_index: Res<'_, StableEntityIndex>,
    actors: Query<'_, '_, (&crate::actor::components::Health, &StableActorId)>,
) {
    if current_actor.0.is_some() {
        return;
    }

    while let Some(next) = clock.pop_next() {
        let Some(entity) = stable_index.actor(next.actor) else {
            continue;
        };
        if actors
            .get(entity)
            .is_ok_and(|(health, stable_id)| health.current > 0 && stable_id.0 == next.actor)
        {
            clock.current_tick = next.next_tick;
            current_actor.0 = Some(next.actor);
            *status = SimulationStatus::Resolving;
            return;
        }
    }
}

pub fn finish_simulation_step(
    mut current_actor: ResMut<'_, CurrentActor>,
    clock: Res<'_, TurnClock>,
    queue: Res<'_, ActionQueue>,
    stable_index: Res<'_, StableEntityIndex>,
    player: Query<'_, '_, &StableActorId, With<Player>>,
    mut status: ResMut<'_, SimulationStatus>,
) {
    if *status == SimulationStatus::WaitingForPlayer {
        return;
    }

    current_actor.0 = None;
    if *status == SimulationStatus::GameOver {
        return;
    }
    if let Some(next) = clock.peek_next() {
        let player_is_next = player
            .iter()
            .next()
            .is_some_and(|stable_id| stable_id.0 == next.actor);
        if player_is_next {
            if queue.contains_actor(next.actor) {
                *status = SimulationStatus::Resolving;
                return;
            }
            *status = SimulationStatus::WaitingForPlayer;
            return;
        }
        if stable_index.actor(next.actor).is_none() {
            *status = SimulationStatus::WaitingForPlayer;
            return;
        }
        *status = SimulationStatus::Resolving;
        return;
    }

    *status = SimulationStatus::WaitingForPlayer;
}

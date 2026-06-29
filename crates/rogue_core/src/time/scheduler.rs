use bevy_ecs::prelude::*;

use crate::actor::components::Player;
use crate::simulation::SimulationStatus;
use crate::time::clock::{CurrentActor, TurnClock};

pub fn select_next_actor(
    mut status: ResMut<'_, SimulationStatus>,
    mut clock: ResMut<'_, TurnClock>,
    mut current_actor: ResMut<'_, CurrentActor>,
    actors: Query<'_, '_, &crate::actor::components::Health>,
) {
    if current_actor.0.is_some() {
        return;
    }

    if let Some(next) = clock.pop_next() {
        if actors
            .get(next.actor)
            .is_ok_and(|health| health.current > 0)
        {
            clock.current_tick = next.next_tick;
            current_actor.0 = Some(next.actor);
            *status = SimulationStatus::Resolving;
            return;
        }

        debug_assert!(
            false,
            "discarded stale scheduled actor that no longer exists or can not act"
        );
        while let Some(next) = clock.pop_next() {
            if actors
                .get(next.actor)
                .is_ok_and(|health| health.current > 0)
            {
                clock.current_tick = next.next_tick;
                current_actor.0 = Some(next.actor);
                *status = SimulationStatus::Resolving;
                return;
            }
        }
    }
}

pub fn finish_simulation_step(
    mut current_actor: ResMut<'_, CurrentActor>,
    clock: Res<'_, TurnClock>,
    player: Query<'_, '_, Entity, With<Player>>,
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
        if player.get(next.actor).is_ok() {
            *status = SimulationStatus::WaitingForPlayer;
            return;
        }
        *status = SimulationStatus::Resolving;
        return;
    }

    *status = SimulationStatus::WaitingForPlayer;
}

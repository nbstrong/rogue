use bevy_ecs::prelude::*;

use crate::actor::components::Player;
use crate::simulation::SimulationStatus;
use crate::time::clock::{CurrentActor, TurnClock};

pub fn select_next_actor(
    mut clock: ResMut<'_, TurnClock>,
    mut current_actor: ResMut<'_, CurrentActor>,
) {
    if let Some(next) = clock.pop_next() {
        clock.current_tick = next.next_tick;
        current_actor.0 = Some(next.actor);
    }
}

pub fn finish_simulation_step(
    mut current_actor: ResMut<'_, CurrentActor>,
    clock: Res<'_, TurnClock>,
    player: Query<'_, '_, Entity, With<Player>>,
    mut status: ResMut<'_, SimulationStatus>,
) {
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

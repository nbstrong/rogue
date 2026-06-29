use bevy_ecs::prelude::*;

use crate::time::clock::{CurrentActor, TurnClock};

pub fn select_next_actor(mut clock: ResMut<'_, TurnClock>, mut current_actor: ResMut<'_, CurrentActor>) {
    if let Some(next) = clock.pop_next() {
        clock.current_tick = next.next_tick;
        current_actor.0 = Some(next.actor);
    }
}

pub fn finish_simulation_step(mut current_actor: ResMut<'_, CurrentActor>) {
    current_actor.0 = None;
}

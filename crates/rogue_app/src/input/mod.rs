use bevy::prelude::*;
use bevy::state::condition::in_state;

pub mod keyboard;
pub mod mapping;
use crate::app_state::AppState;

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                keyboard::capture_keyboard_input.run_if(in_state(AppState::Playing)),
                keyboard::restart_from_game_over.run_if(in_state(AppState::GameOver)),
            ),
        );
    }
}

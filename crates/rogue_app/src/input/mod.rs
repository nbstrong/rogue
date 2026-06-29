use bevy::prelude::*;

pub mod keyboard;
pub mod mapping;

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, keyboard::capture_keyboard_input);
    }
}


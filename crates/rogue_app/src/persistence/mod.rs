use bevy::prelude::*;

pub mod files;

pub use files::{SaveFileLocation, load_game, load_world_from_path, save_game, save_world_to_path};

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, files::bootstrap_save_system);
    }
}

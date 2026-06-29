use bevy::prelude::*;

pub mod files;

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, files::bootstrap_save_system);
    }
}

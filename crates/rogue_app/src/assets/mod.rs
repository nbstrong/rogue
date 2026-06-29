use bevy::prelude::*;

pub mod loading;

pub struct AssetPlugin;

impl Plugin for AssetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, loading::load_content);
    }
}


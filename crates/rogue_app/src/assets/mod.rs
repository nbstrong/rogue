use bevy::prelude::*;

pub mod loading;

pub struct AssetPlugin;

impl Plugin for AssetPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(loading::load_content());
    }
}

use bevy::prelude::*;

pub mod hud;
pub mod inventory;
pub mod log;
pub mod targeting;

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (hud::update_hud, inventory::update_inventory_ui, targeting::update_targeting, log::flush_combat_log),
        );
    }
}


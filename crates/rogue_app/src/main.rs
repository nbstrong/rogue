mod app_state;
mod assets;
mod input;
mod persistence;
mod presentation;
mod ui;

use bevy::prelude::*;
use rogue_core::SimulationPlugin;

use crate::app_state::AppState;
use crate::assets::AssetPlugin;
use crate::input::InputPlugin;
use crate::persistence::SavePlugin;
use crate::presentation::PresentationPlugin;
use crate::ui::GameUiPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .init_state::<AppState>()
        .add_plugins((
            SimulationPlugin,
            AssetPlugin,
            InputPlugin,
            PresentationPlugin,
            GameUiPlugin,
            SavePlugin,
        ))
        .run();
}


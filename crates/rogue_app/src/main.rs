use bevy::prelude::*;
use rogue_core::SimulationPlugin;

use rogue_app::assets::AssetPlugin;
use rogue_app::game::GamePlugin;
use rogue_app::input::InputPlugin;
use rogue_app::persistence::SavePlugin;
use rogue_app::presentation::PresentationPlugin;
use rogue_app::ui::GameUiPlugin;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::BLACK))
        .add_plugins(DefaultPlugins)
        .init_state::<rogue_app::app_state::AppState>()
        .add_plugins((
            SimulationPlugin,
            AssetPlugin,
            GamePlugin,
            InputPlugin,
            PresentationPlugin,
            GameUiPlugin,
            SavePlugin,
        ))
        .run();
}

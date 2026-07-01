use bevy::prelude::*;
use tactical_sim::SimulationPlugin;

use bread_and_iron_app::assets::AssetPlugin;
use bread_and_iron_app::game::GamePlugin;
use bread_and_iron_app::input::InputPlugin;
use bread_and_iron_app::persistence::SavePlugin;
use bread_and_iron_app::presentation::PresentationPlugin;
use bread_and_iron_app::ui::GameUiPlugin;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::BLACK))
        .add_plugins(DefaultPlugins)
        .init_state::<bread_and_iron_app::app_state::AppState>()
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

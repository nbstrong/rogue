use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy_math::IVec2;
use rogue_app::app_state::AppState;
use rogue_app::assets::AssetPlugin;
use rogue_app::game::GamePlugin;
use rogue_app::game::{HudText, LogText};
use rogue_app::input::InputPlugin;
use rogue_app::presentation::PresentationPlugin;
use rogue_app::ui::GameUiPlugin;
use rogue_core::actor::components::Health;
use rogue_core::actor::components::Monster;
use rogue_core::actor::components::Player;
use rogue_core::simulation::{SimulationPlugin, SimulationStatus};
use rogue_core::world::map::GridPosition;
use rogue_core::world::map::LevelMap;

fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins(StatesPlugin);
    app.init_state::<AppState>();
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.add_plugins((
        SimulationPlugin,
        AssetPlugin,
        GamePlugin,
        InputPlugin,
        PresentationPlugin,
        GameUiPlugin,
    ));
    app
}

#[test]
fn app_boots_into_playing_and_drives_a_turn_without_a_window() {
    let mut app = build_app();

    app.update();
    app.update();
    app.update();
    app.update();

    let map = app.world().resource::<LevelMap>();
    let map_views = app.world().resource::<rogue_app::game::MapViews>();

    assert_eq!(
        map_views.tiles.len(),
        map.width as usize * map.height as usize,
        "every map tile should have a presentation entity"
    );

    let actor_views = app.world().resource::<rogue_app::game::ActorViews>();
    assert!(
        actor_views.views.len() >= 2,
        "the player and monster should both have presentation entities"
    );

    let camera_count = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Camera2d>>()
            .iter(world)
            .count()
    };

    assert_eq!(
        camera_count, 1,
        "the game should have exactly one 2D camera"
    );

    assert_eq!(
        *app.world().resource::<State<AppState>>(),
        AppState::Playing
    );
    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );

    let player_position = {
        let world = app.world_mut();
        world
            .query_filtered::<&GridPosition, With<Player>>()
            .iter(world)
            .next()
            .copied()
            .expect("player position")
    };
    let player_tile = app
        .world()
        .resource::<LevelMap>()
        .tile(player_position.cell)
        .expect("player tile");
    assert!(player_tile.visible);
    assert!(player_tile.explored);

    let before_player = {
        let world = app.world_mut();
        world
            .query_filtered::<&GridPosition, With<Player>>()
            .iter(world)
            .next()
            .copied()
            .expect("player position")
    };

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Numpad6);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Numpad6);

    let after_player = {
        let world = app.world_mut();
        world
            .query_filtered::<&GridPosition, With<Player>>()
            .iter(world)
            .next()
            .copied()
            .expect("player position")
    };
    let after_monster = {
        let world = app.world_mut();
        world
            .query_filtered::<&GridPosition, With<rogue_core::actor::components::Monster>>()
            .iter(world)
            .next()
            .copied()
            .expect("monster position")
    };

    assert_eq!(after_player.cell, before_player.cell + IVec2::new(1, 0));
    assert_eq!(after_monster.cell, IVec2::new(7, 7));
    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );
}

#[test]
fn presentation_plugin_hides_monsters_outside_the_player_fov() {
    let mut app = build_app();

    app.update();
    app.update();
    app.update();
    app.update();

    let monster = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Monster>>()
            .iter(world)
            .next()
            .expect("monster entity")
    };

    app.world_mut().entity_mut(monster).insert(GridPosition {
        level: rogue_core::world::map::LevelId(0),
        cell: IVec2::new(20, 13),
    });

    app.update();

    let actor_views = app.world().resource::<rogue_app::game::ActorViews>();
    let view_entity = actor_views
        .views
        .get(&monster)
        .copied()
        .expect("monster view entity");
    let visibility = app
        .world()
        .entity(view_entity)
        .get::<Visibility>()
        .copied()
        .expect("monster visibility");

    assert_eq!(visibility, Visibility::Hidden);

    let player_position = {
        let world = app.world_mut();
        world
            .query_filtered::<&GridPosition, With<Player>>()
            .iter(world)
            .next()
            .copied()
            .expect("player position")
    };
    let player_tile = app
        .world()
        .resource::<LevelMap>()
        .tile(player_position.cell)
        .expect("player tile");
    assert!(player_tile.visible);
    assert!(player_tile.explored);
}

#[test]
fn player_death_enters_game_over_and_restart_rebuilds_the_world() {
    let mut app = build_app();

    app.update();
    app.update();

    let player = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Player>>()
            .iter(world)
            .next()
            .expect("player entity")
    };

    app.world_mut().entity_mut(player).insert(Health {
        current: 0,
        maximum: 10,
    });
    app.world_mut()
        .resource_mut::<rogue_core::time::clock::TurnClock>()
        .schedule_at(player, 0);
    app.world_mut()
        .resource_mut::<rogue_core::action::queue::ActionQueue>()
        .push(rogue_core::action::intent::Action {
            actor: player,
            kind: rogue_core::action::intent::ActionKind::Wait,
        });
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;

    app.update();
    app.update();

    assert_eq!(
        *app.world().resource::<State<AppState>>(),
        AppState::GameOver
    );

    let game_over_text = {
        let world = app.world_mut();
        world
            .query_filtered::<&Text, With<LogText>>()
            .iter(world)
            .next()
            .map(|text| text.as_str().to_string())
            .expect("game over text")
    };
    assert!(game_over_text.contains("Game over"));

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyR);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::KeyR);
    app.update();
    app.update();

    assert_eq!(
        *app.world().resource::<State<AppState>>(),
        AppState::Playing
    );
    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );

    let hud_text = {
        let world = app.world_mut();
        world
            .query_filtered::<&Text, With<HudText>>()
            .iter(world)
            .next()
            .map(|text| text.as_str().to_string())
            .expect("hud text")
    };
    assert!(hud_text.contains("Move with"));
}

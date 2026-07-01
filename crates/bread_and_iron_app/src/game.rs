use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;
use bevy_math::IVec2;
use bread_and_iron::Player;
use bread_and_iron::generate_ai_action;
use tactical_sim::actor::components::Health;
use tactical_sim::actor::components::PersistentIdAllocator;
use tactical_sim::content::registry::ContentRegistry;
use tactical_sim::persistence::rng::RandomStreams;
use tactical_sim::simulation::SimulationDriverState;
use tactical_sim::simulation::SimulationStatus;
use tactical_sim::simulation::{SimulationSet, SimulationStep};
use tactical_sim::world::map::LevelId;

use crate::app_state::{AppState, CurrentInputMode};

pub const TILE_SIZE: f32 = 24.0;

#[derive(Component)]
pub struct SessionEntity;

#[derive(Component)]
pub struct MapTileView;

#[derive(Component)]
pub struct ActorView;

#[derive(Component)]
pub struct HudText;

#[derive(Component)]
pub struct LogText;

#[derive(Resource, Default)]
pub struct MapViews {
    pub tiles: HashMap<(LevelId, IVec2), Entity>,
}

#[derive(Resource, Default)]
pub struct ActorViews {
    pub views: HashMap<Entity, Entity>,
}

#[derive(Resource, Default)]
pub struct HealthSnapshot {
    pub values: HashMap<Entity, i32>,
}

#[derive(Resource, Default)]
pub struct CombatLog {
    pub lines: VecDeque<String>,
}

#[derive(Resource, Default)]
pub struct GameRootState {
    pub initialized: bool,
}

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MapViews>()
            .init_resource::<ActorViews>()
            .init_resource::<HealthSnapshot>()
            .init_resource::<CombatLog>()
            .init_resource::<GameRootState>()
            .init_resource::<CurrentInputMode>()
            .init_resource::<PersistentIdAllocator>()
            .init_resource::<RandomStreams>()
            .add_systems(
                SimulationStep,
                generate_ai_action.in_set(SimulationSet::DecideAction),
            )
            .add_systems(Startup, bootstrap_game)
            .add_systems(
                Update,
                (drive_simulation_if_resolving,)
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(OnEnter(AppState::GameOver), show_game_over_message);
    }
}

fn bootstrap_game(world: &mut World) {
    if !world.contains_resource::<ContentRegistry>() {
        world.insert_resource(ContentRegistry::default());
    }

    spawn_camera_if_needed(world);
    setup_new_game(world, true);
    if let Some(mut next_state) = world.get_resource_mut::<NextState<AppState>>() {
        next_state.set(AppState::Playing);
    }
    world.insert_resource(GameRootState { initialized: true });
}

fn spawn_camera_if_needed(world: &mut World) {
    let has_camera = world
        .query_filtered::<Entity, With<Camera2d>>()
        .iter(world)
        .next()
        .is_some();
    if !has_camera {
        world.spawn((Camera2d, Transform::default(), SessionEntity));
    }
}

pub fn setup_new_game(world: &mut World, clear_existing: bool) {
    bread_and_iron::setup_new_game(world, clear_existing);
}

fn drive_simulation_if_resolving(world: &mut World) {
    let should_drive = world.resource::<SimulationStatus>() == &SimulationStatus::Resolving
        || world
            .resource::<SimulationDriverState>()
            .has_active_domain_request();

    if should_drive {
        tactical_sim::drive_simulation(world);
    }

    let player_alive = world
        .query_filtered::<&Health, With<Player>>()
        .iter(world)
        .any(|health| health.current > 0);
    if !player_alive {
        if let Some(mut simulation) = world.get_resource_mut::<SimulationStatus>() {
            *simulation = SimulationStatus::GameOver;
        }
    }

    if world.resource::<SimulationStatus>() == &SimulationStatus::GameOver {
        let state_is_game_over = world
            .get_resource::<State<AppState>>()
            .is_some_and(|state| state.get() == &AppState::GameOver);
        if !state_is_game_over {
            if let Some(mut next_state) = world.get_resource_mut::<NextState<AppState>>() {
                next_state.set(AppState::GameOver);
            }
        }
    }
}

fn show_game_over_message(
    mut hud: Query<'_, '_, &mut Text, (With<HudText>, Without<LogText>)>,
    mut log: Query<'_, '_, &mut Text, (With<LogText>, Without<HudText>)>,
) {
    let message = "Game over. Press R to restart.";

    if let Some(mut text) = hud.iter_mut().next() {
        *text = Text::new(message);
    }

    if let Some(mut text) = log.iter_mut().next() {
        *text = Text::new(message);
    }
}

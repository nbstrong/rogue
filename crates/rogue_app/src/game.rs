use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;
use bevy::text::{TextColor, TextFont};
use bevy::ui::{Node, PositionType, Val};
use bevy_math::IVec2;
use rogue_core::action::queue::ActionQueue;
use rogue_core::actor::components::{
    ActionSpeed, Actor, BlocksMovement, BlocksSight, CombatStats, Health, HostileToPlayer,
    Monster, PersistentId, Player, PrototypeId, Vision,
};
use rogue_core::content::definitions::ActorDefinition;
use rogue_core::content::registry::ContentRegistry;
use rogue_core::item::effects::EffectQueue;
use rogue_core::simulation::SimulationStatus;
use rogue_core::time::clock::TurnClock;
use rogue_core::world::generation::generate_one_room;
use rogue_core::world::map::{GridPosition, LevelId, LevelMap};
use rogue_core::world::spatial::SpatialIndex;

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

#[derive(Resource, Default)]
pub struct PersistentIdCounter(pub u64);

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MapViews>()
            .init_resource::<ActorViews>()
            .init_resource::<HealthSnapshot>()
            .init_resource::<CombatLog>()
            .init_resource::<GameRootState>()
            .init_resource::<PersistentIdCounter>()
            .init_resource::<CurrentInputMode>()
            .add_systems(Startup, bootstrap_game)
            .add_systems(
                Update,
                (
                    drive_simulation_if_resolving,
                    sync_game_state,
                )
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
    let has_camera = world.query_filtered::<Entity, With<Camera2d>>().iter(world).next().is_some();
    if !has_camera {
        world.spawn((Camera2d, Transform::default(), SessionEntity));
    }
}

pub fn setup_new_game(world: &mut World, clear_existing: bool) {
    if clear_existing {
        let session_entities: Vec<Entity> = world
            .query_filtered::<Entity, With<SessionEntity>>()
            .iter(world)
            .collect();
        for entity in session_entities {
            let _ = world.despawn(entity);
        }
    }

    world.remove_resource::<LevelMap>();
    world.remove_resource::<SpatialIndex>();
    world.remove_resource::<ActionQueue>();
    world.remove_resource::<EffectQueue>();
    world.remove_resource::<TurnClock>();
    world.remove_resource::<SimulationStatus>();

    let player_def = world
        .resource::<ContentRegistry>()
        .actors
        .get("player")
        .cloned()
        .unwrap_or_else(|| panic!("missing player definition"));
    let monster_def = world
        .resource::<ContentRegistry>()
        .actors
        .get("ogre")
        .cloned()
        .unwrap_or_else(|| panic!("missing ogre definition"));

    let level = LevelId(0);
    let map = generate_one_room(21, 15);
    let player_cell = IVec2::new(3, 7);
    let monster_cell = IVec2::new(8, 7);

    let player = spawn_actor(world, &player_def, level, player_cell, true, false);
    let monster = spawn_actor(world, &monster_def, level, monster_cell, false, true);

    let mut spatial = SpatialIndex::default();
    insert_occupant(&mut spatial, level, player_cell, player, true, true);
    insert_occupant(&mut spatial, level, monster_cell, monster, true, true);

    let mut clock = TurnClock::default();
    clock.schedule_at(player, 0);
    clock.schedule_at(monster, 0);

    world.insert_resource(map);
    world.insert_resource(spatial);
    world.insert_resource(ActionQueue::default());
    world.insert_resource(EffectQueue::default());
    world.insert_resource(clock);
    world.insert_resource(SimulationStatus::WaitingForPlayer);
    world.insert_resource(CurrentInputMode::default());

    spawn_camera_if_needed(world);

    if let Some(mut view_cache) = world.get_resource_mut::<MapViews>() {
        view_cache.tiles.clear();
    }
    if let Some(mut actor_views) = world.get_resource_mut::<ActorViews>() {
        actor_views.views.clear();
    }
    if let Some(mut log) = world.get_resource_mut::<CombatLog>() {
        log.lines.clear();
    }
    if let Some(mut snapshot) = world.get_resource_mut::<HealthSnapshot>() {
        snapshot.values.clear();
    }
}

fn spawn_actor(
    world: &mut World,
    definition: &ActorDefinition,
    level: LevelId,
    cell: IVec2,
    is_player: bool,
    hostile: bool,
) -> Entity {
    let persistent_id = next_persistent_id(world);
    let mut entity = world.spawn((
        Actor,
        BlocksMovement,
        BlocksSight,
        Health {
            current: definition.maximum_health,
            maximum: definition.maximum_health,
        },
        CombatStats {
            power: definition.power,
            defense: definition.defense,
        },
        Vision {
            range: definition.vision_range,
        },
        ActionSpeed {
            ticks_per_action: definition.action_speed,
        },
        PrototypeId(definition.id.clone()),
        GridPosition { level, cell },
        PersistentId(persistent_id),
        SessionEntity,
    ));

    if is_player {
        entity.insert(Player);
    }
    if hostile {
        entity.insert((Monster, HostileToPlayer));
    }

    entity.id()
}

fn next_persistent_id(world: &mut World) -> u64 {
    let mut counter = world
        .get_resource_mut::<PersistentIdCounter>()
        .expect("persistent id counter");
    counter.0 += 1;
    counter.0
}

fn insert_occupant(
    spatial: &mut SpatialIndex,
    level: LevelId,
    cell: IVec2,
    entity: Entity,
    blocks_movement: bool,
    blocks_sight: bool,
) {
    let key = (level, cell);
    spatial.occupants.entry(key).or_default().push(entity);
    if blocks_movement {
        spatial.movement_blockers.insert(key);
    }
    if blocks_sight {
        spatial.sight_blockers.insert(key);
    }
}

fn drive_simulation_if_resolving(world: &mut World) {
    if world.resource::<SimulationStatus>() == &SimulationStatus::Resolving {
        rogue_core::drive_simulation(world);
    }
}

fn sync_game_state(
    simulation: Res<'_, SimulationStatus>,
    state: Res<'_, State<AppState>>,
    mut next_state: ResMut<'_, NextState<AppState>>,
) {
    if *simulation == SimulationStatus::GameOver && state.get() != &AppState::GameOver {
        next_state.set(AppState::GameOver);
    }
}

fn show_game_over_message(
    mut commands: Commands<'_, '_>,
    query: Query<'_, '_, Entity, With<LogText>>,
) {
    if query.is_empty() {
        commands.spawn((
            Text::new("Game over. Press R to restart."),
            TextFont::from_font_size(20.0),
            TextColor(Color::srgb(1.0, 0.8, 0.8)),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(16.0),
                bottom: Val::Px(16.0),
                ..default()
            },
            LogText,
        ));
    }
}

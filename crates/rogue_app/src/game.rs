use std::collections::{HashMap, HashSet, VecDeque};

use bevy::prelude::*;
use bevy_math::IVec2;
use rogue_core::ItemId;
use rogue_core::action::queue::ActionQueue;
use rogue_core::action::resolver::{ActionDecision, ActionOutcomeLog};
use rogue_core::actor::components::{
    ActionSpeed, ActiveStatuses, Actor, BlocksMovement, BlocksSight, CombatStats, Health,
    HostileToPlayer, Monster, PersistentId, PersistentIdAllocator, Player, PrototypeId,
    StableActorId, StableEntityIndex, StableItemId, Vision,
};
use rogue_core::content::definitions::ActorDefinition;
use rogue_core::content::registry::ContentRegistry;
use rogue_core::item::components::{Inventory, Item};
use rogue_core::item::effects::EffectQueue;
use rogue_core::persistence::rng::RandomStreams;
use rogue_core::simulation::SimulationStatus;
use rogue_core::time::clock::{CurrentActor, TurnClock};
use rogue_core::world::fov::recalculate_fov_for_player;
use rogue_core::world::generation::generate_one_room_with_rng;
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
    if clear_existing {
        let mut cleanup = HashSet::new();
        for entity in world
            .query_filtered::<Entity, With<SessionEntity>>()
            .iter(world)
        {
            cleanup.insert(entity);
        }
        for entity in world
            .query_filtered::<Entity, With<PersistentId>>()
            .iter(world)
        {
            cleanup.insert(entity);
        }
        for entity in cleanup {
            let _ = world.despawn(entity);
        }
    }

    world.remove_resource::<LevelMap>();
    world.remove_resource::<SpatialIndex>();
    world.remove_resource::<ActionQueue>();
    world.remove_resource::<EffectQueue>();
    world.remove_resource::<TurnClock>();
    world.remove_resource::<SimulationStatus>();
    world.remove_resource::<ActionDecision>();
    world.remove_resource::<ActionOutcomeLog>();
    world.remove_resource::<CurrentActor>();
    world.remove_resource::<StableEntityIndex>();
    world.insert_resource(RandomStreams::seeded(0));
    world.insert_resource(PersistentIdAllocator::default());
    world.insert_resource(StableEntityIndex::default());

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
    let mut map = {
        let mut rng = world.resource_mut::<RandomStreams>();
        generate_one_room_with_rng(21, 15, Some(&mut *rng))
    };
    let player_cell = IVec2::new(3, 7);
    let monster_cell = IVec2::new(8, 7);

    let player = spawn_actor(world, &player_def, level, player_cell, true, false);
    let monster = spawn_actor(world, &monster_def, level, monster_cell, false, true);

    let loot_cell = {
        let mut rng = world.resource_mut::<RandomStreams>();
        let interior_width = map.width as usize - 2;
        let interior_height = map.height as usize - 2;
        let total_cells = interior_width * interior_height;
        let mut candidate_index = (rng.next_generation_u64() as usize) % total_cells;
        let mut cell = IVec2::new(1, 1);
        for _ in 0..total_cells {
            let x = 1 + (candidate_index % interior_width) as i32;
            let y = 1 + (candidate_index / interior_width) as i32;
            cell = IVec2::new(x, y);
            if cell != player_cell && cell != monster_cell {
                break;
            }
            candidate_index = (candidate_index + 1) % total_cells;
        }
        cell
    };
    let loot_name = {
        let mut rng = world.resource_mut::<RandomStreams>();
        if rng.next_loot_u64() & 1 == 0 {
            "healing_potion"
        } else {
            "trinket"
        }
    };
    let loot = spawn_loot_item(world, level, loot_cell, loot_name);
    let player_stable_id = world.entity(player).get::<StableActorId>().copied();
    let monster_stable_id = world.entity(monster).get::<StableActorId>().copied();
    let loot_stable_id = world.entity(loot).get::<StableItemId>().copied();

    let mut spatial = SpatialIndex::default();
    insert_occupant(
        &mut spatial,
        player_stable_id.as_ref(),
        None,
        world.entity(player).get::<PersistentId>().copied(),
        level,
        player_cell,
        player,
        true,
        true,
    );
    insert_occupant(
        &mut spatial,
        monster_stable_id.as_ref(),
        None,
        world.entity(monster).get::<PersistentId>().copied(),
        level,
        monster_cell,
        monster,
        true,
        true,
    );
    insert_occupant(
        &mut spatial,
        None,
        loot_stable_id.as_ref(),
        world.entity(loot).get::<PersistentId>().copied(),
        level,
        loot_cell,
        loot,
        false,
        false,
    );

    if let Some((_, vision)) = world
        .query_filtered::<(&GridPosition, &Vision), With<Player>>()
        .iter(world)
        .next()
    {
        recalculate_fov_for_player(
            &mut map,
            &spatial,
            GridPosition {
                level,
                cell: player_cell,
            },
            vision.range,
        );
    }

    let mut clock = TurnClock::default();
    clock.schedule_at(
        world
            .entity(player)
            .get::<StableActorId>()
            .expect("stable player id")
            .0,
        0,
    );
    clock.schedule_at(
        world
            .entity(monster)
            .get::<StableActorId>()
            .expect("stable monster id")
            .0,
        0,
    );

    world.insert_resource(map);
    world.insert_resource(spatial);
    world.insert_resource(ActionQueue::default());
    world.insert_resource(EffectQueue::default());
    world.insert_resource(clock);
    world.insert_resource(SimulationStatus::WaitingForPlayer);
    world.insert_resource(ActionDecision::default());
    world.insert_resource(ActionOutcomeLog::default());
    world.insert_resource(CurrentActor::default());
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

    let hud_message = format!(
        "HP {}/{}  Pos ({}, {})  {:?}\nMove with HJKL/YUBN or Space to wait.",
        player_def.maximum_health,
        player_def.maximum_health,
        player_cell.x,
        player_cell.y,
        SimulationStatus::WaitingForPlayer
    );
    if let Some(entity) = world
        .query_filtered::<Entity, With<HudText>>()
        .iter(world)
        .next()
    {
        world.entity_mut(entity).insert(Text::new(hud_message));
    }

    if let Some(entity) = world
        .query_filtered::<Entity, With<LogText>>()
        .iter(world)
        .next()
    {
        world
            .entity_mut(entity)
            .insert(Text::new("A new delver enters the room."));
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
    let stable_actor_id = rogue_core::ActorId::new(persistent_id).expect("valid actor id");
    let entity_id = {
        let mut entity = world.spawn((
            Actor,
            BlocksMovement,
            BlocksSight,
            Health {
                current: definition.maximum_health,
                maximum: definition.maximum_health,
            },
            ActiveStatuses::default(),
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
            StableActorId(stable_actor_id),
            SessionEntity,
        ));

        if is_player {
            entity.insert(Player);
            entity.insert(Inventory::new(8));
        }
        if hostile {
            entity.insert((Monster, HostileToPlayer));
        }

        entity.id()
    };

    if let Some(mut index) = world.get_resource_mut::<StableEntityIndex>() {
        index.insert_actor(stable_actor_id, entity_id);
    }

    entity_id
}

fn spawn_loot_item(world: &mut World, level: LevelId, cell: IVec2, prototype: &str) -> Entity {
    let persistent_id = next_persistent_id(world);
    let stable_item_id = ItemId::new(persistent_id).expect("valid item id");
    let entity = world.spawn((
        Item,
        PrototypeId(prototype.to_string()),
        GridPosition { level, cell },
        PersistentId(persistent_id),
        StableItemId(stable_item_id),
        SessionEntity,
    ));
    let entity_id = entity.id();

    if let Some(mut index) = world.get_resource_mut::<StableEntityIndex>() {
        index.insert_item(stable_item_id, entity_id);
    }

    entity_id
}

fn next_persistent_id(world: &mut World) -> u64 {
    let mut allocator = world
        .get_resource_mut::<PersistentIdAllocator>()
        .expect("persistent id allocator");
    allocator
        .allocate()
        .expect("persistent id allocator exhausted")
        .0
}

fn insert_occupant(
    spatial: &mut SpatialIndex,
    stable_actor: Option<&StableActorId>,
    stable_item: Option<&StableItemId>,
    persistent_id: Option<PersistentId>,
    level: LevelId,
    cell: IVec2,
    entity: Entity,
    blocks_movement: bool,
    blocks_sight: bool,
) {
    let position = GridPosition { level, cell };
    spatial.insert_occupant(
        entity,
        position,
        stable_actor,
        stable_item,
        persistent_id.as_ref(),
        blocks_movement,
        blocks_sight,
    );
}

fn drive_simulation_if_resolving(world: &mut World) {
    if world.resource::<SimulationStatus>() == &SimulationStatus::Resolving {
        rogue_core::drive_simulation(world);
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

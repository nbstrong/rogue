use bevy_app::App;
use bevy_math::IVec2;
use rogue_core::actor::components::Vision;
use rogue_core::actor::components::{BlocksSight, Player};
use rogue_core::simulation::SimulationPlugin;
use rogue_core::simulation::SimulationStep;
use rogue_core::world::fov::recalculate_fov_for_player;
use rogue_core::world::map::{GridPosition, LevelId, LevelMap};
use rogue_core::world::spatial::SpatialIndex;
use rogue_core::world::tile::TileKind;

fn recalculate(
    map: &mut LevelMap,
    spatial: &SpatialIndex,
    level: LevelId,
    cell: IVec2,
    range: u32,
) {
    recalculate_fov_for_player(map, spatial, GridPosition { level, cell }, range);
}

#[test]
fn vision_range_limits_visibility() {
    let mut map = LevelMap::new(9, 9, TileKind::Floor);
    let spatial = SpatialIndex::default();
    let player_level = LevelId(0);
    let player_cell = IVec2::new(4, 4);

    recalculate(
        &mut map,
        &spatial,
        player_level,
        player_cell,
        Vision { range: 3 }.range,
    );

    assert!(map.tile(IVec2::new(4, 1)).unwrap().visible);
    assert!(!map.tile(IVec2::new(4, 0)).unwrap().visible);
}

#[test]
fn sight_blockers_occlude_cells_behind_them_but_remain_visible_themselves() {
    let mut map = LevelMap::new(7, 7, TileKind::Floor);
    let mut spatial = SpatialIndex::default();
    let level = LevelId(0);
    let player_cell = IVec2::new(2, 3);
    let blocker_cell = IVec2::new(3, 3);
    let hidden_cell = IVec2::new(4, 3);

    spatial.sight_blockers.insert((level, blocker_cell));

    recalculate(&mut map, &spatial, level, player_cell, 5);

    assert!(map.tile(blocker_cell).unwrap().visible);
    assert!(!map.tile(hidden_cell).unwrap().visible);
}

#[test]
fn removing_a_sight_blocker_reveals_the_cells_behind_it() {
    let mut map = LevelMap::new(7, 7, TileKind::Floor);
    let mut spatial = SpatialIndex::default();
    let level = LevelId(0);
    let player_cell = IVec2::new(2, 3);
    let blocker_cell = IVec2::new(3, 3);
    let hidden_cell = IVec2::new(4, 3);

    spatial.sight_blockers.insert((level, blocker_cell));
    recalculate(&mut map, &spatial, level, player_cell, 5);
    assert!(!map.tile(hidden_cell).unwrap().visible);

    spatial.sight_blockers.clear();
    recalculate(&mut map, &spatial, level, player_cell, 5);

    assert!(map.tile(hidden_cell).unwrap().visible);
}

#[test]
fn opening_a_closed_door_reveals_cells_beyond_it() {
    let mut map = LevelMap::new(7, 7, TileKind::Floor);
    let spatial = SpatialIndex::default();
    let level = LevelId(0);
    let player_cell = IVec2::new(2, 3);
    let door_cell = IVec2::new(3, 3);
    let beyond_cell = IVec2::new(4, 3);

    map.tile_mut(door_cell).unwrap().kind = TileKind::ClosedDoor;
    recalculate(&mut map, &spatial, level, player_cell, 5);
    assert!(!map.tile(beyond_cell).unwrap().visible);

    map.tile_mut(door_cell).unwrap().kind = TileKind::OpenDoor;
    recalculate(&mut map, &spatial, level, player_cell, 5);

    assert!(map.tile(beyond_cell).unwrap().visible);
}

#[test]
fn sight_blockers_on_other_levels_do_not_occlude_the_active_level() {
    let mut map = LevelMap::new(7, 7, TileKind::Floor);
    let mut spatial = SpatialIndex::default();
    let active_level = LevelId(0);
    let other_level = LevelId(1);
    let player_cell = IVec2::new(2, 3);
    let beyond_cell = IVec2::new(4, 3);

    spatial
        .sight_blockers
        .insert((other_level, IVec2::new(3, 3)));

    recalculate(&mut map, &spatial, active_level, player_cell, 5);

    assert!(map.tile(beyond_cell).unwrap().visible);
}

#[test]
fn moving_a_sight_blocker_updates_visibility_on_the_next_pipeline_step() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);

    let level = LevelId(0);
    let player_cell = IVec2::new(2, 3);
    let blocker_cell = IVec2::new(3, 3);
    let hidden_cell = IVec2::new(4, 3);
    let level_map = LevelMap::new(7, 7, TileKind::Floor);

    let player = app
        .world_mut()
        .spawn((
            Player,
            Vision { range: 5 },
            GridPosition {
                level,
                cell: player_cell,
            },
        ))
        .id();
    let blocker = app
        .world_mut()
        .spawn((
            BlocksSight,
            GridPosition {
                level,
                cell: blocker_cell,
            },
        ))
        .id();

    app.world_mut().insert_resource(level_map);
    app.world_mut().insert_resource(SpatialIndex::default());

    app.world_mut().run_schedule(SimulationStep);

    assert!(
        !app.world()
            .resource::<LevelMap>()
            .tile(hidden_cell)
            .unwrap()
            .visible
    );
    assert_eq!(
        app.world().resource::<SpatialIndex>().sight_blockers.len(),
        1
    );

    app.world_mut().entity_mut(blocker).insert(GridPosition {
        level,
        cell: IVec2::new(6, 6),
    });

    app.world_mut().run_schedule(SimulationStep);

    assert!(
        app.world()
            .resource::<LevelMap>()
            .tile(hidden_cell)
            .unwrap()
            .visible
    );
    assert_eq!(
        app.world().resource::<SpatialIndex>().sight_blockers.len(),
        1
    );
    assert!(app.world().entity(player).contains::<Player>());
}

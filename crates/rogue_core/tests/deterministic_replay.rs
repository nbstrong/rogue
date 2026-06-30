use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use serde::Deserialize;

use rogue_core::action::intent::{Action, ActionKind};
use rogue_core::action::queue::ActionQueue;
use rogue_core::actor::components::{
    ActionSpeed, Actor, BlocksMovement, BlocksSight, CombatStats, Health, HostileToPlayer,
    PersistentId, PersistentIdAllocator, Player, PrototypeId, Vision,
};
use rogue_core::persistence::migration::{CURRENT_SAVE_VERSION, migrate_snapshot};
use rogue_core::persistence::rng::RandomStreams;
use rogue_core::persistence::snapshot::{
    ActionKindSnapshot, GameSnapshot, snapshot_digest, snapshot_world,
};
use rogue_core::simulation::{SimulationPlugin, SimulationStatus};
use rogue_core::time::clock::{CurrentActor, TurnClock};
use rogue_core::world::fov::recalculate_fov_for_player;
use rogue_core::world::generation::generate_one_room;
use rogue_core::world::map::{GridPosition, LevelId, LevelMap};
use rogue_core::world::spatial::SpatialIndex;

#[derive(Debug, Clone, Deserialize)]
struct ReplayFixture {
    seed: u64,
    commands: Vec<ActionKindSnapshot>,
    expected_digest: String,
}

fn load_fixture() -> ReplayFixture {
    ron::de::from_str(include_str!("fixtures/deterministic_replay.ron"))
        .expect("deterministic replay fixture")
}

fn command_to_action_kind(command: &ActionKindSnapshot) -> ActionKind {
    match command {
        ActionKindSnapshot::Wait => ActionKind::Wait,
        ActionKindSnapshot::Move { dx, dy } => ActionKind::Move {
            delta: IVec2::new(*dx, *dy),
        },
        ActionKindSnapshot::Melee { .. } => {
            panic!("fixture melee commands are not used in this replay test")
        }
        ActionKindSnapshot::PickUp { .. } => {
            panic!("fixture pick-up commands are not used in this replay test")
        }
        ActionKindSnapshot::Drop { .. } => {
            panic!("fixture drop commands are not used in this replay test")
        }
        ActionKindSnapshot::UseItem { .. } => {
            panic!("fixture item-use commands are not used in this replay test")
        }
        ActionKindSnapshot::Descend => ActionKind::Descend,
        ActionKindSnapshot::Ascend => ActionKind::Ascend,
    }
}

fn allocate_persistent_id(world: &mut World) -> PersistentId {
    world.resource_mut::<PersistentIdAllocator>().allocate()
}

fn build_spatial_index(world: &mut World) {
    let mut spatial = SpatialIndex::default();
    let mut query = world.query::<(
        Entity,
        &GridPosition,
        Option<&BlocksMovement>,
        Option<&BlocksSight>,
    )>();

    for (entity, position, blocks_movement, blocks_sight) in query.iter(world) {
        let key = (position.level, position.cell);
        spatial.occupants.entry(key).or_default().push(entity);
        if blocks_movement.is_some() {
            spatial.movement_blockers.insert(key);
        }
        if blocks_sight.is_some() {
            spatial.sight_blockers.insert(key);
        }
    }

    world.insert_resource(spatial);
}

fn initialize_world(app: &mut App, seed: u64) {
    app.world_mut().insert_resource(generate_one_room(7, 7));
    app.world_mut().insert_resource(SpatialIndex::default());
    app.world_mut().insert_resource(RandomStreams::seeded(seed));
    app.world_mut()
        .insert_resource(PersistentIdAllocator::default());
    app.world_mut()
        .insert_resource(rogue_core::item::effects::EffectQueue::default());
    app.world_mut().insert_resource(ActionQueue::default());
    app.world_mut()
        .insert_resource(SimulationStatus::WaitingForPlayer);
    app.world_mut().insert_resource(CurrentActor::default());

    let player_persistent_id = {
        let world = app.world_mut();
        allocate_persistent_id(world)
    };
    let monster_persistent_id = {
        let world = app.world_mut();
        allocate_persistent_id(world)
    };

    let player = {
        let entity = app.world_mut().spawn((
            Actor,
            Player,
            BlocksMovement,
            BlocksSight,
            Health {
                current: 10,
                maximum: 10,
            },
            CombatStats {
                power: 3,
                defense: 1,
            },
            Vision { range: 8 },
            ActionSpeed {
                ticks_per_action: 100,
            },
            PrototypeId("player".to_string()),
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(2, 2),
            },
            player_persistent_id,
        ));
        entity.id()
    };

    let monster = {
        let entity = app.world_mut().spawn((
            Actor,
            HostileToPlayer,
            BlocksMovement,
            BlocksSight,
            Health {
                current: 6,
                maximum: 6,
            },
            CombatStats {
                power: 2,
                defense: 0,
            },
            Vision { range: 8 },
            ActionSpeed {
                ticks_per_action: 100,
            },
            PrototypeId("ogre".to_string()),
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(5, 2),
            },
            monster_persistent_id,
        ));
        entity.id()
    };

    app.world_mut()
        .resource_mut::<TurnClock>()
        .schedule_at(player, 0);
    app.world_mut()
        .resource_mut::<TurnClock>()
        .schedule_at(monster, 0);

    build_spatial_index(app.world_mut());

    let spatial = app.world().resource::<SpatialIndex>().clone();
    let player_position = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<(&GridPosition, &Vision), With<Player>>();
        query
            .iter(world)
            .next()
            .map(|(position, vision)| (*position, *vision))
            .expect("player position")
    };
    let mut map = app.world_mut().resource_mut::<LevelMap>();
    recalculate_fov_for_player(
        &mut map,
        &spatial,
        player_position.0,
        player_position.1.range,
    );
}

fn drive_command(app: &mut App, command: &ActionKindSnapshot) {
    let player = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<Player>>();
        query.iter(world).next().expect("player entity")
    };

    app.world_mut().resource_mut::<ActionQueue>().push(Action {
        actor: player,
        kind: command_to_action_kind(command),
    });
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;
    rogue_core::drive_simulation(app.world_mut());
}

fn run_replay(fixture: &ReplayFixture) -> GameSnapshot {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, fixture.seed);

    for command in &fixture.commands {
        drive_command(&mut app, command);
    }

    snapshot_world(app.world()).expect("snapshot should be valid")
}

#[test]
fn replay_fixture_produces_a_stable_digest() {
    let fixture = load_fixture();
    let snapshot = run_replay(&fixture);
    let digest = snapshot_digest(&snapshot).expect("digest");

    assert_eq!(snapshot.version, CURRENT_SAVE_VERSION);
    assert_eq!(digest, fixture.expected_digest);
}

#[test]
fn save_roundtrip_preserves_the_digest() {
    let fixture = load_fixture();
    let snapshot = run_replay(&fixture);
    let text = rogue_core::persistence::snapshot::snapshot_to_text(&snapshot).expect("serialize");
    let restored =
        rogue_core::persistence::snapshot::snapshot_from_text(&text).expect("deserialize");

    assert_eq!(snapshot, restored);
    assert_eq!(
        snapshot_digest(&restored).expect("digest"),
        snapshot_digest(&snapshot).expect("digest")
    );
}

#[test]
fn future_snapshot_versions_are_rejected() {
    let fixture = load_fixture();
    let mut snapshot = run_replay(&fixture);
    snapshot.version = CURRENT_SAVE_VERSION + 1;

    assert!(migrate_snapshot(snapshot).is_err());
}

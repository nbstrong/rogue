use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use serde::Deserialize;
use std::collections::HashMap;

use rogue_core::action::intent::{Action, ActionKind};
use rogue_core::action::queue::ActionQueue;
use rogue_core::actor::components::{
    ActionSpeed, Actor, BlocksMovement, BlocksSight, CombatStats, Health, HostileToPlayer,
    PersistentId, PersistentIdAllocator, Player, PrototypeId, Vision,
};
use rogue_core::item::components::{Inventory, Item};
use rogue_core::persistence::migration::{CURRENT_SAVE_VERSION, migrate_snapshot};
use rogue_core::persistence::rng::RandomStreams;
use rogue_core::persistence::snapshot::{
    ActionKindSnapshot, GameSnapshot, snapshot_digest, snapshot_from_text, snapshot_world,
};
use rogue_core::simulation::{SimulationPlugin, SimulationStatus};
use rogue_core::time::clock::{CurrentActor, TurnClock};
use rogue_core::world::fov::recalculate_fov_for_player;
use rogue_core::world::generation::generate_one_room_with_rng;
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

fn persistent_entity_map(world: &mut World) -> HashMap<u64, Entity> {
    let mut entities = HashMap::new();
    let mut query = world.query::<(Entity, &PersistentId)>();
    for (entity, id) in query.iter(world) {
        entities.insert(id.0, entity);
    }
    entities
}

fn command_to_action_kind(
    entity_map: &HashMap<u64, Entity>,
    command: &ActionKindSnapshot,
) -> ActionKind {
    match command {
        ActionKindSnapshot::Wait => ActionKind::Wait,
        ActionKindSnapshot::Move { dx, dy } => ActionKind::Move {
            delta: IVec2::new(*dx, *dy),
        },
        ActionKindSnapshot::Melee { target } => ActionKind::Melee {
            target: entity_map
                .get(target)
                .copied()
                .unwrap_or_else(|| panic!("missing entity for persistent id {}", target)),
        },
        ActionKindSnapshot::PickUp { item } => ActionKind::PickUp {
            item: entity_map
                .get(item)
                .copied()
                .unwrap_or_else(|| panic!("missing entity for persistent id {}", item)),
        },
        ActionKindSnapshot::Drop { item } => ActionKind::Drop {
            item: entity_map
                .get(item)
                .copied()
                .unwrap_or_else(|| panic!("missing entity for persistent id {}", item)),
        },
        ActionKindSnapshot::UseItem { item, target } => ActionKind::UseItem {
            item: entity_map
                .get(item)
                .copied()
                .unwrap_or_else(|| panic!("missing entity for persistent id {}", item)),
            target: match target {
                rogue_core::persistence::snapshot::ActionTargetSnapshot::SelfTarget => {
                    rogue_core::action::intent::ActionTarget::SelfTarget
                }
                rogue_core::persistence::snapshot::ActionTargetSnapshot::Entity(id) => {
                    rogue_core::action::intent::ActionTarget::Entity(
                        entity_map
                            .get(id)
                            .copied()
                            .unwrap_or_else(|| panic!("missing entity for persistent id {}", id)),
                    )
                }
                rogue_core::persistence::snapshot::ActionTargetSnapshot::Cell { level, x, y } => {
                    rogue_core::action::intent::ActionTarget::Cell {
                        level: LevelId(*level),
                        position: IVec2::new(*x, *y),
                    }
                }
            },
        },
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
    app.world_mut().insert_resource(RandomStreams::seeded(seed));
    let map = {
        let mut rng = app.world_mut().resource_mut::<RandomStreams>();
        generate_one_room_with_rng(7, 7, Some(&mut *rng))
    };
    app.world_mut().insert_resource(map);
    app.world_mut().insert_resource(SpatialIndex::default());
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
    let loot_persistent_id = {
        let world = app.world_mut();
        allocate_persistent_id(world)
    };
    let loot_proto = {
        let mut rng = app.world_mut().resource_mut::<RandomStreams>();
        if rng.next_loot_u64() & 1 == 0 {
            "healing_potion"
        } else {
            "trinket"
        }
    };

    let player = {
        let entity = app.world_mut().spawn((
            Actor,
            Player,
            BlocksMovement,
            BlocksSight,
            Health {
                current: 8,
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
            Inventory::new(4),
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

    let _loot = {
        let entity = app.world_mut().spawn((
            Item,
            PrototypeId(loot_proto.to_string()),
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(2, 2),
            },
            loot_persistent_id,
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
    let (player, entity_map) = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<Player>>();
        let player = query.iter(world).next().expect("player entity");
        (player, persistent_entity_map(world))
    };

    app.world_mut().resource_mut::<ActionQueue>().push(Action {
        actor: player,
        kind: command_to_action_kind(&entity_map, command),
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

    let monster = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<HostileToPlayer>>();
        query.iter(world).next().expect("monster entity")
    };
    *app.world_mut().resource_mut::<CurrentActor>() = CurrentActor(Some(monster));
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;
    rogue_core::drive_simulation(app.world_mut());
    {
        let mut rng = app.world_mut().resource_mut::<RandomStreams>();
        rng.next_ai_u64();
        rng.next_combat_u64();
        rng.next_loot_u64();
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
fn identical_replays_produce_identical_snapshots() {
    let fixture = load_fixture();
    let first = run_replay(&fixture);
    let second = run_replay(&fixture);

    assert_eq!(first, second);
    assert_eq!(
        snapshot_digest(&first).expect("digest"),
        snapshot_digest(&second).expect("digest")
    );
}

#[test]
fn save_roundtrip_preserves_the_digest() {
    let fixture = load_fixture();
    let snapshot = run_replay(&fixture);
    let text = rogue_core::persistence::snapshot::snapshot_to_text(&snapshot).expect("serialize");
    let restored = migrate_snapshot(snapshot_from_text(&text).expect("deserialize"))
        .expect("migrate snapshot");

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

    assert!(
        migrate_snapshot(rogue_core::persistence::migration::SnapshotFile::Current(
            snapshot
        ))
        .is_err()
    );
}

#[test]
fn legacy_snapshot_v1_is_migrated() {
    let snapshot = snapshot_from_text(include_str!("fixtures/legacy_snapshot_v1.ron"))
        .expect("deserialize legacy snapshot");
    let migrated = migrate_snapshot(snapshot).expect("migrate legacy snapshot");

    assert_eq!(migrated.version, CURRENT_SAVE_VERSION);
    assert_eq!(migrated.next_sequence, 0);
}

#[test]
fn replay_advances_all_authoritative_rng_streams() {
    let fixture = load_fixture();
    let snapshot = run_replay(&fixture);
    let initial = RandomStreams::seeded(fixture.seed).snapshot();

    assert_ne!(snapshot.rng.generation_state, initial.generation_state);
    assert_ne!(snapshot.rng.ai_state, initial.ai_state);
    assert_ne!(snapshot.rng.combat_state, initial.combat_state);
    assert_ne!(snapshot.rng.loot_state, initial.loot_state);
}

use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use bevy_math::IVec2;
use serde::Deserialize;
use std::collections::HashMap;

use rogue_core::action::intent::{Action, ActionKind};
use rogue_core::action::queue::ActionQueue;
use rogue_core::actor::combat::StatusEffect;
use rogue_core::actor::components::{
    ActionSpeed, ActiveStatuses, Actor, BlocksMovement, BlocksSight, CombatStats, Health,
    HostileToPlayer, Monster, PersistentId, PersistentIdAllocator, Player, PrototypeId, Vision,
};
use rogue_core::actor::spawn::spawn_vertical_slice;
use rogue_core::item::components::{Inventory, Item};
use rogue_core::item::effects::{Effect, EffectQueue, apply_pending_effects};
use rogue_core::persistence::migration::{CURRENT_SAVE_VERSION, migrate_snapshot};
use rogue_core::persistence::rng::RandomStreams;
use rogue_core::persistence::snapshot::{
    ActionKindSnapshot, AiGoalSnapshot, GameSnapshot, SavedInventory, SavedLastKnownPlayerPosition,
    SavedPosition, ScheduledActorSnapshot, SimulationStatusSnapshot, snapshot_digest,
    snapshot_from_text, snapshot_to_text, snapshot_world,
};
use rogue_core::simulation::{SimulationPlugin, SimulationStatus};
use rogue_core::time::clock::{CurrentActor, TurnClock};
use rogue_core::world::fov::recalculate_fov_for_player;
use rogue_core::world::generation::generate_one_room_with_rng;
use rogue_core::world::map::{GridPosition, LevelId, LevelMap};
use rogue_core::world::spatial::SpatialIndex;
use rogue_core::world::tile::TileKind;

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
            ActiveStatuses::default(),
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
            Monster,
            HostileToPlayer,
            BlocksMovement,
            BlocksSight,
            Health {
                current: 6,
                maximum: 6,
            },
            ActiveStatuses::default(),
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

    let monster = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<Monster>>();
        query.iter(world).next().expect("monster entity")
    };
    app.world_mut()
        .resource_mut::<ActionQueue>()
        .actions
        .clear();
    *app.world_mut().resource_mut::<CurrentActor>() = CurrentActor(Some(monster));
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;
    app.world_mut()
        .run_system_once(rogue_core::actor::ai::generate_ai_action)
        .expect("run ai system");
    rogue_core::drive_simulation(app.world_mut());

    for command in &fixture.commands {
        drive_command(&mut app, command);
    }

    snapshot_world(app.world()).expect("snapshot should be valid")
}

fn run_commands(app: &mut App, commands: &[ActionKindSnapshot]) {
    for command in commands {
        drive_command(app, command);
    }
}

fn restore_app_from_snapshot(snapshot: &GameSnapshot) -> App {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    rogue_core::persistence::snapshot::restore_world(app.world_mut(), snapshot)
        .expect("restore snapshot");
    app
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

#[test]
fn continuation_after_restore_matches_the_original_world() {
    let fixture = load_fixture();
    let split = fixture.commands.len().saturating_sub(2);
    let mut original_app = App::new();
    original_app.add_plugins(SimulationPlugin);
    initialize_world(&mut original_app, fixture.seed);
    run_commands(&mut original_app, &fixture.commands[..split]);

    let prefix_snapshot = snapshot_world(original_app.world()).expect("prefix snapshot");
    let prefix_text = snapshot_to_text(&prefix_snapshot).expect("serialize prefix");
    let restored_snapshot = match snapshot_from_text(&prefix_text).expect("deserialize prefix") {
        rogue_core::persistence::migration::SnapshotFile::Current(snapshot) => snapshot,
        rogue_core::persistence::migration::SnapshotFile::V1(_) => {
            panic!("prefix snapshot unexpectedly downgraded")
        }
    };

    let mut restored_app = restore_app_from_snapshot(&restored_snapshot);
    run_commands(&mut original_app, &fixture.commands[split..]);
    run_commands(&mut restored_app, &fixture.commands[split..]);

    let original_final = snapshot_world(original_app.world()).expect("original final snapshot");
    let restored_final = snapshot_world(restored_app.world()).expect("restored final snapshot");

    assert_eq!(original_final, restored_final);
    assert_eq!(
        snapshot_digest(&original_final).expect("original digest"),
        snapshot_digest(&restored_final).expect("restored digest")
    );
}

#[test]
fn snapshot_world_requires_authoritative_resources() {
    let cases = [
        ("random streams", "missing random streams resource"),
        (
            "persistent id allocator",
            "missing persistent id allocator resource",
        ),
        ("turn clock", "missing turn clock resource"),
        ("action queue", "missing action queue resource"),
        ("effect queue", "missing effect queue resource"),
        ("simulation status", "missing simulation status resource"),
        ("current actor", "missing current actor resource"),
        ("action decision", "missing action decision resource"),
    ];

    for (label, expected) in cases {
        let mut app = App::new();
        app.add_plugins(SimulationPlugin);
        initialize_world(&mut app, 0);

        match label {
            "random streams" => {
                app.world_mut().remove_resource::<RandomStreams>();
            }
            "persistent id allocator" => {
                app.world_mut().remove_resource::<PersistentIdAllocator>();
            }
            "turn clock" => {
                app.world_mut().remove_resource::<TurnClock>();
            }
            "action queue" => {
                app.world_mut().remove_resource::<ActionQueue>();
            }
            "effect queue" => {
                app.world_mut()
                    .remove_resource::<rogue_core::item::effects::EffectQueue>();
            }
            "simulation status" => {
                app.world_mut().remove_resource::<SimulationStatus>();
            }
            "current actor" => {
                app.world_mut().remove_resource::<CurrentActor>();
            }
            "action decision" => {
                app.world_mut()
                    .remove_resource::<rogue_core::action::resolver::ActionDecision>();
            }
            _ => unreachable!(),
        }

        let err = snapshot_world(app.world()).expect_err(label);
        assert!(
            err.contains(expected),
            "{} should mention `{}` but was `{}`",
            label,
            expected,
            err
        );
    }
}

#[test]
fn failed_restore_does_not_mutate_the_live_world() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);

    let before = snapshot_world(app.world()).expect("before snapshot");
    let mut corrupted = before.clone();
    corrupted.root_seed ^= 1;
    let result = rogue_core::persistence::snapshot::restore_world(app.world_mut(), &corrupted);
    assert!(result.is_err());

    let after = snapshot_world(app.world()).expect("after snapshot");
    assert_eq!(before, after);
}

#[test]
fn malformed_snapshots_are_rejected() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);
    let base = snapshot_world(app.world()).expect("base snapshot");

    let player_id = base
        .entities
        .iter()
        .find(|entity| entity.player)
        .map(|entity| entity.id)
        .expect("player id");
    let monster_id = base
        .entities
        .iter()
        .find(|entity| entity.monster)
        .map(|entity| entity.id)
        .expect("monster id");
    let item_id = base
        .entities
        .iter()
        .find(|entity| entity.item)
        .map(|entity| entity.id)
        .expect("item id");

    let mut cases = Vec::new();

    let mut mid_step = base.clone();
    mid_step.current_actor = Some(player_id);
    cases.push(("mid-step current actor", mid_step, "stable save boundary"));

    let mut resolving = base.clone();
    resolving.simulation_status = SimulationStatusSnapshot::Resolving;
    cases.push(("resolving status", resolving, "stable save boundary"));

    let mut duplicate_ids = base.clone();
    duplicate_ids
        .entities
        .iter_mut()
        .find(|entity| entity.id == monster_id)
        .expect("monster entity")
        .id = player_id;
    cases.push((
        "duplicate persistent id",
        duplicate_ids,
        "duplicate persistent id",
    ));

    let mut invalid_allocator = base.clone();
    invalid_allocator.persistent_ids.next_available = player_id;
    cases.push((
        "invalid allocator",
        invalid_allocator,
        "must exceed max entity id",
    ));

    let mut duplicate_levels = base.clone();
    duplicate_levels
        .levels
        .push(duplicate_levels.levels[0].clone());
    cases.push(("duplicate levels", duplicate_levels, "duplicate level ids"));

    let mut dangling_reference = base.clone();
    dangling_reference
        .entities
        .iter_mut()
        .find(|entity| entity.id == monster_id)
        .expect("monster entity")
        .ai_goal = Some(AiGoalSnapshot::Chase(999_999));
    cases.push(("dangling reference", dangling_reference, "missing entity"));

    let mut invalid_timeline = base.clone();
    invalid_timeline.timeline.push(ScheduledActorSnapshot {
        next_tick: invalid_timeline.current_tick,
        sequence: 0,
        actor: player_id,
    });
    invalid_timeline.next_sequence = 0;
    cases.push((
        "invalid timeline sequence",
        invalid_timeline,
        "next_sequence",
    ));

    let mut zero_dimensions = base.clone();
    zero_dimensions.levels[0].width = 0;
    zero_dimensions.levels[0].height = 0;
    zero_dimensions.levels[0].tiles.clear();
    cases.push(("zero dimensions", zero_dimensions, "zero dimensions"));

    let mut invalid_position = base.clone();
    invalid_position
        .entities
        .iter_mut()
        .find(|entity| entity.id == player_id)
        .expect("player entity")
        .position = Some(SavedPosition {
        level: 0,
        x: 999,
        y: 0,
    });
    cases.push((
        "out of bounds position",
        invalid_position,
        "out-of-bounds position",
    ));

    let mut invalid_investigate = base.clone();
    invalid_investigate
        .entities
        .iter_mut()
        .find(|entity| entity.id == monster_id)
        .expect("monster entity")
        .ai_goal = Some(AiGoalSnapshot::Investigate(SavedPosition {
        level: 0,
        x: 999,
        y: 999,
    }));
    cases.push((
        "out of bounds investigate",
        invalid_investigate,
        "out-of-bounds position",
    ));

    let mut invalid_last_known = base.clone();
    invalid_last_known
        .entities
        .iter_mut()
        .find(|entity| entity.id == monster_id)
        .expect("monster entity")
        .last_known_player_position = Some(SavedLastKnownPlayerPosition {
        level: 0,
        x: 999,
        y: 999,
        observed_at: 1,
    });
    cases.push((
        "out of bounds last known",
        invalid_last_known,
        "out-of-bounds position",
    ));

    let mut non_item = base.clone();
    non_item
        .entities
        .iter_mut()
        .find(|entity| entity.id == player_id)
        .expect("player entity")
        .inventory = Some(SavedInventory {
        capacity: 4,
        items: vec![player_id],
    });
    cases.push(("inventory references non-item", non_item, "non-item"));

    let mut missing_carried = base.clone();
    missing_carried
        .entities
        .iter_mut()
        .find(|entity| entity.id == player_id)
        .expect("player entity")
        .inventory = Some(SavedInventory {
        capacity: 4,
        items: vec![item_id],
    });
    cases.push((
        "inventory missing carried_by",
        missing_carried,
        "missing carried_by",
    ));

    let mut mismatch = base.clone();
    mismatch
        .entities
        .iter_mut()
        .find(|entity| entity.id == player_id)
        .expect("player entity")
        .inventory = Some(SavedInventory {
        capacity: 4,
        items: vec![item_id],
    });
    mismatch
        .entities
        .iter_mut()
        .find(|entity| entity.id == item_id)
        .expect("item entity")
        .carried_by = Some(monster_id);
    cases.push((
        "carried_by mismatch",
        mismatch,
        "disagrees with inventory owner",
    ));

    let mut duplicate_inventory = base.clone();
    duplicate_inventory
        .entities
        .iter_mut()
        .find(|entity| entity.id == player_id)
        .expect("player entity")
        .inventory = Some(SavedInventory {
        capacity: 4,
        items: vec![item_id, item_id],
    });
    duplicate_inventory
        .entities
        .iter_mut()
        .find(|entity| entity.id == item_id)
        .expect("item entity")
        .carried_by = Some(player_id);
    cases.push((
        "duplicate inventory item",
        duplicate_inventory,
        "appears in multiple inventories",
    ));

    for (label, snapshot, expected) in cases {
        let mut app = App::new();
        app.add_plugins(SimulationPlugin);
        let err = rogue_core::persistence::snapshot::restore_world(app.world_mut(), &snapshot)
            .expect_err(label);
        assert!(
            err.contains(expected),
            "{} should mention `{}` but was `{}`",
            label,
            expected,
            err
        );
    }
}

#[test]
fn spawn_vertical_slice_advances_the_authoritative_allocator() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    app.world_mut().insert_resource(RandomStreams::seeded(0));
    app.world_mut()
        .insert_resource(LevelMap::new(7, 7, TileKind::Floor));
    app.world_mut().insert_resource(SpatialIndex::default());
    app.world_mut()
        .insert_resource(PersistentIdAllocator::default());
    app.world_mut().insert_resource(ActionQueue::default());
    app.world_mut().insert_resource(EffectQueue::default());
    app.world_mut()
        .insert_resource(SimulationStatus::WaitingForPlayer);
    app.world_mut().insert_resource(CurrentActor::default());

    let _ = app.world_mut().run_system_once(
        |mut commands: Commands<'_, '_>, mut allocator: ResMut<'_, PersistentIdAllocator>| {
            spawn_vertical_slice(&mut commands, &mut allocator);
            let bonus_id = allocator.allocate();
            commands.spawn((
                Item,
                PrototypeId("bonus".to_string()),
                GridPosition {
                    level: LevelId(0),
                    cell: IVec2::new(1, 1),
                },
                bonus_id,
            ));
        },
    );

    let mut ids: Vec<u64> = {
        let world = app.world_mut();
        let mut query = world.query::<&PersistentId>();
        query.iter(world).map(|id| id.0).collect()
    };
    ids.sort_unstable();

    assert_eq!(ids, vec![1, 2, 3]);
    assert_eq!(
        app.world()
            .resource::<PersistentIdAllocator>()
            .next_available(),
        4
    );
    snapshot_world(app.world()).expect("vertical slice snapshot");
}

#[test]
fn nonzero_level_ids_survive_restore_and_resave() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    app.world_mut().insert_resource(RandomStreams::seeded(0));
    app.world_mut()
        .insert_resource(LevelMap::with_id(LevelId(7), 7, 7, TileKind::Floor));
    app.world_mut().insert_resource(SpatialIndex::default());
    app.world_mut()
        .insert_resource(PersistentIdAllocator::default());
    app.world_mut().insert_resource(ActionQueue::default());
    app.world_mut().insert_resource(EffectQueue::default());
    app.world_mut()
        .insert_resource(SimulationStatus::WaitingForPlayer);
    app.world_mut().insert_resource(CurrentActor::default());

    let player_id = {
        let mut allocator = app.world_mut().resource_mut::<PersistentIdAllocator>();
        allocator.allocate()
    };
    app.world_mut().spawn((
        Actor,
        Player,
        BlocksMovement,
        BlocksSight,
        Health {
            current: 10,
            maximum: 10,
        },
        ActiveStatuses::default(),
        CombatStats {
            power: 3,
            defense: 1,
        },
        Vision { range: 8 },
        ActionSpeed {
            ticks_per_action: 100,
        },
        PrototypeId("player".to_string()),
        Inventory::new(8),
        GridPosition {
            level: LevelId(7),
            cell: IVec2::new(2, 2),
        },
        player_id,
    ));

    build_spatial_index(app.world_mut());
    let player_position = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<(&GridPosition, &Vision), With<Player>>();
        query
            .iter(world)
            .next()
            .map(|(position, vision)| (*position, *vision))
            .expect("player position")
    };
    let spatial = app.world().resource::<SpatialIndex>().clone();
    let mut map = app.world_mut().resource_mut::<LevelMap>();
    recalculate_fov_for_player(
        &mut map,
        &spatial,
        player_position.0,
        player_position.1.range,
    );

    let snapshot = snapshot_world(app.world()).expect("snapshot");
    assert_eq!(snapshot.current_level, 7);
    let text = snapshot_to_text(&snapshot).expect("serialize");
    let restored = match snapshot_from_text(&text).expect("deserialize") {
        rogue_core::persistence::migration::SnapshotFile::Current(snapshot) => snapshot,
        rogue_core::persistence::migration::SnapshotFile::V1(_) => {
            panic!("nonzero level snapshot should not downgrade")
        }
    };

    let mut restored_app = App::new();
    restored_app.add_plugins(SimulationPlugin);
    rogue_core::persistence::snapshot::restore_world(restored_app.world_mut(), &restored)
        .expect("restore");
    let restored_snapshot = snapshot_world(restored_app.world()).expect("restored snapshot");

    assert_eq!(snapshot, restored_snapshot);
}

#[test]
fn apply_pending_effects_batches_statuses_and_persists_them() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    app.world_mut().insert_resource(RandomStreams::seeded(0));
    app.world_mut()
        .insert_resource(LevelMap::new(7, 7, TileKind::Floor));
    app.world_mut().insert_resource(SpatialIndex::default());
    app.world_mut()
        .insert_resource(PersistentIdAllocator::default());
    app.world_mut().insert_resource(ActionQueue::default());
    app.world_mut().insert_resource(EffectQueue::default());
    app.world_mut()
        .insert_resource(SimulationStatus::WaitingForPlayer);
    app.world_mut().insert_resource(CurrentActor::default());

    let player_id = {
        let mut allocator = app.world_mut().resource_mut::<PersistentIdAllocator>();
        allocator.allocate()
    };
    let player = app
        .world_mut()
        .spawn((
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
            player_id,
        ))
        .id();

    app.world_mut()
        .resource_mut::<EffectQueue>()
        .0
        .push_back(Effect::ApplyStatus {
            target: player,
            status: StatusEffect::Poisoned { remaining: 3 },
        });
    app.world_mut()
        .resource_mut::<EffectQueue>()
        .0
        .push_back(Effect::ApplyStatus {
            target: player,
            status: StatusEffect::Stunned { remaining: 2 },
        });

    app.world_mut()
        .run_system_once(apply_pending_effects)
        .expect("apply effects");

    let statuses = app
        .world()
        .entity(player)
        .get::<ActiveStatuses>()
        .expect("active statuses");
    assert_eq!(
        statuses.0,
        vec![
            StatusEffect::Poisoned { remaining: 3 },
            StatusEffect::Stunned { remaining: 2 },
        ]
    );

    let snapshot = snapshot_world(app.world()).expect("snapshot");
    let text = snapshot_to_text(&snapshot).expect("serialize");
    let restored = match snapshot_from_text(&text).expect("deserialize") {
        rogue_core::persistence::migration::SnapshotFile::Current(snapshot) => snapshot,
        rogue_core::persistence::migration::SnapshotFile::V1(_) => {
            panic!("status snapshot should not downgrade")
        }
    };

    let mut restored_app = App::new();
    restored_app.add_plugins(SimulationPlugin);
    rogue_core::persistence::snapshot::restore_world(restored_app.world_mut(), &restored)
        .expect("restore");

    let restored_player = {
        let world = restored_app.world_mut();
        let mut query = world.query_filtered::<Entity, With<Player>>();
        query.iter(world).next().expect("restored player")
    };
    let restored_statuses = restored_app
        .world()
        .entity(restored_player)
        .get::<ActiveStatuses>()
        .expect("restored active statuses");
    assert_eq!(restored_statuses.0, statuses.0);
}

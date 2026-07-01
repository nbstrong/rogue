use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use bevy_math::IVec2;
use serde::Deserialize;

use rogue_core::SimulationWorkBudget;
use rogue_core::action::intent::{Action, ActionKind};
use rogue_core::action::queue::ActionQueue;
use rogue_core::actor::combat::StatusEffect;
use rogue_core::actor::components::{
    ActionSpeed, ActiveStatuses, Actor, BlocksMovement, BlocksSight, CombatStats, Health,
    HostileToPlayer, Monster, PersistentId, PersistentIdAllocator, Player, PrototypeId,
    StableActorId, StableItemId, Vision,
};
use rogue_core::actor::spawn::spawn_vertical_slice;
use rogue_core::item::components::{Inventory, Item};
use rogue_core::item::effects::{Effect, EffectQueue, apply_pending_effects};
use rogue_core::persistence::migration::{CURRENT_SAVE_VERSION, migrate_snapshot};
use rogue_core::persistence::rng::RandomStreams;
use rogue_core::persistence::snapshot::{
    ActionKindSnapshot, AiGoalSnapshot, GameSnapshot, SavedInventory, SavedLastKnownPlayerPosition,
    SavedPosition, ScheduledActorSnapshot, snapshot_digest, snapshot_from_text, snapshot_to_text,
    snapshot_world,
};
use rogue_core::simulation::{
    DomainWorkError, DomainWorkEvent, SimulationPlugin, SimulationStatus,
};
use rogue_core::time::clock::{CurrentActor, TurnClock};
use rogue_core::world::fov::recalculate_fov_for_player;
use rogue_core::world::generation::generate_one_room_with_rng;
use rogue_core::world::map::{GridPosition, LevelId, LevelMap};
use rogue_core::world::spatial::SpatialIndex;
use rogue_core::world::tile::TileKind;
use sim_core::{DomainWorkId, PresentationRng, SimSpeed};

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
        ActionKindSnapshot::Melee { target } => ActionKind::Melee {
            target: rogue_core::ActorId::new(*target).expect("valid actor id"),
        },
        ActionKindSnapshot::PickUp { item } => ActionKind::PickUp {
            item: rogue_core::ItemId::new(*item).expect("valid item id"),
        },
        ActionKindSnapshot::Drop { item } => ActionKind::Drop {
            item: rogue_core::ItemId::new(*item).expect("valid item id"),
        },
        ActionKindSnapshot::UseItem { item, target } => ActionKind::UseItem {
            item: rogue_core::ItemId::new(*item).expect("valid item id"),
            target: match target {
                rogue_core::persistence::snapshot::ActionTargetSnapshot::SelfTarget => {
                    rogue_core::action::intent::ActionTarget::SelfTarget
                }
                rogue_core::persistence::snapshot::ActionTargetSnapshot::Entity(id) => {
                    rogue_core::action::intent::ActionTarget::Actor(
                        rogue_core::ActorId::new(*id).expect("valid actor id"),
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
    world
        .resource_mut::<PersistentIdAllocator>()
        .allocate()
        .expect("persistent id allocator exhausted")
}

fn build_spatial_index(world: &mut World) {
    let mut spatial = SpatialIndex::default();
    let mut query = world.query::<(
        Entity,
        &GridPosition,
        Option<&BlocksMovement>,
        Option<&BlocksSight>,
        Option<&PersistentId>,
        Option<&StableActorId>,
        Option<&StableItemId>,
    )>();

    for (
        entity,
        position,
        blocks_movement,
        blocks_sight,
        persistent_id,
        stable_actor,
        stable_item,
    ) in query.iter(world)
    {
        spatial.insert_occupant(
            entity,
            *position,
            stable_actor,
            stable_item,
            persistent_id,
            blocks_movement.is_some(),
            blocks_sight.is_some(),
        );
    }

    world.insert_resource(spatial);
}

fn build_stable_entity_index(world: &mut World) {
    let mut index = rogue_core::actor::components::StableEntityIndex::default();
    let mut actors = world.query::<(Entity, &StableActorId)>();
    for (entity, stable_id) in actors.iter(world) {
        index.insert_actor(stable_id.0, entity);
    }

    let mut items = world.query::<(Entity, &StableItemId)>();
    for (entity, stable_id) in items.iter(world) {
        index.insert_item(stable_id.0, entity);
    }

    world.insert_resource(index);
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

    let _player = {
        let stable_actor_id =
            rogue_core::ActorId::new(player_persistent_id.0).expect("valid actor id");
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
            StableActorId(stable_actor_id),
        ));
        entity.id()
    };

    let _monster = {
        let stable_actor_id =
            rogue_core::ActorId::new(monster_persistent_id.0).expect("valid actor id");
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
            StableActorId(stable_actor_id),
        ));
        entity.id()
    };

    let _loot = {
        let stable_item_id = rogue_core::ItemId::new(loot_persistent_id.0).expect("valid item id");
        let entity = app.world_mut().spawn((
            Item,
            PrototypeId(loot_proto.to_string()),
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(2, 2),
            },
            loot_persistent_id,
            StableItemId(stable_item_id),
        ));
        entity.id()
    };

    app.world_mut().resource_mut::<TurnClock>().schedule_at(
        rogue_core::ActorId::new(player_persistent_id.0).expect("valid actor id"),
        0,
    );
    app.world_mut().resource_mut::<TurnClock>().schedule_at(
        rogue_core::ActorId::new(monster_persistent_id.0).expect("valid actor id"),
        0,
    );

    build_spatial_index(app.world_mut());
    build_stable_entity_index(app.world_mut());

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
        let mut query = world.query_filtered::<(&PersistentId, &StableActorId), With<Player>>();
        let (_, stable_id) = query.iter(world).next().expect("player entity");
        stable_id.0
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

    let monster = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<(&PersistentId, &StableActorId), With<Monster>>();
        let (_, stable_id) = query.iter(world).next().expect("monster entity");
        stable_id.0
    };
    app.world_mut()
        .resource_mut::<ActionQueue>()
        .actions
        .clear();
    *app.world_mut().resource_mut::<CurrentActor>() =
        sim_core::schedule::CurrentActor(Some(monster));
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;
    app.world_mut()
        .run_system_once(rogue_core::actor::ai::generate_ai_action)
        .expect("run ai system");
    rogue_core::drive_simulation(app.world_mut());
    drain_simulation(&mut app);

    for command in &fixture.commands {
        drive_command(&mut app, command);
        drain_simulation(&mut app);
    }

    snapshot_world(app.world()).expect("snapshot should be valid")
}

fn run_replay_with_budget_metrics(
    fixture: &ReplayFixture,
    maximum_steps_per_frame: usize,
    maximum_domain_events_per_frame: usize,
) -> (GameSnapshot, String) {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, fixture.seed);
    configure_domain_work(&mut app, SimSpeed::Normal, 40);
    set_simulation_budget(
        &mut app,
        maximum_steps_per_frame,
        maximum_domain_events_per_frame,
    );

    let monster = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<(&PersistentId, &StableActorId), With<Monster>>();
        let (_, stable_id) = query.iter(world).next().expect("monster entity");
        stable_id.0
    };
    app.world_mut()
        .resource_mut::<ActionQueue>()
        .actions
        .clear();
    *app.world_mut().resource_mut::<CurrentActor>() =
        sim_core::schedule::CurrentActor(Some(monster));
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;
    app.world_mut()
        .run_system_once(rogue_core::actor::ai::generate_ai_action)
        .expect("run ai system");
    rogue_core::drive_simulation(app.world_mut());
    drain_simulation(&mut app);

    for command in &fixture.commands {
        drive_command(&mut app, command);
        drain_simulation(&mut app);
    }

    let snapshot = snapshot_world(app.world()).expect("snapshot should be valid");
    let action_log = action_outcome_log_summary(&app);
    (snapshot, action_log)
}

fn run_replay_with_pre_spawned_unrelated_entity(fixture: &ReplayFixture) -> GameSnapshot {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    app.world_mut().spawn_empty();
    initialize_world(&mut app, fixture.seed);

    let monster = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<(&PersistentId, &StableActorId), With<Monster>>();
        let (_, stable_id) = query.iter(world).next().expect("monster entity");
        stable_id.0
    };
    app.world_mut()
        .resource_mut::<ActionQueue>()
        .actions
        .clear();
    *app.world_mut().resource_mut::<CurrentActor>() =
        sim_core::schedule::CurrentActor(Some(monster));
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;
    app.world_mut()
        .run_system_once(rogue_core::actor::ai::generate_ai_action)
        .expect("run ai system");
    rogue_core::drive_simulation(app.world_mut());
    drain_simulation(&mut app);

    for command in &fixture.commands {
        drive_command(&mut app, command);
        drain_simulation(&mut app);
    }

    snapshot_world(app.world()).expect("snapshot should be valid")
}

fn run_replay_with_speed_metrics(
    fixture: &ReplayFixture,
    speed: SimSpeed,
) -> (GameSnapshot, usize, String) {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, fixture.seed);
    configure_domain_work(&mut app, speed, 40);

    let monster = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<(&PersistentId, &StableActorId), With<Monster>>();
        let (_, stable_id) = query.iter(world).next().expect("monster entity");
        stable_id.0
    };
    app.world_mut()
        .resource_mut::<ActionQueue>()
        .actions
        .clear();
    *app.world_mut().resource_mut::<CurrentActor>() =
        sim_core::schedule::CurrentActor(Some(monster));
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;
    app.world_mut()
        .run_system_once(rogue_core::actor::ai::generate_ai_action)
        .expect("run ai system");
    rogue_core::drive_simulation(app.world_mut());
    let mut updates = 1 + drain_simulation_count(&mut app);

    for command in &fixture.commands {
        drive_command(&mut app, command);
        updates += 1 + drain_simulation_count(&mut app);
    }

    let snapshot = snapshot_world(app.world()).expect("snapshot should be valid");
    let action_log = action_outcome_log_summary(&app);
    (snapshot, updates, action_log)
}

fn run_replay_with_speed(fixture: &ReplayFixture, speed: SimSpeed) -> GameSnapshot {
    run_replay_with_speed_metrics(fixture, speed).0
}

fn projected_snapshot_without_speed(snapshot: &GameSnapshot) -> GameSnapshot {
    let mut projected = snapshot.clone();
    projected.simulation_driver.driver.clock.speed = SimSpeed::Normal;
    projected
}

fn configure_domain_work(app: &mut App, speed: SimSpeed, target: u64) {
    let mut driver = app
        .world_mut()
        .resource_mut::<rogue_core::simulation::SimulationDriverState>();
    driver.driver.clock.set_speed(speed);
    driver
        .request_advance(target)
        .expect("domain request should be valid");
    driver
        .enqueue_work(
            sim_core::Cadence::Day,
            5,
            2,
            DomainWorkId::new(3).expect("domain work id"),
        )
        .expect("day work should be valid");
    driver
        .enqueue_work(
            sim_core::Cadence::Hour,
            10,
            1,
            DomainWorkId::new(2).expect("domain work id"),
        )
        .expect("hour work should be valid");
    driver
        .enqueue_work(
            sim_core::Cadence::Minute,
            10,
            0,
            DomainWorkId::new(1).expect("domain work id"),
        )
        .expect("minute work should be valid");
}

fn restore_app_from_snapshot(snapshot: &GameSnapshot) -> App {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    rogue_core::persistence::snapshot::restore_world(app.world_mut(), snapshot)
        .expect("restore snapshot");
    app
}

fn set_simulation_budget(
    app: &mut App,
    maximum_steps_per_frame: usize,
    maximum_domain_events_per_frame: usize,
) {
    app.world_mut().insert_resource(SimulationWorkBudget {
        maximum_steps_per_frame,
        maximum_domain_events_per_frame,
    });
}

fn drain_simulation(app: &mut App) {
    let _ = drain_simulation_count(app);
}

fn drain_simulation_count(app: &mut App) -> usize {
    let mut updates = 0;
    for _ in 0..4096 {
        let status = *app.world().resource::<SimulationStatus>();
        let active_domain = app
            .world()
            .resource::<rogue_core::simulation::SimulationDriverState>()
            .has_active_domain_request();
        if status != SimulationStatus::Resolving && !active_domain {
            return updates;
        }

        rogue_core::drive_simulation(app.world_mut());
        updates += 1;
    }

    let status = *app.world().resource::<SimulationStatus>();
    let state = app
        .world()
        .resource::<rogue_core::simulation::SimulationDriverState>();
    panic!(
        "simulation did not settle within the expected bound: status={status:?}, clock={}, request={:?}, pending={:?}, backlog={}",
        state.driver.clock.minute,
        state.request.target_minute(),
        state.driver.pending_target_minute(),
        state.driver.backlog.entries().len()
    );
}

fn action_outcome_log_summary(app: &App) -> String {
    format!(
        "{:?}",
        app.world()
            .resource::<rogue_core::action::resolver::ActionOutcomeLog>()
            .outcomes
    )
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
fn pre_spawned_unrelated_entities_do_not_change_the_replay_digest() {
    let fixture = load_fixture();
    let baseline = run_replay(&fixture);
    let perturbed = run_replay_with_pre_spawned_unrelated_entity(&fixture);

    assert_eq!(baseline, perturbed);
    assert_eq!(
        snapshot_digest(&baseline).expect("baseline digest"),
        snapshot_digest(&perturbed).expect("perturbed digest")
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
fn snapshot_roundtrip_continues_a_partially_processed_simulation() {
    let fixture = load_fixture();
    let command = fixture
        .commands
        .first()
        .cloned()
        .expect("fixture must contain at least one command");

    let mut interrupted = App::new();
    interrupted.add_plugins(SimulationPlugin);
    initialize_world(&mut interrupted, fixture.seed);
    set_simulation_budget(&mut interrupted, 1, 1);

    drive_command(&mut interrupted, &command);

    let interrupted_snapshot = snapshot_world(interrupted.world()).expect("interrupted snapshot");

    let mut resumed = restore_app_from_snapshot(&interrupted_snapshot);
    set_simulation_budget(&mut interrupted, 1_024, 1_024);
    set_simulation_budget(&mut resumed, 1_024, 1_024);

    rogue_core::drive_simulation(interrupted.world_mut());
    rogue_core::drive_simulation(resumed.world_mut());

    let interrupted_snapshot = snapshot_world(interrupted.world()).expect("interrupted snapshot");
    let resumed_snapshot = snapshot_world(resumed.world()).expect("resumed snapshot");

    assert_eq!(interrupted_snapshot, resumed_snapshot);
    assert_eq!(
        snapshot_digest(&resumed_snapshot).expect("resumed digest"),
        snapshot_digest(&interrupted_snapshot).expect("interrupted digest")
    );
}

#[test]
fn replay_is_invariant_across_frame_budgets() {
    let fixture = load_fixture();
    let (slow, slow_log) = run_replay_with_budget_metrics(&fixture, 1, 1_024);
    let (fast, fast_log) = run_replay_with_budget_metrics(&fixture, 1_024, 1_024);

    assert_eq!(slow, fast);
    assert_eq!(
        snapshot_digest(&slow).expect("slow digest"),
        snapshot_digest(&fast).expect("fast digest")
    );
    assert_eq!(slow_log, fast_log);
}

#[test]
fn replay_is_invariant_across_simulation_speeds() {
    let fixture = load_fixture();
    let normal = run_replay_with_speed(&fixture, SimSpeed::Normal);
    let very_fast = run_replay_with_speed(&fixture, SimSpeed::VeryFast);

    assert_eq!(
        projected_snapshot_without_speed(&normal),
        projected_snapshot_without_speed(&very_fast)
    );
    assert_eq!(
        snapshot_digest(&projected_snapshot_without_speed(&normal)).expect("normal digest"),
        snapshot_digest(&projected_snapshot_without_speed(&very_fast)).expect("very fast digest")
    );
}

#[test]
fn replay_speed_changes_update_counts_without_changing_results() {
    let fixture = load_fixture();
    let (normal, normal_updates, normal_log) =
        run_replay_with_speed_metrics(&fixture, SimSpeed::Normal);
    let (fast, fast_updates, fast_log) = run_replay_with_speed_metrics(&fixture, SimSpeed::Fast);
    let (very_fast, very_fast_updates, very_fast_log) =
        run_replay_with_speed_metrics(&fixture, SimSpeed::VeryFast);

    assert!(normal_updates > fast_updates);
    assert!(fast_updates > very_fast_updates);
    assert_eq!(
        projected_snapshot_without_speed(&normal),
        projected_snapshot_without_speed(&fast)
    );
    assert_eq!(
        projected_snapshot_without_speed(&fast),
        projected_snapshot_without_speed(&very_fast)
    );
    assert_eq!(normal_log, fast_log);
    assert_eq!(fast_log, very_fast_log);
}

#[test]
fn future_backlog_remains_dormant_after_the_current_request_completes() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);

    {
        let mut state = app
            .world_mut()
            .resource_mut::<rogue_core::simulation::SimulationDriverState>();
        state.driver.clock.set_speed(SimSpeed::VeryFast);
        state
            .request_advance(5)
            .expect("initial request should be valid");
        state
            .enqueue_work(
                sim_core::Cadence::Day,
                5,
                2,
                DomainWorkId::new(3).expect("domain work id"),
            )
            .expect("day work should be valid");
        state
            .enqueue_work(
                sim_core::Cadence::Hour,
                10,
                1,
                DomainWorkId::new(2).expect("domain work id"),
            )
            .expect("hour work should be valid");
    }

    rogue_core::drive_simulation(app.world_mut());
    let after_first_request = snapshot_world(app.world()).expect("after first request");
    assert_eq!(after_first_request.simulation_driver.driver.clock.minute, 5);
    assert!(
        after_first_request
            .simulation_driver
            .request
            .target_minute()
            .is_none()
    );
    assert_eq!(
        after_first_request
            .simulation_driver
            .driver
            .backlog
            .entries()
            .len(),
        1
    );

    let dormant_snapshot = snapshot_world(app.world()).expect("dormant snapshot");
    rogue_core::drive_simulation(app.world_mut());
    let after_dormant_drive = snapshot_world(app.world()).expect("after dormant drive");
    assert_eq!(dormant_snapshot, after_dormant_drive);

    {
        let mut state = app
            .world_mut()
            .resource_mut::<rogue_core::simulation::SimulationDriverState>();
        state
            .request_advance(10)
            .expect("second request should be valid");
    }

    drain_simulation(&mut app);

    let final_snapshot = snapshot_world(app.world()).expect("final snapshot");
    assert_eq!(final_snapshot.simulation_driver.driver.clock.minute, 10);
    assert!(
        final_snapshot
            .simulation_driver
            .request
            .target_minute()
            .is_none()
    );
    assert!(final_snapshot.simulation_driver.driver.backlog.is_empty());
    assert_eq!(
        final_snapshot
            .simulation_driver
            .event_log
            .iter()
            .map(|event| event.id.raw())
            .collect::<Vec<_>>(),
        vec![3, 2]
    );
}

#[test]
fn combined_tactical_and_domain_work_share_the_step_budget() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);
    configure_domain_work(&mut app, SimSpeed::VeryFast, 40);
    set_simulation_budget(&mut app, 1, 8);

    drive_command(&mut app, &ActionKindSnapshot::Wait);

    assert_eq!(
        app.world()
            .resource::<rogue_core::simulation::SimulationDriverState>()
            .event_log
            .len(),
        0
    );
    assert!(app.world().resource::<ActionQueue>().actions.is_empty());
}

#[test]
fn current_minute_requests_process_all_due_work_without_losing_the_request() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);

    {
        let mut state = app
            .world_mut()
            .resource_mut::<rogue_core::simulation::SimulationDriverState>();
        state.driver.clock.set_speed(SimSpeed::VeryFast);
        state
            .request_advance(0)
            .expect("current-minute request should be valid");
        state
            .enqueue_work(
                sim_core::Cadence::Minute,
                0,
                0,
                DomainWorkId::new(1).expect("domain work id"),
            )
            .expect("minute work should be valid");
        state
            .enqueue_work(
                sim_core::Cadence::Hour,
                0,
                1,
                DomainWorkId::new(2).expect("domain work id"),
            )
            .expect("hour work should be valid");
    }

    set_simulation_budget(&mut app, 1, 1);
    rogue_core::drive_simulation(app.world_mut());

    let interrupted = snapshot_world(app.world()).expect("interrupted snapshot");
    assert_eq!(interrupted.simulation_driver.driver.clock.minute, 0);
    assert!(
        interrupted
            .simulation_driver
            .request
            .target_minute()
            .is_some()
    );
    assert_eq!(interrupted.simulation_driver.event_log.len(), 1);

    set_simulation_budget(&mut app, 1, 1);
    drain_simulation(&mut app);

    let final_snapshot = snapshot_world(app.world()).expect("final snapshot");
    assert_eq!(final_snapshot.simulation_driver.driver.clock.minute, 0);
    assert!(
        final_snapshot
            .simulation_driver
            .request
            .target_minute()
            .is_none()
    );
    assert!(final_snapshot.simulation_driver.driver.backlog.is_empty());
    assert_eq!(
        final_snapshot
            .simulation_driver
            .event_log
            .iter()
            .map(|event| event.id.raw())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn request_rejection_and_snapshot_request_window_invariants() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);

    {
        let mut state = app
            .world_mut()
            .resource_mut::<rogue_core::simulation::SimulationDriverState>();
        state.driver.clock.minute = 10;
        assert!(matches!(
            state.request_advance(9),
            Err(
                rogue_core::simulation::DomainAdvanceError::TargetPrecedesClock {
                    clock_minute: 10,
                    requested_minute: 9
                }
            )
        ));

        state
            .request_advance(15)
            .expect("request should be accepted");
        assert!(matches!(
            state.request_advance(20),
            Err(
                rogue_core::simulation::DomainAdvanceError::RequestAlreadyActive {
                    active_target: 15,
                    requested_minute: 20
                }
            )
        ));
        state.driver.set_pending_target_minute(Some(20));
    }

    let err = snapshot_world(app.world()).expect_err("invalid request window");
    assert!(err.contains("pending target cannot exceed"));
}

#[test]
fn invalid_domain_work_is_rejected_before_any_authoritative_mutation() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    for declared_cost in [0, 2] {
        let mut app = App::new();
        app.add_plugins(SimulationPlugin);
        initialize_world(&mut app, 0);

        {
            let mut state = app
                .world_mut()
                .resource_mut::<rogue_core::simulation::SimulationDriverState>();
            state.driver.clock.set_speed(SimSpeed::VeryFast);
            state.request_advance(0).expect("request should be valid");
            state.event_log.push(DomainWorkEvent {
                cadence: sim_core::Cadence::Minute,
                due_minute: 0,
                sequence: 0,
                id: DomainWorkId::new(11).expect("domain work id"),
            });
            state.driver.enqueue(sim_core::DueWork {
                cadence: sim_core::Cadence::Minute,
                due_minute: 0,
                sequence: 1,
                id: DomainWorkId::new(7).expect("domain work id"),
                domain_event_cost: declared_cost,
            });
        }

        let before = {
            let state = app
                .world()
                .resource::<rogue_core::simulation::SimulationDriverState>();
            state.clone()
        };
        let result = catch_unwind(AssertUnwindSafe(|| {
            rogue_core::drive_simulation(app.world_mut())
        }));

        assert!(
            result.is_err(),
            "declared cost {declared_cost} should be rejected before execution"
        );
        let after = {
            let state = app
                .world()
                .resource::<rogue_core::simulation::SimulationDriverState>();
            state.clone()
        };
        assert_eq!(before, after);
    }
}

#[test]
fn enqueue_work_rejects_tactical_cadence() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);

    let mut state = app
        .world_mut()
        .resource_mut::<rogue_core::simulation::SimulationDriverState>();
    let result = state.enqueue_work(
        sim_core::Cadence::Tactical,
        0,
        0,
        DomainWorkId::new(77).expect("domain work id"),
    );

    assert!(matches!(
        result,
        Err(DomainWorkError::TacticalCadence {
            id
        }) if id.raw() == 77
    ));
}

#[test]
fn domain_work_is_paused_until_the_request_is_resumed() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);

    configure_domain_work(&mut app, SimSpeed::Paused, 40);

    let before = snapshot_world(app.world()).expect("before snapshot");
    rogue_core::drive_simulation(app.world_mut());
    let paused = snapshot_world(app.world()).expect("paused snapshot");

    assert_eq!(before, paused);

    {
        let mut driver = app
            .world_mut()
            .resource_mut::<rogue_core::simulation::SimulationDriverState>();
        driver.driver.clock.set_speed(SimSpeed::VeryFast);
    }

    drain_simulation(&mut app);

    let resumed = snapshot_world(app.world()).expect("resumed snapshot");
    assert!(resumed.simulation_driver.event_log.len() >= 3);
    assert!(resumed.simulation_driver.request.target_minute().is_none());
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
fn legacy_snapshot_v2_is_migrated() {
    let snapshot = snapshot_from_text(include_str!("fixtures/legacy_snapshot_v2.ron"))
        .expect("deserialize legacy v2 snapshot");
    let migrated = migrate_snapshot(snapshot).expect("migrate legacy v2 snapshot");

    assert_eq!(migrated, run_replay(&load_fixture()));
}

#[test]
fn domain_work_roundtrip_preserves_partial_progress_and_event_order() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);
    {
        let mut state = app
            .world_mut()
            .resource_mut::<rogue_core::simulation::SimulationDriverState>();
        state.driver.clock.set_speed(SimSpeed::VeryFast);
        state
            .request_advance(10)
            .expect("domain request should be valid");
        state
            .enqueue_work(
                sim_core::Cadence::Minute,
                10,
                0,
                DomainWorkId::new(1).expect("domain work id"),
            )
            .expect("minute work should be valid");
        state
            .enqueue_work(
                sim_core::Cadence::Hour,
                10,
                1,
                DomainWorkId::new(2).expect("domain work id"),
            )
            .expect("hour work should be valid");
    }
    set_simulation_budget(&mut app, 1, 1);

    rogue_core::drive_simulation(app.world_mut());

    let interrupted_snapshot = snapshot_world(app.world()).expect("interrupted snapshot");
    let mut restored_app = restore_app_from_snapshot(&interrupted_snapshot);

    set_simulation_budget(&mut app, 1_024, 1_024);
    set_simulation_budget(&mut restored_app, 1_024, 1_024);

    drain_simulation(&mut app);
    drain_simulation(&mut restored_app);

    let original_final = snapshot_world(app.world()).expect("original final snapshot");
    let restored_final = snapshot_world(restored_app.world()).expect("restored final snapshot");

    assert_eq!(original_final, restored_final);
    assert_eq!(
        snapshot_digest(&original_final).expect("original final digest"),
        snapshot_digest(&restored_final).expect("restored final digest")
    );
    assert_eq!(
        original_final.simulation_driver.event_log,
        restored_final.simulation_driver.event_log
    );
}

#[test]
fn domain_work_orders_by_due_minute_then_cadence_then_sequence() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);
    configure_domain_work(&mut app, SimSpeed::VeryFast, 40);

    drain_simulation(&mut app);

    let snapshot = snapshot_world(app.world()).expect("domain snapshot");
    let processed_ids: Vec<_> = snapshot
        .simulation_driver
        .event_log
        .iter()
        .map(|event| event.id.raw())
        .collect();

    assert_eq!(processed_ids, vec![3, 1, 2]);
}

#[test]
fn version_two_current_shape_is_upgraded() {
    let snapshot = run_replay(&load_fixture());
    let mut legacy_shape = snapshot.clone();
    legacy_shape.version = 2;

    let migrated = migrate_snapshot(rogue_core::persistence::migration::SnapshotFile::Current(
        legacy_shape,
    ))
    .expect("migrate version two current shape");

    assert_eq!(migrated.version, CURRENT_SAVE_VERSION);
    assert_eq!(migrated, snapshot);
}

#[test]
fn snapshot_roundtrip_preserves_non_tactical_driver_backlog() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);

    {
        let mut driver = app
            .world_mut()
            .resource_mut::<rogue_core::simulation::SimulationDriverState>();
        driver
            .enqueue_work(
                sim_core::Cadence::Hour,
                12,
                99,
                DomainWorkId::new(5).expect("domain work id"),
            )
            .expect("hour work should be valid");
    }

    let snapshot = snapshot_world(app.world()).expect("snapshot");
    let text = snapshot_to_text(&snapshot).expect("serialize");
    let restored = match snapshot_from_text(&text).expect("deserialize") {
        rogue_core::persistence::migration::SnapshotFile::Current(snapshot) => snapshot,
        rogue_core::persistence::migration::SnapshotFile::V1(_) => {
            panic!("snapshot should not downgrade")
        }
        rogue_core::persistence::migration::SnapshotFile::V2(_) => {
            panic!("snapshot should not downgrade")
        }
    };

    assert_eq!(snapshot, restored);
}

#[test]
fn replay_advances_all_authoritative_rng_streams() {
    let fixture = load_fixture();
    let snapshot = run_replay(&fixture);
    let initial = RandomStreams::seeded(fixture.seed).snapshot();

    assert_ne!(snapshot.rng.generation_state, initial.generation_state);
    assert_ne!(snapshot.rng.loot_state, initial.loot_state);
}

#[test]
fn continuation_after_restore_matches_the_original_world() {
    let fixture = load_fixture();
    let split = fixture.commands.len().saturating_sub(2);
    let mut original_app = App::new();
    original_app.add_plugins(SimulationPlugin);
    initialize_world(&mut original_app, fixture.seed);
    for command in &fixture.commands[..split] {
        drive_command(&mut original_app, command);
        drain_simulation(&mut original_app);
    }

    let prefix_snapshot = snapshot_world(original_app.world()).expect("prefix snapshot");
    let prefix_text = snapshot_to_text(&prefix_snapshot).expect("serialize prefix");
    let restored_snapshot = match snapshot_from_text(&prefix_text).expect("deserialize prefix") {
        rogue_core::persistence::migration::SnapshotFile::Current(snapshot) => snapshot,
        rogue_core::persistence::migration::SnapshotFile::V2(_) => {
            panic!("prefix snapshot unexpectedly downgraded")
        }
        rogue_core::persistence::migration::SnapshotFile::V1(_) => {
            panic!("prefix snapshot unexpectedly downgraded")
        }
    };

    assert_eq!(prefix_snapshot, restored_snapshot);
    assert_eq!(
        snapshot_digest(&prefix_snapshot).expect("prefix digest"),
        snapshot_digest(&restored_snapshot).expect("restored digest")
    );

    let mut restored_app = App::new();
    restored_app.add_plugins(SimulationPlugin);
    rogue_core::persistence::snapshot::restore_world(restored_app.world_mut(), &restored_snapshot)
        .expect("restore prefix snapshot");

    for command in &fixture.commands[split..] {
        drive_command(&mut original_app, command);
        drain_simulation(&mut original_app);
        drive_command(&mut restored_app, command);
        drain_simulation(&mut restored_app);
    }

    let original_final = snapshot_world(original_app.world()).expect("original final snapshot");
    let restored_final = snapshot_world(restored_app.world()).expect("restored final snapshot");

    assert_eq!(original_final, restored_final);
    assert_eq!(
        snapshot_digest(&original_final).expect("original final digest"),
        snapshot_digest(&restored_final).expect("restored final digest")
    );
}

#[test]
fn presentation_rng_consumption_does_not_change_the_authoritative_replay() {
    let fixture = load_fixture();
    let baseline = run_replay(&fixture);

    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, fixture.seed);
    let mut presentation_rng = PresentationRng::seeded(fixture.seed);

    let monster = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<(&PersistentId, &StableActorId), With<Monster>>();
        let (_, stable_id) = query.iter(world).next().expect("monster entity");
        stable_id.0
    };
    app.world_mut()
        .resource_mut::<ActionQueue>()
        .actions
        .clear();
    *app.world_mut().resource_mut::<CurrentActor>() =
        sim_core::schedule::CurrentActor(Some(monster));
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;
    app.world_mut()
        .run_system_once(rogue_core::actor::ai::generate_ai_action)
        .expect("run ai system");
    rogue_core::drive_simulation(app.world_mut());
    drain_simulation(&mut app);

    for command in &fixture.commands {
        let _ = presentation_rng.next_u64();
        drive_command(&mut app, command);
        drain_simulation(&mut app);
    }

    let with_presentation_noise = snapshot_world(app.world()).expect("snapshot should be valid");

    assert_eq!(baseline, with_presentation_noise);
    assert_eq!(
        snapshot_digest(&baseline).expect("baseline digest"),
        snapshot_digest(&with_presentation_noise).expect("noisy digest")
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
        ("simulation driver", "missing simulation driver resource"),
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
            "simulation driver" => {
                app.world_mut()
                    .remove_resource::<rogue_core::simulation::SimulationDriverState>();
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

    let mut invalid_domain_target = base.clone();
    invalid_domain_target.simulation_driver.driver.clock.minute = 10;
    invalid_domain_target
        .simulation_driver
        .driver
        .set_pending_target_minute(Some(5));
    cases.push((
        "invalid domain target",
        invalid_domain_target,
        "pending target cannot precede",
    ));

    let mut pending_without_request = base.clone();
    pending_without_request
        .simulation_driver
        .driver
        .set_pending_target_minute(Some(5));
    cases.push((
        "pending without request",
        pending_without_request,
        "requires an active request",
    ));

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

    let mut tactical_event_log = base.clone();
    tactical_event_log
        .simulation_driver
        .event_log
        .push(DomainWorkEvent {
            cadence: sim_core::Cadence::Tactical,
            due_minute: 0,
            sequence: 0,
            id: DomainWorkId::new(900).expect("domain work id"),
        });
    cases.push((
        "tactical event log",
        tactical_event_log,
        "event log must not contain tactical work",
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
    cases.push(("inventory references non-item", non_item, "missing item"));

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
fn duplicate_stable_actor_ids_are_rejected() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    initialize_world(&mut app, 0);

    let player_id = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<Player>>();
        query.iter(world).next().expect("player entity")
    };
    let monster_id = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<Monster>>();
        query.iter(world).next().expect("monster entity")
    };

    let player_stable_id = app
        .world()
        .entity(player_id)
        .get::<StableActorId>()
        .copied()
        .expect("player stable id");
    app.world_mut()
        .entity_mut(monster_id)
        .insert(player_stable_id);

    let err = snapshot_world(app.world()).expect_err("duplicate stable actor ids");
    assert!(
        err.contains("duplicate stable actor id"),
        "unexpected error: {}",
        err
    );
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
            let bonus_id = allocator
                .allocate()
                .expect("persistent id allocator exhausted");
            commands.spawn((
                Item,
                PrototypeId("bonus".to_string()),
                GridPosition {
                    level: LevelId(0),
                    cell: IVec2::new(1, 1),
                },
                bonus_id,
                StableItemId(rogue_core::ItemId::new(bonus_id.0).expect("valid item id")),
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
    build_stable_entity_index(app.world_mut());
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
        allocator
            .allocate()
            .expect("persistent id allocator exhausted")
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
        StableActorId(rogue_core::ActorId::new(player_id.0).expect("valid actor id")),
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

    build_stable_entity_index(app.world_mut());
    let snapshot = snapshot_world(app.world()).expect("snapshot");
    assert_eq!(snapshot.current_level, 7);
    let text = snapshot_to_text(&snapshot).expect("serialize");
    let restored = match snapshot_from_text(&text).expect("deserialize") {
        rogue_core::persistence::migration::SnapshotFile::Current(snapshot) => snapshot,
        rogue_core::persistence::migration::SnapshotFile::V2(_) => {
            panic!("nonzero level snapshot should not downgrade")
        }
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
        allocator
            .allocate()
            .expect("persistent id allocator exhausted")
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
            StableActorId(rogue_core::ActorId::new(player_id.0).expect("valid actor id")),
        ))
        .id();

    app.world_mut()
        .resource_mut::<EffectQueue>()
        .0
        .push_back(Effect::ApplyStatus {
            target: rogue_core::ActorId::new(player_id.0).expect("valid actor id"),
            status: StatusEffect::Poisoned { remaining: 3 },
        });
    app.world_mut()
        .resource_mut::<EffectQueue>()
        .0
        .push_back(Effect::ApplyStatus {
            target: rogue_core::ActorId::new(player_id.0).expect("valid actor id"),
            status: StatusEffect::Stunned { remaining: 2 },
        });

    build_stable_entity_index(app.world_mut());
    app.world_mut()
        .run_system_once(apply_pending_effects)
        .expect("apply effects");
    app.world_mut().flush();

    build_stable_entity_index(app.world_mut());
    let statuses = app
        .world()
        .entity(player)
        .get::<ActiveStatuses>()
        .expect("active statuses");
    let statuses_value = statuses.0.clone();
    assert_eq!(
        statuses.0,
        vec![
            StatusEffect::Poisoned { remaining: 3 },
            StatusEffect::Stunned { remaining: 2 },
        ]
    );

    build_stable_entity_index(app.world_mut());
    let snapshot = snapshot_world(app.world()).expect("snapshot");
    let text = snapshot_to_text(&snapshot).expect("serialize");
    let restored = match snapshot_from_text(&text).expect("deserialize") {
        rogue_core::persistence::migration::SnapshotFile::Current(snapshot) => snapshot,
        rogue_core::persistence::migration::SnapshotFile::V2(_) => {
            panic!("status snapshot should not downgrade")
        }
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
    assert_eq!(restored_statuses.0, statuses_value);
}

#[test]
fn select_next_actor_preserves_scheduled_work_when_the_index_is_stale() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    app.world_mut()
        .insert_resource(SimulationStatus::WaitingForPlayer);
    app.world_mut().insert_resource(CurrentActor::default());
    app.world_mut()
        .insert_resource(rogue_core::actor::components::StableEntityIndex::default());
    app.world_mut().insert_resource(TurnClock::default());
    app.world_mut().insert_resource(ActionQueue::default());

    let actor_id = rogue_core::ActorId::new(1).expect("valid actor id");
    let entity = app
        .world_mut()
        .spawn((
            Actor,
            Health {
                current: 4,
                maximum: 4,
            },
            StableActorId(actor_id),
        ))
        .id();

    app.world_mut()
        .resource_mut::<TurnClock>()
        .schedule_at(actor_id, 0);

    app.world_mut()
        .run_system_once(rogue_core::time::scheduler::select_next_actor)
        .expect("select next actor");

    assert_eq!(
        app.world().resource::<SimulationStatus>(),
        &SimulationStatus::WaitingForPlayer
    );
    assert!(app.world().resource::<CurrentActor>().0.is_none());
    assert_eq!(
        app.world()
            .resource::<TurnClock>()
            .peek_next()
            .map(|entry| entry.actor),
        Some(actor_id)
    );

    {
        let mut index = app
            .world_mut()
            .resource_mut::<rogue_core::actor::components::StableEntityIndex>();
        index.insert_actor(actor_id, entity);
    }

    app.world_mut()
        .run_system_once(rogue_core::time::scheduler::select_next_actor)
        .expect("select next actor");

    assert_eq!(
        app.world().resource::<SimulationStatus>(),
        &SimulationStatus::Resolving
    );
    assert_eq!(app.world().resource::<CurrentActor>().0, Some(actor_id));
}

#[test]
fn spatial_index_orders_occupants_by_stable_identity() {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    let level = LevelId(0);
    let cell = IVec2::new(2, 2);

    let first = app
        .world_mut()
        .spawn((
            Actor,
            BlocksMovement,
            Health {
                current: 3,
                maximum: 3,
            },
            GridPosition { level, cell },
            StableActorId(rogue_core::ActorId::new(2).expect("valid actor id")),
        ))
        .id();
    let second = app
        .world_mut()
        .spawn((
            Actor,
            BlocksMovement,
            Health {
                current: 3,
                maximum: 3,
            },
            GridPosition { level, cell },
            StableActorId(rogue_core::ActorId::new(1).expect("valid actor id")),
        ))
        .id();
    let first_stable = app.world().entity(first).get::<StableActorId>().copied();
    let second_stable = app.world().entity(second).get::<StableActorId>().copied();

    let mut spatial = SpatialIndex::default();
    spatial.insert_occupant(
        first,
        GridPosition { level, cell },
        first_stable.as_ref(),
        None,
        None,
        true,
        false,
    );
    spatial.insert_occupant(
        second,
        GridPosition { level, cell },
        second_stable.as_ref(),
        None,
        None,
        true,
        false,
    );

    let occupants: Vec<_> = spatial.occupants_at(level, cell).collect();
    assert_eq!(occupants, vec![second, first]);
}

#[test]
#[should_panic(expected = "unstable occupant cannot block movement or sight")]
fn unstable_blockers_are_rejected() {
    let mut spatial = SpatialIndex::default();
    let level = LevelId(0);
    let cell = IVec2::new(4, 4);
    let first = Entity::from_bits(1);
    let second = Entity::from_bits(2);

    spatial.insert_occupant(
        first,
        GridPosition { level, cell },
        None,
        None,
        None,
        true,
        false,
    );
    spatial.insert_occupant(
        second,
        GridPosition { level, cell },
        None,
        None,
        None,
        true,
        false,
    );

    let _ = spatial.occupants_at(level, cell).collect::<Vec<_>>();
}

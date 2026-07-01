use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use tactical_sim::action::intent::{Action, ActionKind};
use tactical_sim::action::queue::ActionQueue;
use tactical_sim::action::resolver::{ActionFailure, ActionOutcome, ActionOutcomeLog};
use tactical_sim::actor::components::*;
use tactical_sim::item::components::{CarriedBy, Inventory, Item};
use tactical_sim::item::effects::EffectQueue;
use tactical_sim::simulation::{
    SimulationPlugin, SimulationStatus, SimulationStep, drive_simulation,
};
use tactical_sim::time::clock::CurrentActor;
use tactical_sim::time::clock::TurnClock;
use tactical_sim::world::generation::generate_one_room;
use tactical_sim::world::map::{GridPosition, LevelId};
use tactical_sim::world::spatial::SpatialIndex;

fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    app
}

macro_rules! schedule_actor {
    ($app:expr, $entity:expr, $tick:expr) => {{
        let actor = actor_id($app.world(), $entity);
        $app.world_mut()
            .resource_mut::<TurnClock>()
            .schedule_at(actor, $tick);
    }};
}

macro_rules! push_action {
    ($app:expr, $entity:expr, $kind:expr) => {{
        let actor = actor_id($app.world(), $entity);
        let kind = $kind;
        $app.world_mut()
            .resource_mut::<ActionQueue>()
            .push(Action { actor, kind });
    }};
}

fn tag_actor(world: &mut World, entity: Entity) -> ActorId {
    let id = ActorId::new(entity.to_bits()).expect("valid actor id");
    world.entity_mut(entity).insert(StableActorId(id));
    id
}

fn tag_item(world: &mut World, entity: Entity) -> ItemId {
    let id = ItemId::new(entity.to_bits()).expect("valid item id");
    world.entity_mut(entity).insert(StableItemId(id));
    id
}

fn actor_id(world: &World, entity: Entity) -> ActorId {
    let _ = world;
    ActorId::new(entity.to_bits()).expect("valid actor id")
}

fn item_id(world: &World, entity: Entity) -> ItemId {
    let _ = world;
    ItemId::new(entity.to_bits()).expect("valid item id")
}

fn register_spatial_occupant(
    spatial: &mut SpatialIndex,
    entity: Entity,
    level: LevelId,
    cell: IVec2,
    stable_actor: Option<StableActorId>,
    stable_item: Option<StableItemId>,
    persistent_id: Option<PersistentId>,
    blocks_movement: bool,
    blocks_sight: bool,
) {
    spatial.insert_occupant(
        entity,
        GridPosition { level, cell },
        stable_actor.as_ref(),
        stable_item.as_ref(),
        persistent_id.as_ref(),
        blocks_movement,
        blocks_sight,
    );
}

fn spawn_test_world(app: &mut App) -> (Entity, Entity) {
    let level = LevelId(0);
    app.world_mut().insert_resource(generate_one_room(7, 7));
    let mut spatial = SpatialIndex::default();
    app.world_mut().insert_resource(EffectQueue::default());
    app.world_mut().insert_resource(ActionQueue::default());
    app.world_mut().insert_resource(SimulationStatus::Resolving);

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
            Inventory::new(4),
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
            GridPosition {
                level,
                cell: IVec2::new(2, 2),
            },
        ))
        .id();
    tag_actor(app.world_mut(), player);

    let monster = app
        .world_mut()
        .spawn((
            Actor,
            Monster,
            BlocksMovement,
            BlocksSight,
            HostileToPlayer,
            Health {
                current: 10,
                maximum: 10,
            },
            ActiveStatuses::default(),
            CombatStats {
                power: 1,
                defense: 0,
            },
            Vision { range: 8 },
            ActionSpeed {
                ticks_per_action: 120,
            },
            PrototypeId("rat".to_string()),
            GridPosition {
                level,
                cell: IVec2::new(3, 2),
            },
        ))
        .id();
    tag_actor(app.world_mut(), monster);

    let player_stable = app.world().entity(player).get::<StableActorId>().copied();
    let monster_stable = app.world().entity(monster).get::<StableActorId>().copied();
    register_spatial_occupant(
        &mut spatial,
        player,
        level,
        IVec2::new(2, 2),
        player_stable,
        None,
        app.world().entity(player).get::<PersistentId>().copied(),
        true,
        true,
    );
    register_spatial_occupant(
        &mut spatial,
        monster,
        level,
        IVec2::new(3, 2),
        monster_stable,
        None,
        app.world().entity(monster).get::<PersistentId>().copied(),
        true,
        true,
    );
    app.world_mut().insert_resource(spatial);
    build_stable_entity_index(app.world_mut());

    (player, monster)
}

fn build_stable_entity_index(world: &mut World) {
    let mut index = StableEntityIndex::default();
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

#[test]
fn bumping_into_an_enemy_converts_to_damage() {
    let mut app = build_app();
    let (player, monster) = spawn_test_world(&mut app);

    schedule_actor!(app, player, 0);
    push_action!(
        app,
        player,
        ActionKind::Move {
            delta: IVec2::new(1, 0)
        }
    );

    app.world_mut().run_schedule(SimulationStep);

    let health = app.world().entity(monster).get::<Health>().unwrap();
    assert!(health.current < health.maximum);
}

#[test]
fn waiting_player_turn_is_preserved_until_action_arrives() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);

    schedule_actor!(app, player, 0);

    app.world_mut().run_schedule(SimulationStep);

    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );
    assert_eq!(
        app.world().resource::<CurrentActor>().0,
        Some(actor_id(app.world(), player))
    );

    push_action!(app, player, ActionKind::Wait);
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;

    app.world_mut().run_schedule(SimulationStep);

    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );
    assert!(app.world().resource::<CurrentActor>().0.is_none());
}

#[test]
fn stale_scheduled_actor_is_skipped_before_the_next_valid_actor() {
    let mut app = build_app();
    let (player, monster) = spawn_test_world(&mut app);

    app.world_mut().entity_mut(monster).insert(Health {
        current: 0,
        maximum: 4,
    });

    schedule_actor!(app, monster, 0);
    schedule_actor!(app, player, 0);

    app.world_mut().run_schedule(SimulationStep);

    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );
    assert_eq!(
        app.world().resource::<CurrentActor>().0,
        Some(actor_id(app.world(), player))
    );
}

#[test]
fn drive_simulation_keeps_a_live_scheduled_actor_when_the_index_starts_stale() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);
    app.world_mut()
        .insert_resource(tactical_sim::actor::components::StableEntityIndex::default());
    schedule_actor!(app, player, 0);
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;

    drive_simulation(app.world_mut());

    assert_eq!(
        app.world().resource::<SimulationStatus>(),
        &SimulationStatus::WaitingForPlayer
    );
    assert_eq!(app.world().resource::<CurrentActor>().0, None);
    assert_eq!(
        app.world()
            .resource::<TurnClock>()
            .peek_next()
            .map(|entry| entry.actor.raw()),
        Some(actor_id(app.world(), player).raw())
    );
}

#[test]
fn prequeued_player_action_keeps_the_simulation_resolving() {
    let mut app = build_app();
    let (player, monster) = spawn_test_world(&mut app);

    schedule_actor!(app, monster, 0);
    schedule_actor!(app, player, 50);
    push_action!(app, player, ActionKind::Wait);

    app.world_mut().run_schedule(SimulationStep);

    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::Resolving
    );
    assert!(
        app.world()
            .resource::<ActionQueue>()
            .contains_actor(actor_id(app.world(), player))
    );
    assert!(app.world().resource::<CurrentActor>().0.is_none());
}

#[test]
fn actions_for_other_actors_are_preserved_through_their_turn() {
    let mut app = build_app();
    let (player, monster) = spawn_test_world(&mut app);

    schedule_actor!(app, player, 0);
    schedule_actor!(app, monster, 0);

    push_action!(app, monster, ActionKind::Wait);
    push_action!(app, player, ActionKind::Wait);

    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;
    drive_simulation(app.world_mut());

    let queue = app.world().resource::<ActionQueue>();
    assert!(queue.is_empty());
    assert!(matches!(
        app.world().resource::<ActionOutcomeLog>().latest(),
        Some(ActionOutcome::Resolved(action))
            if action.actor == actor_id(app.world(), monster)
    ));
}

#[test]
fn moving_over_an_item_does_not_damage_it() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);
    schedule_actor!(app, player, 0);
    let item = app
        .world_mut()
        .spawn((
            Item,
            Health {
                current: 5,
                maximum: 5,
            },
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(2, 3),
            },
        ))
        .id();
    tag_item(app.world_mut(), item);

    {
        let item_stable = app.world().entity(item).get::<StableItemId>().copied();
        let item_persistent = app.world().entity(item).get::<PersistentId>().copied();
        let mut spatial = app.world_mut().resource_mut::<SpatialIndex>();
        register_spatial_occupant(
            &mut spatial,
            item,
            LevelId(0),
            IVec2::new(2, 3),
            None,
            item_stable,
            item_persistent,
            false,
            false,
        );
    }

    push_action!(
        app,
        player,
        ActionKind::Move {
            delta: IVec2::new(0, 1)
        }
    );

    app.world_mut().run_schedule(SimulationStep);

    let item_health = app.world().entity(item).get::<Health>().unwrap();
    assert_eq!(item_health.current, 5);

    let player_position = app.world().entity(player).get::<GridPosition>().unwrap();
    assert_eq!(player_position.cell, IVec2::new(2, 3));
}

#[test]
fn moving_into_a_friendly_blocker_is_rejected() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);
    schedule_actor!(app, player, 0);
    let blocker = app
        .world_mut()
        .spawn((
            Actor,
            Health {
                current: 6,
                maximum: 6,
            },
            ActiveStatuses::default(),
            BlocksMovement,
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(2, 3),
            },
        ))
        .id();
    tag_actor(app.world_mut(), blocker);

    {
        let blocker_stable = app.world().entity(blocker).get::<StableActorId>().copied();
        let blocker_persistent = app.world().entity(blocker).get::<PersistentId>().copied();
        let mut spatial = app.world_mut().resource_mut::<SpatialIndex>();
        register_spatial_occupant(
            &mut spatial,
            blocker,
            LevelId(0),
            IVec2::new(2, 3),
            blocker_stable,
            None,
            blocker_persistent,
            true,
            false,
        );
    }

    push_action!(
        app,
        player,
        ActionKind::Move {
            delta: IVec2::new(0, 1)
        }
    );

    app.world_mut().run_schedule(SimulationStep);

    let blocker_health = app.world().entity(blocker).get::<Health>().unwrap();
    assert_eq!(blocker_health.current, 6);

    let player_position = app.world().entity(player).get::<GridPosition>().unwrap();
    assert_eq!(player_position.cell, IVec2::new(2, 2));
}

#[test]
fn non_hostile_actor_does_not_bump_the_player() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);
    let neutral = app
        .world_mut()
        .spawn((
            Actor,
            Monster,
            BlocksMovement,
            BlocksSight,
            Health {
                current: 5,
                maximum: 5,
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
            PrototypeId("neutral".to_string()),
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(1, 2),
            },
        ))
        .id();
    tag_actor(app.world_mut(), neutral);

    {
        let neutral_stable = app.world().entity(neutral).get::<StableActorId>().copied();
        let neutral_persistent = app.world().entity(neutral).get::<PersistentId>().copied();
        let mut spatial = app.world_mut().resource_mut::<SpatialIndex>();
        register_spatial_occupant(
            &mut spatial,
            neutral,
            LevelId(0),
            IVec2::new(1, 2),
            neutral_stable,
            None,
            neutral_persistent,
            true,
            true,
        );
    }

    build_stable_entity_index(app.world_mut());
    schedule_actor!(app, neutral, 0);
    push_action!(
        app,
        neutral,
        ActionKind::Move {
            delta: IVec2::new(1, 0)
        }
    );

    app.world_mut().run_schedule(SimulationStep);

    let player_health = app.world().entity(player).get::<Health>().unwrap();
    let neutral_position = app.world().entity(neutral).get::<GridPosition>().unwrap();

    assert_eq!(player_health.current, player_health.maximum);
    assert_eq!(neutral_position.cell, IVec2::new(1, 2));
}

#[test]
fn direct_melee_against_a_distant_target_fails_without_damage() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);
    schedule_actor!(app, player, 0);
    let target = app
        .world_mut()
        .spawn((
            Actor,
            Monster,
            BlocksMovement,
            BlocksSight,
            HostileToPlayer,
            Health {
                current: 9,
                maximum: 9,
            },
            ActiveStatuses::default(),
            CombatStats {
                power: 1,
                defense: 0,
            },
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(5, 5),
            },
        ))
        .id();
    tag_actor(app.world_mut(), target);
    build_stable_entity_index(app.world_mut());

    {
        let target_stable = app.world().entity(target).get::<StableActorId>().copied();
        let target_persistent = app.world().entity(target).get::<PersistentId>().copied();
        let mut spatial = app.world_mut().resource_mut::<SpatialIndex>();
        register_spatial_occupant(
            &mut spatial,
            target,
            LevelId(0),
            IVec2::new(5, 5),
            target_stable,
            None,
            target_persistent,
            true,
            true,
        );
    }

    push_action!(
        app,
        player,
        ActionKind::Melee {
            target: actor_id(app.world(), target)
        }
    );

    app.world_mut().run_schedule(SimulationStep);

    let target_health = app.world().entity(target).get::<Health>().unwrap();
    assert_eq!(target_health.current, 9);
    assert!(matches!(
        app.world().resource::<ActionOutcomeLog>().latest(),
        Some(ActionOutcome::Failed {
            failure: ActionFailure::OutOfRange,
            ..
        })
    ));
    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );
}

#[test]
fn unsupported_actions_report_an_explicit_failure() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);

    schedule_actor!(app, player, 0);
    push_action!(app, player, ActionKind::Descend);

    app.world_mut().run_schedule(SimulationStep);

    assert!(matches!(
        app.world().resource::<ActionOutcomeLog>().latest(),
        Some(ActionOutcome::Failed {
            failure: ActionFailure::Unsupported,
            ..
        })
    ));
    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );
}

#[test]
fn distant_pickup_does_not_mutate_inventory_or_item_state() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);
    let item = app
        .world_mut()
        .spawn((
            Item,
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(5, 5),
            },
        ))
        .id();
    tag_item(app.world_mut(), item);

    schedule_actor!(app, player, 0);
    push_action!(
        app,
        player,
        ActionKind::PickUp {
            item: item_id(app.world(), item)
        }
    );

    app.world_mut().run_schedule(SimulationStep);

    let inventory = app.world().entity(player).get::<Inventory>().unwrap();
    assert!(inventory.items.is_empty());
    assert!(app.world().entity(item).get::<CarriedBy>().is_none());
    assert_eq!(
        app.world().entity(item).get::<GridPosition>().unwrap().cell,
        IVec2::new(5, 5)
    );
    assert!(matches!(
        app.world().resource::<ActionOutcomeLog>().latest(),
        Some(ActionOutcome::Failed {
            failure: ActionFailure::InvalidTarget,
            ..
        })
    ));
}

#[test]
fn pickup_of_an_item_carried_by_someone_else_does_not_mutate_state() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);
    let owner = app
        .world_mut()
        .spawn((
            Actor,
            Monster,
            BlocksMovement,
            BlocksSight,
            ActiveStatuses::default(),
            Inventory::new(4),
            Health {
                current: 7,
                maximum: 7,
            },
            CombatStats {
                power: 2,
                defense: 0,
            },
            PrototypeId("owner".to_string()),
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(4, 4),
            },
        ))
        .id();
    tag_actor(app.world_mut(), owner);
    let owner_actor_id = actor_id(app.world(), owner);
    let item = app
        .world_mut()
        .spawn((
            Item,
            PrototypeId("trinket".to_string()),
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(4, 4),
            },
            CarriedBy(owner_actor_id),
        ))
        .id();
    tag_item(app.world_mut(), item);
    let item_stable_id = item_id(app.world(), item);

    {
        let mut owner_inventory = app.world_mut().entity_mut(owner);
        owner_inventory
            .get_mut::<Inventory>()
            .unwrap()
            .items
            .push(item_stable_id);
    }

    build_stable_entity_index(app.world_mut());
    schedule_actor!(app, player, 0);
    push_action!(
        app,
        player,
        ActionKind::PickUp {
            item: item_id(app.world(), item)
        }
    );

    app.world_mut().run_schedule(SimulationStep);

    let owner_inventory = app.world().entity(owner).get::<Inventory>().unwrap();
    let player_inventory = app.world().entity(player).get::<Inventory>().unwrap();
    assert_eq!(owner_inventory.items, vec![item_id(app.world(), item)]);
    assert!(player_inventory.items.is_empty());
    assert_eq!(
        app.world().entity(item).get::<CarriedBy>().unwrap().0,
        actor_id(app.world(), owner)
    );
    assert!(matches!(
        app.world().resource::<ActionOutcomeLog>().latest(),
        Some(ActionOutcome::Failed {
            failure: ActionFailure::MissingItem,
            ..
        })
    ));
}

#[test]
fn invalid_target_item_use_does_not_consume_or_despawn_the_item() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);
    let player_actor_id = actor_id(app.world(), player);
    let item = app
        .world_mut()
        .spawn((
            Item,
            PrototypeId("healing_potion".to_string()),
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(2, 2),
            },
            CarriedBy(player_actor_id),
        ))
        .id();
    tag_item(app.world_mut(), item);
    let item_stable_id = item_id(app.world(), item);
    {
        let mut inventory = app.world_mut().entity_mut(player);
        inventory
            .get_mut::<Inventory>()
            .unwrap()
            .items
            .push(item_stable_id);
    }

    build_stable_entity_index(app.world_mut());
    schedule_actor!(app, player, 0);
    push_action!(
        app,
        player,
        ActionKind::UseItem {
            item: item_id(app.world(), item),
            target: tactical_sim::action::intent::ActionTarget::Cell {
                level: LevelId(0),
                position: IVec2::new(5, 5)
            }
        }
    );

    app.world_mut().run_schedule(SimulationStep);

    let inventory = app.world().entity(player).get::<Inventory>().unwrap();
    assert_eq!(inventory.items, vec![item_id(app.world(), item)]);
    assert!(app.world().get_entity(item).is_ok());
    assert!(matches!(
        app.world().resource::<ActionOutcomeLog>().latest(),
        Some(ActionOutcome::Failed {
            failure: ActionFailure::InvalidTarget,
            ..
        })
    ));
}

#[test]
fn drive_simulation_preserves_a_failed_player_action_across_an_ai_turn() {
    let mut app = build_app();
    let (player, monster) = spawn_test_world(&mut app);

    schedule_actor!(app, player, 0);
    schedule_actor!(app, monster, 0);
    push_action!(app, player, ActionKind::Descend);
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;

    drive_simulation(app.world_mut());

    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );
    assert_eq!(
        app.world()
            .resource::<TurnClock>()
            .peek_next()
            .map(|next| next.actor),
        Some(actor_id(app.world(), player))
    );
    assert!(app.world().resource::<ActionQueue>().is_empty());

    let outcome_log = app.world().resource::<ActionOutcomeLog>();
    let outcomes = &outcome_log.outcomes;
    assert!(matches!(
        outcomes.front(),
        Some(ActionOutcome::Failed {
            failure: ActionFailure::Unsupported,
            ..
        })
    ));
    assert!(matches!(
        outcomes.back(),
        Some(ActionOutcome::Resolved(action))
            if action.actor == actor_id(app.world(), monster)
    ));
}

#[test]
fn actionless_non_player_is_skipped_without_rescheduling() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);
    let neutral = app
        .world_mut()
        .spawn((
            Actor,
            Monster,
            BlocksMovement,
            BlocksSight,
            Health {
                current: 5,
                maximum: 5,
            },
            CombatStats {
                power: 2,
                defense: 0,
            },
            Vision { range: 8 },
            ActionSpeed {
                ticks_per_action: 100,
            },
            PrototypeId("neutral".to_string()),
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(1, 2),
            },
        ))
        .id();
    tag_actor(app.world_mut(), neutral);

    {
        let neutral_stable = app.world().entity(neutral).get::<StableActorId>().copied();
        let neutral_persistent = app.world().entity(neutral).get::<PersistentId>().copied();
        let mut spatial = app.world_mut().resource_mut::<SpatialIndex>();
        register_spatial_occupant(
            &mut spatial,
            neutral,
            LevelId(0),
            IVec2::new(1, 2),
            neutral_stable,
            None,
            neutral_persistent,
            true,
            true,
        );
    }

    build_stable_entity_index(app.world_mut());
    schedule_actor!(app, neutral, 0);
    schedule_actor!(app, player, 50);
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;

    drive_simulation(app.world_mut());

    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );
    assert_eq!(
        app.world()
            .resource::<TurnClock>()
            .peek_next()
            .map(|next| next.actor),
        Some(actor_id(app.world(), player))
    );
}

#[test]
fn identical_input_sequences_produce_identical_state() {
    let mut first = build_app();
    let mut second = build_app();

    let first_entities = spawn_test_world(&mut first);
    let second_entities = spawn_test_world(&mut second);

    for app in [&mut first, &mut second] {
        let player = app
            .world()
            .iter_entities()
            .find_map(|entity_ref| entity_ref.get::<Player>().map(|_| entity_ref.id()))
            .expect("player");
        schedule_actor!(app, player, 0);
        push_action!(
            app,
            player,
            ActionKind::Move {
                delta: IVec2::new(1, 0)
            }
        );
        app.world_mut().run_schedule(SimulationStep);
    }

    fn signature(world: &World) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut entries: Vec<_> = world
            .iter_entities()
            .filter_map(|entity_ref| {
                let prototype = entity_ref.get::<PrototypeId>()?;
                let position = entity_ref.get::<GridPosition>()?;
                let health = entity_ref.get::<Health>().map(|h| (h.current, h.maximum));
                Some((
                    prototype.0.clone(),
                    health,
                    position.level.0,
                    position.cell.x,
                    position.cell.y,
                ))
            })
            .collect();
        entries.sort();

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        entries.hash(&mut hasher);
        hasher.finish()
    }

    assert_eq!(signature(first.world()), signature(second.world()));
    assert_ne!(first_entities.0, first_entities.1);
    assert_ne!(second_entities.0, second_entities.1);
}

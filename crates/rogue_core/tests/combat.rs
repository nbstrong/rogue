use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use rogue_core::action::intent::{Action, ActionKind};
use rogue_core::action::queue::ActionQueue;
use rogue_core::actor::components::*;
use rogue_core::item::components::Item;
use rogue_core::item::effects::EffectQueue;
use rogue_core::simulation::{SimulationPlugin, SimulationStatus, SimulationStep};
use rogue_core::time::clock::CurrentActor;
use rogue_core::time::clock::TurnClock;
use rogue_core::world::generation::generate_one_room;
use rogue_core::world::map::{GridPosition, LevelId};
use rogue_core::world::spatial::SpatialIndex;

fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    app
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

    let monster = app
        .world_mut()
        .spawn((
            Actor,
            Monster,
            BlocksMovement,
            BlocksSight,
            HostileToPlayer,
            Health {
                current: 4,
                maximum: 4,
            },
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

    spatial
        .occupants
        .insert((level, IVec2::new(2, 2)), vec![player]);
    spatial.movement_blockers.insert((level, IVec2::new(2, 2)));
    spatial.sight_blockers.insert((level, IVec2::new(2, 2)));
    spatial
        .occupants
        .insert((level, IVec2::new(3, 2)), vec![monster]);
    spatial.movement_blockers.insert((level, IVec2::new(3, 2)));
    spatial.sight_blockers.insert((level, IVec2::new(3, 2)));
    app.world_mut().insert_resource(spatial);

    (player, monster)
}

#[test]
fn bumping_into_an_enemy_converts_to_damage() {
    let mut app = build_app();
    let (player, monster) = spawn_test_world(&mut app);

    {
        let mut clock = app.world_mut().resource_mut::<TurnClock>();
        clock.schedule_at(player, 0);
    }

    app.world_mut().resource_mut::<ActionQueue>().push(Action {
        actor: player,
        kind: ActionKind::Move {
            delta: IVec2::new(1, 0),
        },
    });

    app.world_mut().run_schedule(SimulationStep);

    let health = app.world().entity(monster).get::<Health>().unwrap();
    assert!(health.current < health.maximum);
}

#[test]
fn waiting_player_turn_is_preserved_until_action_arrives() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);

    app.world_mut()
        .resource_mut::<TurnClock>()
        .schedule_at(player, 0);

    app.world_mut().run_schedule(SimulationStep);

    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );
    assert_eq!(app.world().resource::<CurrentActor>().0, Some(player));

    app.world_mut().resource_mut::<ActionQueue>().push(Action {
        actor: player,
        kind: ActionKind::Wait,
    });
    *app.world_mut().resource_mut::<SimulationStatus>() = SimulationStatus::Resolving;

    app.world_mut().run_schedule(SimulationStep);

    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
    );
    assert!(app.world().resource::<CurrentActor>().0.is_none());
}

#[test]
fn moving_over_an_item_does_not_damage_it() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);
    app.world_mut()
        .resource_mut::<TurnClock>()
        .schedule_at(player, 0);
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

    app.world_mut()
        .resource_mut::<SpatialIndex>()
        .occupants
        .entry((LevelId(0), IVec2::new(2, 3)))
        .or_default()
        .push(item);

    app.world_mut().resource_mut::<ActionQueue>().push(Action {
        actor: player,
        kind: ActionKind::Move {
            delta: IVec2::new(0, 1),
        },
    });

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
    app.world_mut()
        .resource_mut::<TurnClock>()
        .schedule_at(player, 0);
    let blocker = app
        .world_mut()
        .spawn((
            Actor,
            Health {
                current: 6,
                maximum: 6,
            },
            BlocksMovement,
            GridPosition {
                level: LevelId(0),
                cell: IVec2::new(2, 3),
            },
        ))
        .id();

    {
        let mut spatial = app.world_mut().resource_mut::<SpatialIndex>();
        spatial
            .occupants
            .entry((LevelId(0), IVec2::new(2, 3)))
            .or_default()
            .push(blocker);
        spatial
            .movement_blockers
            .insert((LevelId(0), IVec2::new(2, 3)));
    }

    app.world_mut().resource_mut::<ActionQueue>().push(Action {
        actor: player,
        kind: ActionKind::Move {
            delta: IVec2::new(0, 1),
        },
    });

    app.world_mut().run_schedule(SimulationStep);

    let blocker_health = app.world().entity(blocker).get::<Health>().unwrap();
    assert_eq!(blocker_health.current, 6);

    let player_position = app.world().entity(player).get::<GridPosition>().unwrap();
    assert_eq!(player_position.cell, IVec2::new(2, 2));
}

#[test]
fn direct_melee_against_a_distant_target_fails_without_damage() {
    let mut app = build_app();
    let (player, _monster) = spawn_test_world(&mut app);
    app.world_mut()
        .resource_mut::<TurnClock>()
        .schedule_at(player, 0);
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

    {
        let mut spatial = app.world_mut().resource_mut::<SpatialIndex>();
        spatial
            .occupants
            .entry((LevelId(0), IVec2::new(5, 5)))
            .or_default()
            .push(target);
        spatial
            .movement_blockers
            .insert((LevelId(0), IVec2::new(5, 5)));
        spatial
            .sight_blockers
            .insert((LevelId(0), IVec2::new(5, 5)));
    }

    app.world_mut().resource_mut::<ActionQueue>().push(Action {
        actor: player,
        kind: ActionKind::Melee { target },
    });

    app.world_mut().run_schedule(SimulationStep);

    let target_health = app.world().entity(target).get::<Health>().unwrap();
    assert_eq!(target_health.current, 9);
    assert_eq!(
        *app.world().resource::<SimulationStatus>(),
        SimulationStatus::WaitingForPlayer
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
        app.world_mut()
            .resource_mut::<TurnClock>()
            .schedule_at(player, 0);
        app.world_mut().resource_mut::<ActionQueue>().push(Action {
            actor: player,
            kind: ActionKind::Move {
                delta: IVec2::new(1, 0),
            },
        });
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

use bevy_app::App;
use bevy_math::IVec2;
use rogue_core::action::intent::{Action, ActionKind};
use rogue_core::action::queue::ActionQueue;
use rogue_core::actor::components::*;
use rogue_core::item::effects::EffectQueue;
use rogue_core::simulation::{SimulationPlugin, SimulationStatus, SimulationStep};
use rogue_core::time::clock::TurnClock;
use rogue_core::world::generation::generate_one_room;
use rogue_core::world::map::{GridPosition, LevelId};
use rogue_core::world::spatial::SpatialIndex;

fn run_replay() -> u64 {
    let mut app = App::new();
    app.add_plugins(SimulationPlugin);
    app.world_mut().insert_resource(generate_one_room(7, 7));
    app.world_mut().insert_resource(SpatialIndex::default());
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
                level: LevelId(0),
                cell: IVec2::new(2, 2),
            },
        ))
        .id();

    app.world_mut()
        .resource_mut::<TurnClock>()
        .schedule_at(player, 0);
    app.world_mut().resource_mut::<ActionQueue>().push(Action {
        actor: player,
        kind: ActionKind::Wait,
    });

    app.world_mut().run_schedule(SimulationStep);

    let mut entries: Vec<_> = app
        .world()
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

    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    entries.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn replay_hash_is_stable_for_the_same_sequence() {
    assert_eq!(run_replay(), run_replay());
}

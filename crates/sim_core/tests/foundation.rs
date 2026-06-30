use sim_core::identity::{ActorTag, IdAllocator, ItemTag, SimId};
use sim_core::persistence::version::validate_supported_version;
use sim_core::rng::{PresentationRng, RandomStreams};
use sim_core::schedule::{ScheduledWork, TurnClock, stable_sort_by_key};
use sim_core::time::{SimClock, SimSpeed};
use sim_core::work_budget::{SimulationWorkBudget, WorkBudgetProgress};

#[test]
fn typed_ids_allocate_independently() {
    let mut actors = IdAllocator::<ActorTag>::default();
    let mut items = IdAllocator::<ItemTag>::default();

    let actor_a = actors.allocate();
    let actor_b = actors.allocate();
    let item_a = items.allocate();

    assert_eq!(actor_a.raw(), 1);
    assert_eq!(actor_b.raw(), 2);
    assert_eq!(item_a.raw(), 1);

    let _: SimId<ActorTag> = actor_a;
    let _: SimId<ItemTag> = item_a;
}

#[test]
fn random_streams_snapshot_and_presentation_rng_are_separate() {
    let mut streams = RandomStreams::seeded(42);
    let baseline = streams.snapshot();
    let _ = streams.next_generation_u64();
    let _ = streams.next_ai_u64();

    let restored = RandomStreams::from_snapshot(&baseline);
    assert_eq!(baseline, restored.snapshot());

    let mut presentation = PresentationRng::seeded(42);
    let presentation_value = presentation.next_u64();
    assert_ne!(presentation_value, 0);
    assert_eq!(restored.snapshot(), baseline);
}

#[test]
fn sim_clock_advances_and_respects_pause() {
    let mut clock = SimClock::default();
    assert_eq!(clock.minute, 0);
    assert_eq!(clock.speed, SimSpeed::Normal);

    clock.advance_minutes(15);
    assert_eq!(clock.minute, 15);

    clock.set_speed(SimSpeed::Paused);
    clock.advance_minutes(30);
    assert_eq!(clock.minute, 15);
    assert!(clock.paused);
}

#[test]
fn work_budget_continues_without_reordering() {
    let budget = SimulationWorkBudget {
        maximum_steps_per_frame: 1,
        maximum_domain_events_per_frame: 2,
    };
    let mut progress = WorkBudgetProgress::default();
    let mut processed = Vec::new();
    let mut backlog = vec![1, 2, 3, 4];

    while !backlog.is_empty() {
        progress.consume_step();
        while let Some(next) = backlog.first().copied() {
            processed.push(next);
            backlog.remove(0);
            progress.consume_domain_events(1);
            if budget.exhausted(&progress) {
                progress.steps_consumed = 0;
                progress.domain_events_consumed = 0;
                break;
            }
        }
    }

    assert_eq!(processed, vec![1, 2, 3, 4]);
}

#[test]
fn stable_ordering_uses_tick_sequence_and_actor_identity() {
    let mut entries = vec![
        ScheduledWork {
            next_tick: 5,
            sequence: 2,
            actor: 9_u64,
        },
        ScheduledWork {
            next_tick: 4,
            sequence: 99,
            actor: 1_u64,
        },
        ScheduledWork {
            next_tick: 5,
            sequence: 1,
            actor: 7_u64,
        },
    ];

    stable_sort_by_key(&mut entries, |entry| {
        (entry.next_tick, entry.sequence, entry.actor)
    });

    assert_eq!(
        entries
            .into_iter()
            .map(|entry| entry.actor)
            .collect::<Vec<_>>(),
        vec![1, 7, 9]
    );
}

#[test]
fn turn_clock_uses_stable_tie_breaking() {
    let mut clock = TurnClock::<u64>::default();
    clock.schedule_at(9, 5);
    clock.schedule_at(1, 5);

    let first = clock.pop_next().expect("first entry");
    let second = clock.pop_next().expect("second entry");

    assert_eq!(first.actor, 9);
    assert_eq!(second.actor, 1);
}

#[test]
fn version_validation_rejects_zero_and_future_versions() {
    assert!(validate_supported_version(0).is_err());
    assert!(validate_supported_version(u32::MAX).is_err());
    assert!(validate_supported_version(sim_core::CURRENT_SCHEMA_VERSION).is_ok());
}

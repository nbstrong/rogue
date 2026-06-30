use sim_core::identity::{ActorTag, IdAllocator, ItemTag, SimId};
use sim_core::persistence::version::validate_supported_version;
use sim_core::rng::{PresentationRng, RandomStreams};
use sim_core::schedule::{ScheduledWork, TurnClock, stable_sort_by_key};
use sim_core::time::{SimClock, SimSpeed};
use sim_core::work_budget::SimulationWorkBudget;
use sim_core::{Cadence, DeterministicDriver, DueWork};

#[test]
fn typed_ids_allocate_independently() {
    let mut actors = IdAllocator::<ActorTag>::default();
    let mut items = IdAllocator::<ItemTag>::default();

    let actor_a = actors.allocate().expect("actor id");
    let actor_b = actors.allocate().expect("actor id");
    let item_a = items.allocate().expect("item id");

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
    assert!(clock.is_paused());
}

#[test]
fn work_budget_continues_without_reordering() {
    let mut driver = DeterministicDriver::<u64>::default();
    driver.budget = SimulationWorkBudget {
        maximum_steps_per_frame: 1,
        maximum_domain_events_per_frame: 2,
    };
    driver.clock.speed = SimSpeed::VeryFast;
    driver.enqueue(DueWork {
        cadence: Cadence::Minute,
        due_minute: 1,
        sequence: 1,
        id: 4,
        domain_event_cost: 1,
    });
    driver.enqueue(DueWork {
        cadence: Cadence::Minute,
        due_minute: 0,
        sequence: 0,
        id: 1,
        domain_event_cost: 1,
    });
    driver.enqueue(DueWork {
        cadence: Cadence::Hour,
        due_minute: 0,
        sequence: 2,
        id: 2,
        domain_event_cost: 1,
    });

    let mut processed = Vec::new();
    while !driver.backlog.is_empty() {
        driver.begin_frame();
        driver
            .run_frame(|_, work| {
                processed.push(work.id);
                work.domain_event_cost
            })
            .expect("driver frame");
    }

    assert_eq!(processed, vec![1, 2, 4]);
}

#[test]
fn work_budget_orders_same_minute_by_cadence_then_sequence() {
    let mut driver = DeterministicDriver::<u64>::default();
    driver.budget = SimulationWorkBudget {
        maximum_steps_per_frame: 8,
        maximum_domain_events_per_frame: 8,
    };
    driver.clock.speed = SimSpeed::VeryFast;
    driver.enqueue(DueWork {
        cadence: Cadence::Hour,
        due_minute: 0,
        sequence: 3,
        id: 3,
        domain_event_cost: 1,
    });
    driver.enqueue(DueWork {
        cadence: Cadence::Tactical,
        due_minute: 0,
        sequence: 2,
        id: 2,
        domain_event_cost: 1,
    });
    driver.enqueue(DueWork {
        cadence: Cadence::Tactical,
        due_minute: 0,
        sequence: 1,
        id: 1,
        domain_event_cost: 1,
    });

    let mut processed = Vec::new();
    driver.begin_frame();
    driver
        .run_frame(|_, work| {
            processed.push(work.id);
            work.domain_event_cost
        })
        .expect("driver frame");

    assert_eq!(processed, vec![1, 2, 3]);
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
fn driver_exposes_the_due_clock_to_callbacks() {
    let mut driver = DeterministicDriver::<u64>::default();
    driver.budget = SimulationWorkBudget {
        maximum_steps_per_frame: 4,
        maximum_domain_events_per_frame: 4,
    };
    driver.clock.speed = SimSpeed::VeryFast;
    driver.enqueue(DueWork {
        cadence: Cadence::Minute,
        due_minute: 7,
        sequence: 0,
        id: 11,
        domain_event_cost: 2,
    });

    let mut observed_minutes = Vec::new();
    driver.begin_frame();
    driver
        .run_frame(|clock, work| {
            observed_minutes.push((clock.minute, work.due_minute));
            work.domain_event_cost
        })
        .expect("driver frame");

    assert_eq!(observed_minutes, vec![(7, 7)]);
}

#[test]
fn driver_roundtrip_preserves_backlog_and_progress() {
    let mut driver = DeterministicDriver::<u64>::default();
    driver.budget = SimulationWorkBudget {
        maximum_steps_per_frame: 1,
        maximum_domain_events_per_frame: 2,
    };
    driver.clock.speed = SimSpeed::Normal;
    driver.enqueue(DueWork {
        cadence: Cadence::Minute,
        due_minute: 0,
        sequence: 0,
        id: 1,
        domain_event_cost: 1,
    });
    driver.enqueue(DueWork {
        cadence: Cadence::Minute,
        due_minute: 1,
        sequence: 1,
        id: 2,
        domain_event_cost: 1,
    });

    driver.begin_frame();
    driver
        .run_frame(|_, work| work.domain_event_cost)
        .expect("driver frame");

    let encoded = ron::to_string(&driver).expect("driver snapshot");
    let mut restored: DeterministicDriver<u64> = ron::from_str(&encoded).expect("driver restore");

    let mut uninterrupted = driver.clone();

    let mut resumed_processed = Vec::new();
    restored.begin_frame();
    restored
        .run_frame(|_, work| {
            resumed_processed.push(work.id);
            work.domain_event_cost
        })
        .expect("driver frame");

    let mut expected_processed = Vec::new();
    uninterrupted.begin_frame();
    uninterrupted
        .run_frame(|_, work| {
            expected_processed.push(work.id);
            work.domain_event_cost
        })
        .expect("driver frame");

    assert_eq!(resumed_processed, expected_processed);
    assert_eq!(restored.clock.minute, uninterrupted.clock.minute);
    assert_eq!(restored.progress, uninterrupted.progress);
    assert_eq!(
        restored.backlog.peek().map(|work| work.id),
        uninterrupted.backlog.peek().map(|work| work.id)
    );
}

#[test]
fn version_validation_rejects_zero_and_future_versions() {
    assert!(validate_supported_version(0).is_err());
    assert!(validate_supported_version(u32::MAX).is_err());
    assert!(validate_supported_version(sim_core::CURRENT_SCHEMA_VERSION).is_ok());
}

#[test]
fn allocator_exhaustion_is_explicit() {
    let mut allocator = IdAllocator::<ActorTag>::default();
    allocator.set_next_available(u64::MAX - 1);
    let _ = allocator.allocate().expect("final available id");
    assert!(allocator.allocate().is_err());
}

#[test]
fn zero_ids_are_rejected_during_deserialization() {
    let result = ron::from_str::<sim_core::SimId<ActorTag>>("0");
    assert!(result.is_err());
}

#[test]
fn allocator_deserialization_rejects_zero_state() {
    let result = ron::from_str::<IdAllocator<ActorTag>>("IdAllocator(next_id: 0)");
    assert!(result.is_err());
}

#[test]
fn work_items_larger_than_the_frame_budget_are_rejected() {
    let mut driver = DeterministicDriver::<u64>::default();
    driver.budget = SimulationWorkBudget {
        maximum_steps_per_frame: 4,
        maximum_domain_events_per_frame: 3,
    };
    driver.clock.speed = SimSpeed::Normal;
    driver.enqueue(DueWork {
        cadence: Cadence::Minute,
        due_minute: 0,
        sequence: 0,
        id: 1,
        domain_event_cost: 4,
    });

    driver.begin_frame();
    let err = driver
        .run_frame(|_, work| work.domain_event_cost)
        .expect_err("oversized work should be rejected");

    assert_eq!(
        err,
        sim_core::DriverError::WorkExceedsRemainingBudget {
            id: 1,
            remaining_domain_events: 3,
            declared_cost: 4,
        }
    );
    assert_eq!(driver.backlog.peek().map(|work| work.id), Some(1));
}

#[test]
fn callbacks_cannot_exceed_the_declared_event_cost() {
    let mut driver = DeterministicDriver::<u64>::default();
    driver.budget = SimulationWorkBudget {
        maximum_steps_per_frame: 4,
        maximum_domain_events_per_frame: 8,
    };
    driver.clock.speed = SimSpeed::Normal;
    driver.enqueue(DueWork {
        cadence: Cadence::Minute,
        due_minute: 0,
        sequence: 0,
        id: 1,
        domain_event_cost: 2,
    });

    driver.begin_frame();
    let err = driver
        .run_frame(|_, _| 3)
        .expect_err("violating the declared event cost should fail");

    assert_eq!(
        err,
        sim_core::DriverError::WorkProducedMoreEventsThanDeclared {
            id: 1,
            declared_cost: 2,
            produced: 3,
        }
    );
}

#[test]
fn backlog_serializes_canonically_regardless_of_insertion_order() {
    let mut first = DeterministicDriver::<u64>::default();
    first.enqueue(DueWork {
        cadence: Cadence::Hour,
        due_minute: 4,
        sequence: 2,
        id: 20,
        domain_event_cost: 1,
    });
    first.enqueue(DueWork {
        cadence: Cadence::Minute,
        due_minute: 1,
        sequence: 1,
        id: 10,
        domain_event_cost: 1,
    });

    let mut second = DeterministicDriver::<u64>::default();
    second.enqueue(DueWork {
        cadence: Cadence::Minute,
        due_minute: 1,
        sequence: 1,
        id: 10,
        domain_event_cost: 1,
    });
    second.enqueue(DueWork {
        cadence: Cadence::Hour,
        due_minute: 4,
        sequence: 2,
        id: 20,
        domain_event_cost: 1,
    });

    assert_eq!(
        ron::to_string(&first).expect("serialize first"),
        ron::to_string(&second).expect("serialize second")
    );
}

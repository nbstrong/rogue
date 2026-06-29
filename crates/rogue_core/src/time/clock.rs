use std::{
    cmp::Reverse,
    collections::BinaryHeap,
};

use bevy_ecs::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledActor {
    pub next_tick: u64,
    pub sequence: u64,
    pub actor: Entity,
}

impl Ord for ScheduledActor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.next_tick, self.sequence, self.actor).cmp(&(other.next_tick, other.sequence, other.actor))
    }
}

impl PartialOrd for ScheduledActor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Resource, Default, Debug, Clone)]
pub struct TurnClock {
    pub current_tick: u64,
    pub next_sequence: u64,
    pub timeline: BinaryHeap<Reverse<ScheduledActor>>,
}

impl TurnClock {
    pub fn schedule_at(&mut self, actor: Entity, next_tick: u64) {
        let entry = ScheduledActor {
            next_tick,
            sequence: self.next_sequence,
            actor,
        };
        self.next_sequence += 1;
        self.timeline.push(Reverse(entry));
    }

    pub fn schedule_after(&mut self, actor: Entity, cost: u64) {
        let next_tick = self.current_tick + cost;
        self.schedule_at(actor, next_tick);
    }

    pub fn pop_next(&mut self) -> Option<ScheduledActor> {
        self.timeline.pop().map(|entry| entry.0)
    }

    pub fn peek_next(&self) -> Option<&ScheduledActor> {
        self.timeline.peek().map(|entry| &entry.0)
    }
}

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CurrentActor(pub Option<Entity>);

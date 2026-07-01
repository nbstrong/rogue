use core::cmp::Reverse;
use std::collections::BinaryHeap;

use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentActor<Id>(pub Option<Id>);

impl<Id> Default for CurrentActor<Id> {
    fn default() -> Self {
        Self(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledWork<Id> {
    pub next_tick: u64,
    pub sequence: u64,
    pub actor: Id,
}

impl<Id: Ord> Ord for ScheduledWork<Id> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (self.next_tick, self.sequence, &self.actor).cmp(&(
            other.next_tick,
            other.sequence,
            &other.actor,
        ))
    }
}

impl<Id: Ord> PartialOrd for ScheduledWork<Id> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Resource, Debug, Clone)]
pub struct TurnClock<Id> {
    pub current_tick: u64,
    pub next_sequence: u64,
    pub timeline: BinaryHeap<Reverse<ScheduledWork<Id>>>,
}

impl<Id> Default for TurnClock<Id> {
    fn default() -> Self {
        Self {
            current_tick: 0,
            next_sequence: 0,
            timeline: BinaryHeap::new(),
        }
    }
}

impl<Id: Ord + Copy> TurnClock<Id> {
    pub fn schedule_at(&mut self, actor: Id, next_tick: u64) {
        let entry = ScheduledWork {
            next_tick,
            sequence: self.next_sequence,
            actor,
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.timeline.push(Reverse(entry));
    }

    pub fn schedule_after(&mut self, actor: Id, cost: u64) {
        let next_tick = self.current_tick + cost;
        self.schedule_at(actor, next_tick);
    }

    pub fn pop_next(&mut self) -> Option<ScheduledWork<Id>> {
        self.timeline.pop().map(|entry| entry.0)
    }

    pub fn peek_next(&self) -> Option<&ScheduledWork<Id>> {
        self.timeline.peek().map(|entry| &entry.0)
    }
}

pub fn stable_sort_by_key<T, K: Ord, F>(values: &mut [T], mut key: F)
where
    F: FnMut(&T) -> K,
{
    values.sort_by_key(|value| key(value));
}

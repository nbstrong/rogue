use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::time::{SimClock, SimSpeed};
use crate::work_budget::{SimulationWorkBudget, WorkBudgetProgress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cadence {
    Tactical,
    Minute,
    Hour,
    Day,
    Strategic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DueWork<Id> {
    pub cadence: Cadence,
    pub due_minute: u64,
    pub sequence: u64,
    pub id: Id,
}

impl<Id: Ord> Ord for DueWork<Id> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.cadence, self.due_minute, self.sequence, &self.id).cmp(&(
            other.cadence,
            other.due_minute,
            other.sequence,
            &other.id,
        ))
    }
}

impl<Id: Ord> PartialOrd for DueWork<Id> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone)]
pub struct WorkBacklog<Id> {
    queue: BinaryHeap<Reverse<DueWork<Id>>>,
}

impl<Id> Default for WorkBacklog<Id> {
    fn default() -> Self {
        Self {
            queue: BinaryHeap::default(),
        }
    }
}

impl<Id: Ord + Copy> WorkBacklog<Id> {
    pub fn enqueue(&mut self, work: DueWork<Id>) {
        self.queue.push(Reverse(work));
    }

    pub fn peek(&self) -> Option<&DueWork<Id>> {
        self.queue.peek().map(|entry| &entry.0)
    }

    pub fn pop(&mut self) -> Option<DueWork<Id>> {
        self.queue.pop().map(|entry| entry.0)
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct DeterministicDriver<Id> {
    pub clock: SimClock,
    pub budget: SimulationWorkBudget,
    pub progress: WorkBudgetProgress,
    pub backlog: WorkBacklog<Id>,
}

impl<Id> Default for DeterministicDriver<Id> {
    fn default() -> Self {
        Self {
            clock: SimClock::default(),
            budget: SimulationWorkBudget::default(),
            progress: WorkBudgetProgress::default(),
            backlog: WorkBacklog::default(),
        }
    }
}

impl<Id: Ord + Copy> DeterministicDriver<Id> {
    pub fn enqueue(&mut self, work: DueWork<Id>) {
        self.backlog.enqueue(work);
    }

    pub fn begin_frame(&mut self) {
        self.progress = WorkBudgetProgress::default();
    }

    pub fn target_minute(&self) -> u64 {
        self.clock
            .minute
            .saturating_add(self.clock.speed.advance_minutes())
    }

    pub fn run_frame<F>(&mut self, mut apply: F)
    where
        F: FnMut(DueWork<Id>) -> usize,
    {
        let target = self.target_minute();
        while !self.budget.exhausted(&self.progress) {
            let Some(next) = self.backlog.peek().copied() else {
                break;
            };
            if next.due_minute > target {
                break;
            }

            let work = self.backlog.pop().expect("backlog peek/pop mismatch");
            if self.clock.minute < work.due_minute {
                self.clock.minute = work.due_minute;
            }

            let produced = apply(work);
            self.progress.consume_step();
            self.progress.consume_domain_events(produced.max(1));
        }

        self.clock.minute = target.max(self.clock.minute);
    }
}

impl SimSpeed {
    pub fn advance_minutes(self) -> u64 {
        match self {
            SimSpeed::Paused => 0,
            SimSpeed::Normal => 1,
            SimSpeed::Fast => 10,
            SimSpeed::VeryFast => 100,
        }
    }
}

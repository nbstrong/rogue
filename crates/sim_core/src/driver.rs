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

impl Cadence {
    pub fn rank(self) -> u8 {
        match self {
            Cadence::Tactical => 0,
            Cadence::Minute => 1,
            Cadence::Hour => 2,
            Cadence::Day => 3,
            Cadence::Strategic => 4,
        }
    }
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
        (
            self.due_minute,
            self.cadence.rank(),
            self.sequence,
            &self.id,
        )
            .cmp(&(
                other.due_minute,
                other.cadence.rank(),
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
    pending_target_minute: Option<u64>,
}

impl<Id> Default for DeterministicDriver<Id> {
    fn default() -> Self {
        Self {
            clock: SimClock::default(),
            budget: SimulationWorkBudget::default(),
            progress: WorkBudgetProgress::default(),
            backlog: WorkBacklog::default(),
            pending_target_minute: None,
        }
    }
}

impl<Id: Ord + Copy> DeterministicDriver<Id> {
    pub fn enqueue(&mut self, work: DueWork<Id>) {
        self.backlog.enqueue(work);
    }

    pub fn begin_frame(&mut self) {
        self.progress = WorkBudgetProgress::default();
        if self.pending_target_minute.is_none() {
            self.pending_target_minute = Some(self.target_minute());
        }
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
        let target = self
            .pending_target_minute
            .unwrap_or_else(|| self.target_minute());
        let mut processed_minute = self.clock.minute;
        while !self.budget.exhausted(&self.progress) {
            let Some(next) = self.backlog.peek().copied() else {
                break;
            };
            if next.due_minute > target {
                break;
            }

            let work = self.backlog.pop().expect("backlog peek/pop mismatch");
            if processed_minute < work.due_minute {
                processed_minute = work.due_minute;
            }

            let produced = apply(work);
            self.progress.consume_step();
            self.progress.consume_domain_events(produced.max(1));
        }

        let exhausted = self.budget.exhausted(&self.progress);
        if self.backlog.peek().is_none()
            || self
                .backlog
                .peek()
                .is_some_and(|next| next.due_minute > target)
        {
            self.clock.minute = target.max(processed_minute);
            self.pending_target_minute = None;
        } else if exhausted {
            self.clock.minute = processed_minute;
            self.pending_target_minute = Some(target);
        } else {
            self.clock.minute = processed_minute;
            self.pending_target_minute = Some(target);
        }
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

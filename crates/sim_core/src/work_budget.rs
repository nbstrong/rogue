use bevy_ecs::prelude::Resource;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulationWorkBudget {
    pub maximum_steps_per_frame: usize,
    pub maximum_domain_events_per_frame: usize,
}

impl Default for SimulationWorkBudget {
    fn default() -> Self {
        Self {
            maximum_steps_per_frame: 1_024,
            maximum_domain_events_per_frame: 1_024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkBudgetProgress {
    pub steps_consumed: usize,
    pub domain_events_consumed: usize,
}

impl SimulationWorkBudget {
    pub fn remaining_steps(&self, progress: &WorkBudgetProgress) -> usize {
        self.maximum_steps_per_frame
            .saturating_sub(progress.steps_consumed)
    }

    pub fn remaining_domain_events(&self, progress: &WorkBudgetProgress) -> usize {
        self.maximum_domain_events_per_frame
            .saturating_sub(progress.domain_events_consumed)
    }

    pub fn exhausted(&self, progress: &WorkBudgetProgress) -> bool {
        self.remaining_steps(progress) == 0 || self.remaining_domain_events(progress) == 0
    }
}

impl WorkBudgetProgress {
    pub fn consume_step(&mut self) {
        self.steps_consumed = self.steps_consumed.saturating_add(1);
    }

    pub fn consume_domain_events(&mut self, count: usize) {
        self.domain_events_consumed = self.domain_events_consumed.saturating_add(count);
    }
}

use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimSpeed {
    Paused,
    Normal,
    Fast,
    VeryFast,
}

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimClock {
    pub minute: u64,
    pub speed: SimSpeed,
}

impl Default for SimClock {
    fn default() -> Self {
        Self {
            minute: 0,
            speed: SimSpeed::Normal,
        }
    }
}

impl SimClock {
    pub fn advance_minutes(&mut self, minutes: u64) {
        if !self.is_paused() {
            self.minute = self.minute.saturating_add(minutes);
        }
    }

    pub fn set_speed(&mut self, speed: SimSpeed) {
        self.speed = speed;
    }

    pub fn is_paused(&self) -> bool {
        matches!(self.speed, SimSpeed::Paused)
    }
}

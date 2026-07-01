use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RandomSnapshot {
    pub seed: u64,
    pub generation_state: u64,
    pub combat_state: u64,
    pub loot_state: u64,
    pub ai_state: u64,
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct RandomStreams {
    pub seed: u64,
    pub generation_state: u64,
    pub combat_state: u64,
    pub loot_state: u64,
    pub ai_state: u64,
}

impl Default for RandomStreams {
    fn default() -> Self {
        Self::seeded(0)
    }
}

impl RandomStreams {
    pub fn seeded(seed: u64) -> Self {
        Self {
            seed,
            generation_state: mix(seed ^ 0x6a09_e667_f3bc_c909),
            combat_state: mix(seed ^ 0xbb67_ae85_84ca_a73b),
            loot_state: mix(seed ^ 0x3c6e_f372_fe94_f82b),
            ai_state: mix(seed ^ 0xa54f_f53a_5f1d_36f1),
        }
    }

    pub fn from_snapshot(snapshot: &RandomSnapshot) -> Self {
        Self {
            seed: snapshot.seed,
            generation_state: snapshot.generation_state,
            combat_state: snapshot.combat_state,
            loot_state: snapshot.loot_state,
            ai_state: snapshot.ai_state,
        }
    }

    pub fn snapshot(&self) -> RandomSnapshot {
        RandomSnapshot {
            seed: self.seed,
            generation_state: self.generation_state,
            combat_state: self.combat_state,
            loot_state: self.loot_state,
            ai_state: self.ai_state,
        }
    }

    pub fn next_generation_u64(&mut self) -> u64 {
        next_u64(&mut self.generation_state)
    }

    pub fn next_combat_u64(&mut self) -> u64 {
        next_u64(&mut self.combat_state)
    }

    pub fn next_loot_u64(&mut self) -> u64 {
        next_u64(&mut self.loot_state)
    }

    pub fn next_ai_u64(&mut self) -> u64 {
        next_u64(&mut self.ai_state)
    }
}

#[derive(Debug, Clone)]
pub struct PresentationRng {
    state: u64,
}

impl PresentationRng {
    pub fn seeded(seed: u64) -> Self {
        Self {
            state: mix(seed ^ 0x1d2e_3f4a_5b6c_7d8e),
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        next_u64(&mut self.state)
    }
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn next_u64(state: &mut u64) -> u64 {
    let mut x = *state;
    if x == 0 {
        x = 0x9e37_79b9_7f4a_7c15;
    }
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

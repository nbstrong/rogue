use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DamageKind {
    Melee,
    Ranged,
    Fire,
    Poison,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusEffect {
    Poisoned { remaining: u32 },
    Stunned { remaining: u32 },
}

use crate::actor::components::CombatStats;

pub fn melee_damage(attacker: CombatStats, defender: CombatStats) -> i32 {
    (attacker.power - defender.defense).max(1)
}

use serde::{Deserialize, Serialize};

use crate::persistence::rng::RandomStreams;

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

pub fn melee_damage(
    attacker: CombatStats,
    defender: CombatStats,
    rng: Option<&mut RandomStreams>,
) -> i32 {
    let base = (attacker.power - defender.defense).max(1);
    let variance = rng
        .map(|rng| (rng.next_combat_u64() % 3) as i32 - 1)
        .unwrap_or(0);
    (base + variance).max(1)
}

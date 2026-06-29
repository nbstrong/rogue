#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    Melee,
    Ranged,
    Fire,
    Poison,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusEffect {
    Poisoned { remaining: u32 },
    Stunned { remaining: u32 },
}


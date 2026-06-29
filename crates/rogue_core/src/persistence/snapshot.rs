use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RandomSnapshot {
    pub seed: u64,
    pub combat_state: u64,
    pub loot_state: u64,
    pub ai_state: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SavedPosition {
    pub level: u32,
    pub x: i32,
    pub y: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SavedHealth {
    pub current: i32,
    pub maximum: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LevelSnapshot {
    pub id: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GameSnapshot {
    pub version: u32,
    pub current_level: u32,
    pub current_tick: u64,
    pub levels: Vec<LevelSnapshot>,
    pub entities: Vec<EntitySnapshot>,
    pub rng: RandomSnapshot,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntitySnapshot {
    pub id: u64,
    pub prototype: String,
    pub position: Option<SavedPosition>,
    pub health: Option<SavedHealth>,
    pub inventory_owner: Option<u64>,
}

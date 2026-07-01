use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ItemUseEffect {
    Heal { amount: i32 },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActorDefinition {
    pub id: String,
    pub name: String,
    pub maximum_health: i32,
    pub power: i32,
    pub defense: i32,
    pub vision_range: u32,
    pub action_speed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub use_effect: Option<ItemUseEffect>,
}

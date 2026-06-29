use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ActorDefinition {
    pub id: String,
    pub name: String,
    pub glyph: char,
    pub maximum_health: i32,
    pub power: i32,
    pub defense: i32,
    pub vision_range: u32,
    pub action_speed: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ItemDefinition {
    pub id: String,
    pub name: String,
    pub glyph: char,
}


use bevy::prelude::*;

#[allow(dead_code)]
#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Boot,
    MainMenu,
    Playing,
    GameOver,
}

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentInputMode(pub InputMode);

impl Default for CurrentInputMode {
    fn default() -> Self {
        Self(InputMode::Normal)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum InputMode {
    #[default]
    Normal,
    Inventory,
    Targeting,
    Examine,
}

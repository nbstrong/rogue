use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Boot,
    MainMenu,
    Playing,
    GameOver,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum InputMode {
    #[default]
    Normal,
    Inventory,
    Targeting,
    Examine,
}


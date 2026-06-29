use bevy::prelude::*;
use rogue_core::action::intent::{Action, ActionKind};
use rogue_core::action::queue::ActionQueue;
use rogue_core::actor::components::Player;
use rogue_core::simulation::SimulationStatus;

use crate::app_state::{AppState, CurrentInputMode, InputMode};

fn key_to_delta(key: KeyCode) -> Option<IVec2> {
    match key {
        KeyCode::KeyH => Some(IVec2::new(-1, 0)),
        KeyCode::KeyJ => Some(IVec2::new(0, -1)),
        KeyCode::KeyK => Some(IVec2::new(0, 1)),
        KeyCode::KeyL => Some(IVec2::new(1, 0)),
        KeyCode::KeyY => Some(IVec2::new(-1, 1)),
        KeyCode::KeyU => Some(IVec2::new(1, 1)),
        KeyCode::KeyB => Some(IVec2::new(-1, -1)),
        KeyCode::KeyN => Some(IVec2::new(1, -1)),
        _ => None,
    }
}

pub fn capture_keyboard_input(
    keys: Res<'_, ButtonInput<KeyCode>>,
    player: Query<'_, '_, Entity, With<Player>>,
    mut queue: ResMut<'_, ActionQueue>,
    mut simulation: ResMut<'_, SimulationStatus>,
    input_mode: Res<'_, CurrentInputMode>,
) {
    if input_mode.0 != InputMode::Normal || *simulation != SimulationStatus::WaitingForPlayer {
        return;
    }

    let Some(player) = player.iter().next() else {
        return;
    };

    let action = if keys.just_pressed(KeyCode::Space) {
        Some(ActionKind::Wait)
    } else {
        keys.get_just_pressed()
            .copied()
            .find_map(key_to_delta)
            .map(|delta| ActionKind::Move { delta })
    };

    if let Some(kind) = action {
        queue.push(Action {
            actor: player,
            kind,
        });
        *simulation = SimulationStatus::Resolving;
    }
}

pub fn restart_from_game_over(
    keys: Res<'_, ButtonInput<KeyCode>>,
    mut commands: Commands<'_, '_>,
    mut next_state: ResMut<'_, NextState<AppState>>,
    mut simulation: ResMut<'_, SimulationStatus>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        commands.queue(|world: &mut World| {
            crate::game::setup_new_game(world, true);
        });
        *simulation = SimulationStatus::WaitingForPlayer;
        next_state.set(AppState::Playing);
    }
}

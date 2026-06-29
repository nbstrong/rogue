use bevy_math::IVec2;

use rogue_core::ActionKind;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyBindingAction {
    Move(IVec2),
    Wait,
    Inventory,
}

#[allow(dead_code)]
pub fn map_binding(action: KeyBindingAction) -> ActionKind {
    match action {
        KeyBindingAction::Move(delta) => ActionKind::Move { delta },
        KeyBindingAction::Wait => ActionKind::Wait,
        KeyBindingAction::Inventory => ActionKind::Wait,
    }
}

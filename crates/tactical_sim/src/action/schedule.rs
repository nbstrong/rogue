use crate::action::intent::ActionKind;

pub fn action_cost(kind: &ActionKind) -> u64 {
    match kind {
        ActionKind::Wait => 100,
        ActionKind::Move { .. } => 100,
        ActionKind::Melee { .. } => 140,
        ActionKind::PickUp { .. } => 60,
        ActionKind::Drop { .. } => 60,
        ActionKind::UseItem { .. } => 80,
        ActionKind::Descend | ActionKind::Ascend => 80,
    }
}

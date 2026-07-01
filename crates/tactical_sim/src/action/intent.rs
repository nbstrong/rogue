use bevy_math::IVec2;

use crate::actor::components::{ActorId, ItemId};
use crate::world::map::{GridPosition, LevelId};

#[derive(Debug, Clone)]
pub struct Action {
    pub actor: ActorId,
    pub kind: ActionKind,
}

#[derive(Debug, Clone)]
pub enum ActionKind {
    Wait,
    Move { delta: IVec2 },
    Melee { target: ActorId },
    PickUp { item: ItemId },
    Drop { item: ItemId },
    UseItem { item: ItemId, target: ActionTarget },
    Descend,
    Ascend,
}

#[derive(Debug, Clone)]
pub enum ActionTarget {
    SelfTarget,
    Actor(ActorId),
    Cell { level: LevelId, position: IVec2 },
}

impl ActionTarget {
    pub fn self_target() -> Self {
        Self::SelfTarget
    }
}

pub type ActorIntent = ActionKind;

impl From<GridPosition> for ActionTarget {
    fn from(value: GridPosition) -> Self {
        Self::Cell {
            level: value.level,
            position: value.cell,
        }
    }
}

use bevy_ecs::prelude::Entity;
use bevy_math::IVec2;

use crate::world::map::{GridPosition, LevelId};

#[derive(Debug, Clone)]
pub struct Action {
    pub actor: Entity,
    pub kind: ActionKind,
}

#[derive(Debug, Clone)]
pub enum ActionKind {
    Wait,
    Move { delta: IVec2 },
    Melee { target: Entity },
    PickUp { item: Entity },
    Drop { item: Entity },
    UseItem { item: Entity, target: ActionTarget },
    Descend,
    Ascend,
}

#[derive(Debug, Clone)]
pub enum ActionTarget {
    SelfTarget,
    Entity(Entity),
    Cell { level: LevelId, position: IVec2 },
}

impl ActionTarget {
    pub fn self_target() -> Self {
        Self::SelfTarget
    }
}

pub type PlayerIntent = ActionKind;

impl From<GridPosition> for ActionTarget {
    fn from(value: GridPosition) -> Self {
        Self::Cell {
            level: value.level,
            position: value.cell,
        }
    }
}

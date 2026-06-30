use std::collections::VecDeque;

use bevy_ecs::prelude::Entity;
use bevy_ecs::prelude::Resource;

use crate::action::intent::Action;

#[derive(Resource, Default, Debug, Clone)]
pub struct ActionQueue {
    pub actions: VecDeque<Action>,
}

impl ActionQueue {
    pub fn push(&mut self, action: Action) {
        if self
            .actions
            .iter()
            .any(|pending| pending.actor == action.actor)
        {
            return;
        }
        self.actions.push_back(action);
    }

    pub fn pop(&mut self) -> Option<Action> {
        self.actions.pop_front()
    }

    pub fn contains_actor(&self, actor: Entity) -> bool {
        self.actions.iter().any(|action| action.actor == actor)
    }

    pub fn take_for_actor(&mut self, actor: Entity) -> Option<Action> {
        let index = self
            .actions
            .iter()
            .position(|action| action.actor == actor)?;
        self.actions.remove(index)
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

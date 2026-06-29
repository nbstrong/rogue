use std::collections::VecDeque;

use bevy_ecs::prelude::Resource;

use crate::action::intent::Action;

#[derive(Resource, Default, Debug, Clone)]
pub struct ActionQueue {
    pub actions: VecDeque<Action>,
}

impl ActionQueue {
    pub fn push(&mut self, action: Action) {
        self.actions.push_back(action);
    }

    pub fn pop(&mut self) -> Option<Action> {
        self.actions.pop_front()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

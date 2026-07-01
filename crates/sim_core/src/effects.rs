use std::collections::VecDeque;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectQueue<T>(pub VecDeque<T>);

impl<T> EffectQueue<T> {
    pub fn push(&mut self, effect: T) {
        self.0.push_back(effect);
    }

    pub fn pop(&mut self) -> Option<T> {
        self.0.pop_front()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

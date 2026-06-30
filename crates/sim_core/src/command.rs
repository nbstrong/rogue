use std::collections::VecDeque;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandQueue<T>(pub VecDeque<T>);

impl<T> CommandQueue<T> {
    pub fn push(&mut self, command: T) {
        self.0.push_back(command);
    }

    pub fn pop(&mut self) -> Option<T> {
        self.0.pop_front()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

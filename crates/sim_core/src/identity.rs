use core::fmt;
use core::marker::PhantomData;

use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimId<Tag> {
    raw: u64,
    marker: PhantomData<fn() -> Tag>,
}

impl<Tag> SimId<Tag> {
    pub fn new(raw: u64) -> Option<Self> {
        (raw != 0).then_some(Self::from_raw_unchecked(raw))
    }

    pub const fn from_raw_unchecked(raw: u64) -> Self {
        Self {
            raw,
            marker: PhantomData,
        }
    }

    pub const fn raw(self) -> u64 {
        self.raw
    }
}

impl<Tag> fmt::Display for SimId<Tag> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw)
    }
}

impl<Tag> Serialize for SimId<Tag> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(self.raw)
    }
}

impl<'de, Tag> Deserialize<'de> for SimId<Tag> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = u64::deserialize(deserializer)?;
        Ok(Self::from_raw_unchecked(raw))
    }
}

pub trait StableIdTag: Sized + 'static {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ActorTag {}

impl StableIdTag for ActorTag {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemTag {}

impl StableIdTag for ItemTag {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PersistentTag {}

impl StableIdTag for PersistentTag {}

pub type ActorId = SimId<ActorTag>;
pub type ItemId = SimId<ItemTag>;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdAllocator<Tag> {
    next_id: u64,
    #[serde(skip)]
    marker: PhantomData<fn() -> Tag>,
}

impl<Tag> Default for IdAllocator<Tag> {
    fn default() -> Self {
        Self {
            next_id: 1,
            marker: PhantomData,
        }
    }
}

impl<Tag> IdAllocator<Tag> {
    pub fn allocate(&mut self) -> SimId<Tag> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        SimId::from_raw_unchecked(id)
    }

    pub fn next_available(&self) -> u64 {
        self.next_id
    }

    pub fn set_next_available(&mut self, next_id: u64) {
        self.next_id = next_id.max(1);
    }
}

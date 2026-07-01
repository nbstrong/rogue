use core::fmt;
use core::marker::PhantomData;
use core::num::NonZeroU64;

use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimId<Tag> {
    raw: u64,
    marker: PhantomData<fn() -> Tag>,
}

impl<Tag> SimId<Tag> {
    pub fn new(raw: u64) -> Option<Self> {
        NonZeroU64::new(raw).map(|value| Self {
            raw: value.get(),
            marker: PhantomData,
        })
    }

    pub(crate) const fn from_raw_unchecked(raw: u64) -> Self {
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
        Self::new(raw).ok_or_else(|| serde::de::Error::custom("zero is not a valid stable id"))
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
pub enum DomainTag {}

impl StableIdTag for DomainTag {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PersistentTag {}

impl StableIdTag for PersistentTag {}

pub type ActorId = SimId<ActorTag>;
pub type ItemId = SimId<ItemTag>;
pub type DomainWorkId = SimId<DomainTag>;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    pub fn allocate(&mut self) -> Result<SimId<Tag>, AllocationError> {
        let id = self.next_id;
        let next = self
            .next_id
            .checked_add(1)
            .ok_or(AllocationError::Exhausted)?;
        self.next_id = next;
        Ok(SimId::from_raw_unchecked(id))
    }

    pub fn next_available(&self) -> u64 {
        self.next_id
    }

    pub fn set_next_available(&mut self, next_id: u64) {
        self.next_id = next_id.max(1);
    }
}

impl<'de, Tag> Deserialize<'de> for IdAllocator<Tag> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct IdAllocatorSnapshot {
            next_id: u64,
        }

        let snapshot = IdAllocatorSnapshot::deserialize(deserializer)?;
        if snapshot.next_id == 0 {
            return Err(serde::de::Error::custom(
                "allocator next_id must be at least 1",
            ));
        }

        Ok(Self {
            next_id: snapshot.next_id,
            marker: PhantomData,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocationError {
    Exhausted,
}

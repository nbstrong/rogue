use bevy_ecs::prelude::{Component, Resource};
use bevy_math::IVec2;

use crate::world::tile::{Tile, TileKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LevelId(pub u32);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPosition {
    pub level: LevelId,
    pub cell: IVec2,
}

#[derive(Resource, Debug, Clone)]
pub struct LevelMap {
    pub width: u32,
    pub height: u32,
    pub tiles: Vec<Tile>,
}

impl LevelMap {
    pub fn new(width: u32, height: u32, fill: TileKind) -> Self {
        let tile = Tile::new(fill);
        Self {
            width,
            height,
            tiles: vec![tile; width as usize * height as usize],
        }
    }

    pub fn index(&self, position: IVec2) -> Option<usize> {
        if position.x < 0
            || position.y < 0
            || position.x >= self.width as i32
            || position.y >= self.height as i32
        {
            return None;
        }

        Some(position.y as usize * self.width as usize + position.x as usize)
    }

    pub fn tile(&self, position: IVec2) -> Option<&Tile> {
        self.index(position).map(|index| &self.tiles[index])
    }

    pub fn tile_mut(&mut self, position: IVec2) -> Option<&mut Tile> {
        self.index(position).map(|index| &mut self.tiles[index])
    }

    pub fn set_kind(&mut self, position: IVec2, kind: TileKind) -> bool {
        if let Some(tile) = self.tile_mut(position) {
            tile.kind = kind;
            true
        } else {
            false
        }
    }

    pub fn in_bounds(&self, position: IVec2) -> bool {
        self.index(position).is_some()
    }
}

use bevy_math::IVec2;

use crate::world::map::LevelMap;
use crate::world::tile::TileKind;

pub fn generate_one_room(width: u32, height: u32) -> LevelMap {
    let mut map = LevelMap::new(width, height, TileKind::Wall);

    if width < 3 || height < 3 {
        return map;
    }

    for y in 1..(height as i32 - 1) {
        for x in 1..(width as i32 - 1) {
            map.set_kind(IVec2::new(x, y), TileKind::Floor);
        }
    }

    map
}

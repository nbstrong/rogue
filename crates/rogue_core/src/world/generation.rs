use bevy_math::IVec2;

use crate::persistence::rng::RandomStreams;
use crate::world::map::LevelMap;
use crate::world::tile::TileKind;

pub fn generate_one_room(width: u32, height: u32) -> LevelMap {
    generate_one_room_with_rng(width, height, None)
}

pub fn generate_one_room_with_rng(
    width: u32,
    height: u32,
    mut rng: Option<&mut RandomStreams>,
) -> LevelMap {
    let mut map = LevelMap::new(width, height, TileKind::Wall);

    if width < 3 || height < 3 {
        return map;
    }

    for y in 1..(height as i32 - 1) {
        for x in 1..(width as i32 - 1) {
            map.set_kind(IVec2::new(x, y), TileKind::Floor);
        }
    }

    if let Some(rng) = rng.as_mut() {
        let interior_width = width as usize - 2;
        let interior_height = height as usize - 2;
        let total_cells = interior_width * interior_height;
        let down_index = (rng.next_generation_u64() as usize) % total_cells;
        let up_index = (rng.next_generation_u64() as usize) % total_cells;

        let to_cell = |index: usize| -> IVec2 {
            let x = 1 + (index % interior_width) as i32;
            let y = 1 + (index / interior_width) as i32;
            IVec2::new(x, y)
        };

        let down = to_cell(down_index);
        let up = to_cell(up_index);
        map.set_kind(down, TileKind::StairsDown);
        if up != down {
            map.set_kind(up, TileKind::StairsUp);
        }
    }

    map
}

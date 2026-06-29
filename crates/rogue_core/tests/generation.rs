use bevy_math::IVec2;
use rogue_core::world::generation::generate_one_room;
use rogue_core::world::tile::TileKind;

#[test]
fn one_room_has_wall_border_and_floor_interior() {
    let map = generate_one_room(5, 5);

    assert_eq!(map.tile(IVec2::new(0, 0)).unwrap().kind, TileKind::Wall);
    assert_eq!(map.tile(IVec2::new(2, 2)).unwrap().kind, TileKind::Floor);
    assert_eq!(map.tile(IVec2::new(4, 4)).unwrap().kind, TileKind::Wall);
}

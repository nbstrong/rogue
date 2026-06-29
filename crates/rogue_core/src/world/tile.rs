#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKind {
    Floor,
    Wall,
    ClosedDoor,
    OpenDoor,
    StairsUp,
    StairsDown,
}

#[derive(Debug, Clone)]
pub struct Tile {
    pub kind: TileKind,
    pub explored: bool,
    pub visible: bool,
}

impl Tile {
    pub fn new(kind: TileKind) -> Self {
        Self {
            kind,
            explored: false,
            visible: false,
        }
    }
}

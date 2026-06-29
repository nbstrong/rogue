use bevy_ecs::prelude::*;

use crate::item::components::{CarriedBy, Inventory};

pub fn add_item_to_inventory(
    mut inventories: Query<'_, '_, &mut Inventory>,
    mut carried_by: Query<'_, '_, &mut CarriedBy>,
    carrier: Entity,
    item: Entity,
) -> bool {
    let Ok(mut inventory) = inventories.get_mut(carrier) else {
        return false;
    };

    if inventory.is_full() {
        return false;
    }

    inventory.items.push(item);
    if let Ok(mut owner) = carried_by.get_mut(item) {
        owner.0 = carrier;
    }
    true
}


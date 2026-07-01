use bevy_ecs::prelude::*;

use crate::actor::components::{ActorId, ItemId, StableEntityIndex};
use crate::item::components::{CarriedBy, Inventory};

pub fn add_item_to_inventory(
    mut inventories: Query<'_, '_, &mut Inventory>,
    mut carried_by: Query<'_, '_, &mut CarriedBy>,
    stable_index: Res<'_, StableEntityIndex>,
    carrier: ActorId,
    item: ItemId,
) -> bool {
    let Some(carrier_entity) = stable_index.actor(carrier) else {
        return false;
    };
    let Some(item_entity) = stable_index.item(item) else {
        return false;
    };

    let Ok(mut inventory) = inventories.get_mut(carrier_entity) else {
        return false;
    };

    if inventory.is_full() {
        return false;
    }

    inventory.items.push(item);
    if let Ok(mut owner) = carried_by.get_mut(item_entity) {
        owner.0 = carrier;
    }
    true
}

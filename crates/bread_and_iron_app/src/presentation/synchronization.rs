use super::PresentationRngState;
use bevy_ecs::prelude::ResMut;

pub fn synchronize_presentation(mut rng: ResMut<'_, PresentationRngState>) {
    let _ = rng.0.next_u64();
}

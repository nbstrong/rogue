use crate::app_state::AppState;
use bevy::prelude::*;
use bevy::state::condition::in_state;

pub mod actor_view;
pub mod animation;
pub mod camera;
pub mod map_view;
pub mod synchronization;

pub struct PresentationPlugin;

impl Plugin for PresentationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                map_view::synchronize_map_view,
                actor_view::synchronize_actor_views,
                camera::update_camera,
                animation::update_animations,
                synchronization::synchronize_presentation,
            )
                .run_if(in_state(AppState::Playing)),
        );
    }
}

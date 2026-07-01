use crate::app_state::AppState;
use bevy::prelude::*;
use bevy::state::condition::in_state;
use tactical_sim::persistence::rng::PresentationRng;

#[derive(Resource, Debug, Clone)]
pub struct PresentationRngState(pub PresentationRng);

pub mod actor_view;
pub mod animation;
pub mod camera;
pub mod map_view;
pub mod synchronization;

pub struct PresentationPlugin;

impl Plugin for PresentationPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PresentationRngState(PresentationRng::seeded(0)));
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

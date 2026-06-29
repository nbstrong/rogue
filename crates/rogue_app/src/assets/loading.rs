use std::fs;
use std::path::PathBuf;

use rogue_core::content::definitions::ActorDefinition;
use rogue_core::content::registry::ContentRegistry;

fn load_actor_definitions() -> Vec<ActorDefinition> {
    let path = PathBuf::from("assets/data/actors.ron");
    let text = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("failed to read {:?}: {}", path, error);
    });

    ron::from_str::<Vec<ActorDefinition>>(&text).unwrap_or_else(|error| {
        panic!("failed to parse {:?}: {}", path, error);
    })
}

pub fn load_content() -> ContentRegistry {
    let mut registry = ContentRegistry::default();

    for actor in load_actor_definitions() {
        registry
            .insert_actor(actor)
            .unwrap_or_else(|error| panic!("{}", error));
    }

    registry
}

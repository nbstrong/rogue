use std::fs;
use std::path::{Path, PathBuf};

use bread_and_iron::register_content;
use tactical_sim::content::definitions::ActorDefinition;
use tactical_sim::content::registry::ContentRegistry;

fn load_actor_definitions() -> Vec<ActorDefinition> {
    let path = asset_path();
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {:?}: {}", path, error));

    ron::from_str::<Vec<ActorDefinition>>(&text).unwrap_or_else(|error| {
        panic!("failed to parse {:?}: {}", path, error);
    })
}

fn asset_path() -> PathBuf {
    let relative_path = Path::new("assets/data/actors.ron");
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data/actors.ron");

    if manifest_path.exists() {
        manifest_path
    } else {
        relative_path.into()
    }
}

pub fn load_content() -> ContentRegistry {
    let mut registry = ContentRegistry::default();

    register_content(&mut registry, load_actor_definitions());

    registry
}

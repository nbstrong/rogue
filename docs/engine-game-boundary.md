# Engine-Game Boundary

This workspace separates reusable simulation code from Bread and Iron specific composition.

## Engine Owned

- `sim_core`
- `tactical_sim`

## Game Owned

- `bread_and_iron`
- `bread_and_iron_app`
- `assets`

## Dependency Direction

```text
bread_and_iron_app
├── bread_and_iron
└── tactical_sim
    └── sim_core

bread_and_iron
└── tactical_sim
    └── sim_core
```

`tactical_sim` must not depend on `bread_and_iron` or `bread_and_iron_app`.

## Ownership Rules

- Put deterministic simulation infrastructure in `sim_core`.
- Put tactical ECS mechanics in `tactical_sim`.
- Put game-specific scenario setup, content registration, and defeat policy in `bread_and_iron`.
- Put windowing, input, rendering, UI, asset loading, and save-file I/O in `bread_and_iron_app`.

If the engine is extracted to another repository later, the expected work should mostly be Cargo dependency rewrites and moving game-owned assets and app code out of the engine workspace.

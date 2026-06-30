# Agent Instructions

## Repository Access
https://github.com/nbstrong/rogue

* Always use the GitHub connector app when interacting with the repository.
* Do not use gh (Github CLI) or any other command-line tool to interact with the repository.
* Only perform read actions on the repository unless the user explicitly requests a repository write.
* Do not perform source-control operations. The user handles branches, commits, rebases, merges, and pushes.

## Build and Test Scope

Use the narrowest Cargo command that validates the files changed.

Do not build or test unrelated workspace crates.

### Documentation and asset-only changes

For changes limited to these paths:

```text
docs/**
README.md
AGENTS.md
assets/**
```

Do not run Cargo commands unless the change modifies data parsed or embedded at compile time.

### Core simulation changes

For changes limited to:

```text
crates/rogue_core/**
crates/rogue_core/tests/**
```

Run the narrowest relevant test first:

```bash
cargo test -p rogue_core <test-name>
```

For a specific integration-test file:

```bash
cargo test -p rogue_core --test combat
cargo test -p rogue_core --test deterministic_replay
cargo test -p rogue_core --test generation
```

Before completing a core-only task, run:

```bash
cargo test -p rogue_core
```

Do not build or test `rogue_app` for a core-only change unless a public core API used by the application changed.

### Application changes

For changes limited to:

```text
crates/rogue_app/**
crates/rogue_app/tests/**
```

For compile validation, run:

```bash
cargo check -p rogue_app --features dev
```

For a specific application integration test:

```bash
cargo test -p rogue_app --features dev --test app_loop <test-name>
```

Before completing an application task, run:

```bash
cargo test -p rogue_app --features dev
```

Run the application only when validating visual presentation, input, window startup, assets, UI, or runtime integration:

```bash
cargo run -p rogue_app --features dev
```

Do not run the application for ordinary compile validation.

### Manifest and cross-crate changes

For changes to:

```text
Cargo.toml
Cargo.lock
.cargo/**
```

or changes that alter public interfaces shared between `rogue_core` and `rogue_app`, run:

```bash
cargo test -p rogue_core
cargo test -p rogue_app --features dev
```

Run the full workspace validation only after the targeted commands succeed:

```bash
cargo test --workspace --features rogue_app/dev
```

## Formatting

Run formatting validation for Rust source changes:

```bash
cargo fmt --all -- --check
```

Do not run formatting for documentation-only changes.

## Command Restrictions

* Never run `cargo clean` unless the user explicitly requests it.
* Never delete the `target` directory as routine troubleshooting.
* Never run `cargo build --workspace` by default.
* Never run `cargo test --workspace --features rogue_app/dev` when a crate-specific test is sufficient.
* Never run a release build unless the user explicitly requests one.
* Never run both `cargo check` and `cargo test` for the same scope without a reason; tests already compile the affected targets.
* Do not use unqualified commands such as `cargo build` or `cargo test`. Always specify the intended package.
* Do not repeatedly rebuild after every small edit. Make the complete logical change, then run the narrowest applicable validation.
* Treat warnings and test failures as findings. Do not broaden the build scope merely to investigate an unrelated warning.

## Standard Completion Commands

Core-only task:

```bash
cargo fmt --all -- --check
cargo test -p rogue_core
```

Application-only task:

```bash
cargo fmt --all -- --check
cargo test -p rogue_app --features dev
```

Cross-crate or Cargo configuration task:

```bash
cargo fmt --all -- --check
cargo test -p rogue_core
cargo test -p rogue_app --features dev
cargo test --workspace --features rogue_app/dev
```

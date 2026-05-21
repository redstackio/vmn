# TODO: `vmn` Implementation Plan

This plan intentionally uses fewer, heavier sprints. The target is to finish a complete v1 in three focused coding sessions instead of spreading work across many small milestones.

## Current Status

- Sprint 1: implemented and locally verified.
- Sprint 2: implemented and locally verified.
- Sprint 3: release-ready; actual public publish is pending GitHub remote setup and crates.io account/token confirmation.

## Sprint 1: Usable Core MVP

Goal: Build a working Rust CLI with SQLite state, managed env creation, listing, activation path output, and zsh/fzf integration.

Definition of done:

- A developer can install locally, run `vmn init`, add the zsh snippet, create an environment, run `v`, pick it in fzf, and activate it in the current shell.

Tasks:

- Scaffold Rust crate with `clap` command structure.
- Add config/data directory resolution.
- Add SQLite connection setup with migrations.
- Implement `environments`, `packages`, and `schema_migrations` tables.
- Implement registry data layer with transactions and selector resolution.
- Generate stable environment ids.
- Implement `vmn init --shell zsh`.
- Implement `vmn list`, including `--fzf`, `--json`, and human output.
- Implement `vmn path <selector>`.
- Implement `vmn activate-path <selector>`.
- Implement `vmn create <name>`.
- Implement `vmn create <name> --here`.
- Support `--python <executable>` for creation.
- Capture basic Python and pip versions after create.
- Add zsh/fzf wrapper snippet that sources only the activation path.
- Add unit tests for registry CRUD, selector ambiguity, and output formatting.
- Add integration tests for init/list/path/create where feasible without depending on a user's shell.

Risks to resolve in this sprint:

- Verify `source "$(vmn activate-path <id>)"` works cleanly from a zsh function.

## Sprint 2: Full v1 Feature Completion And Hardening

Goal: Add discovery, inspection, refresh, cleanup, health checks, and safety guards.

Definition of done:

- The tool can manage both newly created and discovered venvs, inspect them, refresh package metadata, detect missing paths, remove/prune records safely, and pass the v1 success criteria in the PRD.

Tasks:

- Implement `vmn scan <dir>...`.
- Detect venv roots using `pyvenv.cfg`.
- Avoid descending into detected venv directories.
- Handle permission errors and symlink loops without crashing.
- Add `--max-depth`, `--dry-run`, and `--json` to scan.
- Implement missing-path marking.
- Implement `vmn info <selector>`.
- Add `--packages`, `--json`, and `--live` to info.
- Implement `vmn refresh <selector|--all>`.
- Capture package snapshots without slowing normal `info`.
- Implement `vmn remove <selector>`.
- Add `--delete-files` and `--yes` with deletion safeguards.
- Implement `vmn prune` with `--dry-run`, `--missing`, `--deleted`, and `--yes`.
- Implement `vmn doctor`.
- Add checks for fzf, Python, writable state directories, DB migrations, missing paths, activation scripts, and ambiguous names.
- Add robust stderr/stdout tests for machine-readable command contracts.
- Add tests for scan behavior, missing state, package refresh parsing, and deletion safeguards.

## Sprint 3: Public Crate Packaging And Release

Goal: Package `vmn` as a polished Rust binary crate and publish it to crates.io with the documentation and metadata people need to install and use it confidently.

Definition of done:

- A new user can find the crate on crates.io, run `cargo install vmn`, follow the README setup steps, initialize zsh/fzf integration, and successfully manage their first virtual environment.

Tasks:

- Confirm crates.io package name availability for `vmn`.
- If `vmn` is unavailable, choose an available crate package name while preserving the installed binary name `vmn`.
- Finalize `Cargo.toml` package metadata: name, version, edition, rust-version, description, readme, license/license-file, repository, keywords, categories, and include/exclude rules.
- Add or finalize `README.md`.
- Document installation with `cargo install`.
- Document runtime requirements: `fzf`, Python 3, and zsh for the provided integration.
- Document `vmn init --shell zsh` and the `v` workflow.
- Document managed vs scanned environments.
- Document all v1 commands with examples.
- Document SQLite/config/data paths and how to reset local state.
- Document deletion safeguards and cleanup behavior.
- Add troubleshooting for missing `fzf`, ambiguous selectors, missing paths, Python probing failures, and package refresh slowness.
- Add `LICENSE` or confirm SPDX-only licensing is sufficient for the chosen release model.
- Add `CHANGELOG.md` with an initial `0.1.0` release entry.
- Add crate-level docs suitable for docs.rs where helpful.
- Add `.gitignore` entries for build output and local generated state.
- Run `cargo fmt`.
- Run `cargo clippy --all-targets --all-features`.
- Run `cargo test --all-targets --all-features`.
- Run `cargo install --path .` and verify the installed binary works.
- Run `cargo package --list` and inspect packaged files.
- Run `cargo publish --dry-run`.
- Publish with `cargo publish`.
- Create a git tag for the published version.
- Record the release in `CHANGELOG.md` and the repository release notes.

Risks to resolve in this sprint:

- Crate name `vmn` may already be taken on crates.io.
- crates.io account/token setup may need to happen outside the coding session.
- README and crates.io metadata cannot be meaningfully changed for the published version without a new release, so the docs review must happen before publish.

Deferred beyond v1:

- Archive/resume.
- Native Rust TUI.
- Bash and fish integrations.
- Windows shell integration.
- uv/poetry-specific project intelligence.
- Import/export or sync of the registry.

# vmn

`vmn` is a fast local Python virtual environment manager for developers who work across many projects.

It keeps a SQLite registry of known virtual environments, gives you optional zsh/bash/fzf shell integration for switching between them, and can create, discover, inspect, refresh, remove, and prune environments from one CLI.

With shell integration installed, the daily workflow is:

```zsh
v
```

Pick an environment in fzf. `vmn` changes into that environment's project directory when one is known, then activates the venv in your current shell.

## Features

- Create centrally managed venvs.
- Create project-local `./.venv` environments.
- Scan project folders for existing `.venv` directories.
- Activate environments from anywhere with optional zsh/bash/fzf integration.
- Inspect Python, pip, package, path, status, and source metadata.
- Mark missing environments instead of silently deleting registry records.
- Safely remove registry entries and optionally delete venv directories.
- Use scriptable output contracts for shell integration.

## Requirements

- Rust/Cargo for installation.
- Python 3 with the standard `venv` module.
- Optional: `fzf` for the interactive shell picker.
- Optional: zsh or bash for generated shell integration.

`vmn` targets Unix-like developer environments: Linux, macOS, and WSL2. The CLI commands work without shell integration or fzf. Shell integration is only needed for the `v` picker to activate a venv in the current parent shell.

## Install

From crates.io, after the first public release:

```zsh
cargo install vmn
```

From a local checkout:

```zsh
cargo install --path .
```

Verify:

```zsh
vmn --version
vmn doctor
```

## Shell Setup

Initialize local state and print shell integration for your shell:

```zsh
vmn init --shell zsh
vmn init --shell bash
```

Append the printed snippet to your shell config:

```text
zsh:  ~/.zshrc
bash: ~/.bashrc
```

Then restart your shell:

```zsh
exec zsh
# or
exec bash
```

The snippet defines:

```text
v             open the VMN fzf picker, cd to the project, activate the venv
vd            deactivate the current Python venv
Ctrl-F        open the VMN picker
Ctrl-Shift-F  deactivate when supported by the terminal
Ctrl-X Ctrl-F deactivate fallback
```

Many terminals cannot distinguish `Ctrl-F` from `Ctrl-Shift-F`. When that happens, use `vd` or `Ctrl-X Ctrl-F` to deactivate.

If `fzf` is not installed, the CLI still works. Use:

```zsh
vmn list
source "$(vmn activate-path <selector>)"
```

## Quick Start

Create a managed environment:

```zsh
vmn create api
```

Create a project-local environment:

```zsh
cd ~/Projects/api
vmn create api --here
```

Scan existing project venvs:

```zsh
vmn scan ~/Projects
```

Open the picker:

```zsh
v
```

Deactivate:

```zsh
vd
```

or use Python venv's standard function:

```zsh
deactivate
```

## Managed vs Scanned Environments

`vmn` supports two common styles.

Managed environments live under the VMN data directory:

```text
~/.local/share/vmn/envs/<name>
```

Project-local environments live in your project:

```text
~/Projects/my-app/.venv
```

Use managed envs when you want named environments independent of a project folder. Use `--here` or `scan` when each project owns its own `.venv`.

## Commands

### `vmn pythons`

List Python interpreters discovered on `PATH`.

```zsh
vmn pythons
vmn pythons --json
```

Use this before creating a venv when multiple Python versions are installed.

### `vmn init`

Create config/data directories, initialize the SQLite registry, and print shell integration.

```zsh
vmn init --shell zsh
vmn init --shell bash
```

### `vmn list`

List registered environments.

```zsh
vmn list
vmn list --fzf
vmn list --json
vmn list --include-missing
vmn list --all
```

`--fzf` prints tab-separated rows for shell integration:

```text
<id> <name> <status> <path> <python_version> <last_used_at>
```

### `vmn create`

Create and register a new venv.

```zsh
vmn create api
vmn create api --here
vmn create api --path ~/scratch/api-venv
vmn create api --python 3.12
vmn create api --python python3.12
vmn create api --python /opt/homebrew/bin/python3.12
```

By default, `vmn create <name>` creates a managed environment. `--here` creates `./.venv` in the current directory. `--python` accepts an executable name, a full executable path, or a version selector such as `3.11` or `3.12`.

### `vmn scan`

Discover existing venvs under one or more directories.

```zsh
vmn scan ~/Projects
vmn scan ~/Projects ~/Work --max-depth 5
vmn scan ~/Projects --dry-run
vmn scan ~/Projects --json
```

Scanning detects venv roots with `pyvenv.cfg` and an activation script. Permission-denied paths are skipped and counted instead of crashing the scan.

### `vmn info`

Show cached metadata for one environment.

```zsh
vmn info api
vmn info api --packages
vmn info api --json
vmn info api --live
```

Default `info` reads SQLite only. Use `--live` to probe Python/pip before printing.

### `vmn refresh`

Refresh Python, pip, and package metadata.

```zsh
vmn refresh api
vmn refresh --all
```

Package capture can be slower than registry reads, so `vmn` does it explicitly through `refresh` or `info --live`.

### `vmn path`

Print only the venv root path.

```zsh
vmn path api
```

### `vmn project-dir`

Print the associated project directory when known. Managed envs without a project print nothing.

```zsh
vmn project-dir api
```

### `vmn activate-path`

Print only the activation script path and update `last_used_at`.

```zsh
source "$(vmn activate-path api)"
```

The shell integration uses this internally. `vmn` does not print shell code to be evaluated.

### `vmn doctor`

Check local health.

```zsh
vmn doctor
```

Checks include writable config/data directories, database availability, Python, optional fzf availability, missing active paths, activation scripts, and duplicate names.

### `vmn remove`

Remove an environment from active use.

```zsh
vmn remove api
vmn remove api --delete-files
vmn remove api --delete-files --yes
```

Without `--delete-files`, `vmn` marks the environment deleted in the registry and leaves files alone.

With `--delete-files`, `vmn` requires the target directory to look like a venv before deleting it. It refuses to delete paths without `pyvenv.cfg` and an activation script.

### `vmn prune`

Delete stale registry records.

```zsh
vmn prune --dry-run
vmn prune --missing
vmn prune --deleted
vmn prune --deleted --yes
```

## Selectors

Most commands accept:

- a full environment id
- a unique id prefix
- a unique name

If a name or prefix is ambiguous, `vmn` fails and lists matching ids and paths.

## Data Locations

On Linux and WSL2/XDG-style systems:

```text
~/.config/vmn/config.toml
~/.local/share/vmn/vmn.db
~/.local/share/vmn/envs/
```

On macOS, `vmn` uses the platform-standard application support directory through Rust's `directories` crate, typically under:

```text
~/Library/Application Support/vmn/
```

For tests or isolated usage, override locations:

```zsh
export VMN_CONFIG_DIR=/tmp/vmn-config
export VMN_DATA_DIR=/tmp/vmn-data
```

Reset all local VMN state:

```zsh
rm -rf ~/.config/vmn ~/.local/share/vmn
```

This deletes VMN-managed environments under `~/.local/share/vmn/envs/`. It does not delete project-local `.venv` directories elsewhere.

## Troubleshooting

### `fzf` is missing

`fzf` is optional for the CLI and required for the `v` picker. Install fzf if you want interactive selection, then rerun:

```zsh
vmn doctor
```

Without fzf, use direct activation:

```zsh
vmn list
source "$(vmn activate-path <selector>)"
```

### `v` is not found

Restart your shell:

```zsh
exec zsh
# or
exec bash
```

Then check:

```zsh
type v
```

### The picker opens but activation does not stick

Activation must happen in the parent shell. Use the shell function from `vmn init --shell zsh` or `vmn init --shell bash`, not `vmn` directly.

### I selected an env but it did not cd where I expected

Check the project directory:

```zsh
vmn info <name-or-id>
vmn project-dir <name-or-id>
```

Managed envs usually have no project directory. Project-local envs created with `--here` or discovered by `scan` should have one.

### A path is missing

If a venv is deleted manually, `vmn` marks it missing instead of silently deleting it.

```zsh
vmn doctor
vmn prune --missing --dry-run
vmn prune --missing --yes
```

### A selector is ambiguous

Use the id prefix shown by:

```zsh
vmn list --all
```

### Package metadata looks stale

Refresh it:

```zsh
vmn refresh <name-or-id>
```

## Development

Run checks:

```zsh
cargo fmt
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

Install locally:

```zsh
cargo install --path . --force
```

Package check:

```zsh
cargo package --list
cargo publish --dry-run
```

## License

MIT. See [LICENSE](LICENSE).

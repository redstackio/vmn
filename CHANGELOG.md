# Changelog

All notable changes to `vmn` will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-05-21

### Added

- Added `vmn pythons` to list Python interpreters discovered on `PATH`.
- Added Python version selectors for environment creation, so `vmn create api --python 3.12` can resolve an installed `python3.12` interpreter.
- Added support for full Python executable paths in `vmn create --python`.

## [0.1.0] - 2026-05-21

### Added

- SQLite-backed registry for known Python virtual environments.
- zsh/fzf integration through `vmn init --shell zsh`.
- `v` shell function for selecting, changing into the project directory, and activating an environment.
- `vd` shell function for deactivating the current Python virtual environment.
- Keybindings for zsh integration:
  - `Ctrl-F` opens the VMN picker.
  - `Ctrl-Shift-F` deactivates when the terminal supports CSI-u key encoding.
  - `Ctrl-X Ctrl-F` deactivates as a portable fallback.
- Managed environment creation with `vmn create <name>`.
- Project-local environment creation with `vmn create <name> --here`.
- Existing venv discovery with `vmn scan <dir>...`.
- Environment metadata inspection with `vmn info`.
- Package and Python metadata refresh with `vmn refresh`.
- Health checks with `vmn doctor`.
- Safe registry/file cleanup with `vmn remove` and `vmn prune`.
- Scriptable outputs for shell integration: `vmn list --fzf`, `vmn path`, `vmn project-dir`, and `vmn activate-path`.

# Engineering PRD: `vmn` Rust Python Venv Manager

Status: Draft v1
Date: 2026-05-21

## 1. Project Summary

`vmn` is a fast local Python virtual environment manager for developers who work across many projects and want one CLI-driven way to create, discover, inspect, activate, and remove virtual environments.

The core product is a Rust CLI backed by a local SQLite registry and optional zsh/bash/fzf shell integration. Rust owns durable state, scanning, creation, deletion, and environment inspection. The shell wrapper owns activation because only the parent shell can modify its own environment.

The default daily workflow should be:

```zsh
v
```

That opens an fzf picker of known environments. Selecting one activates it in the current shell.

## 2. Goals

- Provide a fast optional `v` workflow for activating known venvs from anywhere.
- Support both centrally managed venvs and discovered project-local `.venv` directories.
- Make creating new environments easy without hiding where files are created.
- Track metadata locally: path, display name, project directory, status, Python version, pip version, package snapshots, creation time, last used time, and source.
- Provide safe inspection and cleanup commands for stale or missing environments.
- Keep command output scriptable and predictable.

## 3. Non-Goals For v1

- No archive/resume feature.
- No native Rust TUI. fzf is the interactive selector for v1.
- No attempt by the Rust process to directly modify the parent shell.
- No automatic deletion of missing registry entries.
- No replacement for pip, uv, poetry, pyenv, or conda.
- No remote sync or multi-machine registry.

## 4. Architecture

### Rust CLI: `vmn`

The Rust binary handles:

- CLI argument parsing.
- SQLite migrations and registry access.
- Venv creation and deletion.
- Filesystem scanning.
- Python/pip metadata probing.
- Machine-readable command output.
- Human-readable diagnostics on stderr.

The Rust binary must never print shell code that users are expected to `eval`.

### Shell Integration: `v`

The shell wrapper handles:

- Calling `vmn list --fzf`.
- Passing results to `fzf`.
- Extracting the selected environment id.
- Calling `vmn activate-path <id>`.
- Running `source "$activate_path"` in the current shell.

This keeps shell execution explicit and avoids executing arbitrary stdout from the Rust binary.

### Data Locations

Use platform-appropriate XDG-style locations through a Rust directory helper crate.

Primary Unix/Linux layout:

```text
~/.config/vmn/config.toml
~/.local/share/vmn/vmn.db
~/.local/share/vmn/envs/
```

macOS should use the platform-standard application support directory through the Rust directory helper crate, typically under `~/Library/Application Support/vmn/`.

The managed env directory is only for `vmn`-created central environments. Scanned project-local environments stay in their original project directories.

## 5. Output Contract

`vmn` commands must keep stdout stable and scriptable.

- stdout is for requested data only.
- stderr is for logs, warnings, progress, and errors.
- `--json` emits valid JSON only.
- `--fzf` emits tab-separated rows designed for fzf.
- Commands used by shell wrappers should print exactly one path or no output.

Examples:

```zsh
vmn activate-path api
# /home/ermis/products/api/.venv/bin/activate
```

```zsh
source "$(vmn activate-path api)"
```

## 6. SQLite Registry

SQLite is the v1 registry store. It is fast enough for this use case, avoids hand-rolled JSON durability problems, and gives us clean querying as metadata grows.

Implementation requirements:

- Use transactions for all mutations.
- Set a reasonable busy timeout for concurrent terminal usage.
- Run migrations on startup or `vmn init`.
- Treat the database as local user state, not a networked shared database.

Recommended Rust crate: `rusqlite` with bundled SQLite for predictable `cargo install` behavior.

Initial schema:

```sql
CREATE TABLE schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
);

CREATE TABLE environments (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  project_dir TEXT,
  source TEXT NOT NULL CHECK (source IN ('managed', 'scanned', 'manual')),
  status TEXT NOT NULL CHECK (status IN ('active', 'missing', 'deleted')),
  python_version TEXT,
  pip_version TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_used_at TEXT,
  last_scanned_at TEXT
);

CREATE TABLE packages (
  env_id TEXT NOT NULL,
  name TEXT NOT NULL,
  version TEXT NOT NULL,
  captured_at TEXT NOT NULL,
  PRIMARY KEY (env_id, name),
  FOREIGN KEY (env_id) REFERENCES environments(id) ON DELETE CASCADE
);

CREATE INDEX idx_environments_name ON environments(name);
CREATE INDEX idx_environments_status ON environments(status);
CREATE INDEX idx_environments_last_used ON environments(last_used_at);
```

### Environment Identity

- `id` is the canonical selector and should be a stable ULID string.
- `name` is a human-readable display label and may collide.
- CLI commands may accept a full id, unique id prefix, or unique name.
- Ambiguous selectors must produce an error that lists matching ids and paths.
- `path` must be unique.

## 7. CLI Commands

### `vmn init`

Creates config/data directories, initializes the SQLite database, and prints a shell integration snippet.

Options:

- `--shell zsh`
- `--shell bash`
- `--print-shell`

### `vmn list`

Lists registered environments.

Options:

- `--fzf`: tab-separated output for fzf.
- `--json`: JSON output.
- `--include-missing`: include environments marked missing.
- `--all`: include active, missing, and deleted records.

Default human output should be concise and sorted by last used, then name.

`--fzf` output format:

```text
<id>\t<name>\t<status>\t<path>\t<python_version>\t<last_used_at>
```

### `vmn activate-path <selector>`

Prints the activation script path for one active environment and updates `last_used_at`.

Behavior:

- Fails if the selector is ambiguous.
- Fails if the venv path is missing.
- Marks the environment as `missing` if the path no longer exists.
- Prints only the activation script path to stdout on success.

### `vmn path <selector>`

Prints the venv root path only.

### `vmn project-dir <selector>`

Prints the associated project directory when known. Prints no output for centrally managed environments without a project directory.

### `vmn create <name>`

Creates a new Python virtual environment and registers it.

Modes:

- Default: create a managed environment at `~/.local/share/vmn/envs/<name>`.
- `--here`: create `./.venv` in the current directory and register it.
- `--path <dir>`: create a venv at an explicit path, or register it if it is already a valid venv.

Options:

- `--python <executable>`: Python executable to use.
- `--no-activate-output`: do not print activation path after creation.

Behavior:

- Runs `<python> -m venv <path>`.
- Records Python and pip versions when available.
- Prints the activation script path on success unless suppressed.

### `vmn scan <dir>...`

Discovers existing Python venvs under one or more directories.

Detection:

- Prefer detecting venv roots by `pyvenv.cfg`.
- Validate expected activation script exists for the current platform.
- Avoid descending into known venv directories once detected.
- Ignore permission-denied paths without crashing.
- Do not follow symlink loops.

Options:

- `--max-depth <n>`
- `--dry-run`
- `--json`

Behavior:

- Inserts new venvs.
- Updates existing records by path.
- Marks previously scanned records missing only when explicitly checked and absent.

### `vmn info <selector>`

Shows detailed metadata for one environment.

Default output reads cached SQLite metadata only.

Options:

- `--packages`: include cached package snapshot.
- `--json`: JSON output.
- `--live`: probe the environment before printing.

Example:

```text
Name: api
ID: 01HX...
Status: active
Path: /home/ermis/products/api/.venv
Project: /home/ermis/products/api
Python: 3.12.3
Pip: 24.2
Packages: 83
Source: scanned
Last used: 2026-05-21 10:42
```

### `vmn refresh <selector|--all>`

Probes environments and refreshes cached metadata.

Behavior:

- Reads Python version.
- Reads pip version.
- Captures package list using pip.
- Marks missing environments as `missing`.

Package capture may be slower than registry reads, so it should be explicit through `refresh` or `info --live`.

### `vmn remove <selector>`

Removes or deletes an environment.

Default behavior:

- Unregisters the environment from active use without deleting files.
- Marks status as `deleted`.

Options:

- `--delete-files`: delete the venv directory after confirmation.
- `--yes`: skip confirmation.

Deletion safeguards:

- Refuse to delete paths outside known venv roots.
- Require `pyvenv.cfg` before deleting a directory.
- Never delete a project directory when the venv path is `./.venv`.

### `vmn doctor`

Checks the local installation and registry health.

Checks:

- Database exists and migrations are current.
- Config/data directories are writable.
- `fzf` is installed when the user wants interactive shell picking. Missing fzf should be reported as an optional warning, not a hard CLI failure.
- A usable Python executable is available.
- Registered active paths still exist.
- Activation scripts exist.
- Duplicate or ambiguous names are reported.

### `vmn prune`

Cleans stale registry state intentionally.

Options:

- `--dry-run`
- `--missing`: remove records marked missing.
- `--deleted`: remove records marked deleted.
- `--yes`: skip confirmation.

## 8. Shell/fzf Integration

The CLI works without shell integration. Shell integration is optional, but needed for the `v` picker to activate an environment in the current parent shell.

`vmn init --shell zsh` should print a zsh snippet similar to:

```zsh
# VMN zsh integration
v() {
  local selected id project_dir activate_path

  if ! command -v fzf >/dev/null 2>&1; then
    echo 'vmn: fzf is required for the interactive picker. Install fzf or use "vmn list" and source "$(vmn activate-path <selector>)".' >&2
    return 1
  fi

  selected=$(vmn list --fzf | fzf --delimiter=$'\t' --with-nth=2,3,4 --height=40% --reverse) || return
  id=${selected%%$'\t'*}
  [[ -z "$id" ]] && return

  project_dir=$(vmn project-dir "$id") || return
  if [[ -n "$project_dir" ]]; then
    cd "$project_dir" || return
  fi

  activate_path=$(vmn activate-path "$id") || return
  source "$activate_path"
}

vd() {
  if (( $+functions[deactivate] )); then
    deactivate
  else
    echo "No active Python virtual environment." >&2
    return 1
  fi
}

_vmn_widget() {
  zle -I
  v
  zle reset-prompt
}

_vmn_deactivate_widget() {
  zle -I
  vd
  zle reset-prompt
}

if [[ -o interactive ]]; then
  zle -N _vmn_widget 2>/dev/null
  zle -N _vmn_deactivate_widget 2>/dev/null
  bindkey '^F' _vmn_widget 2>/dev/null
  bindkey $'\e[70;6u' _vmn_deactivate_widget 2>/dev/null
  bindkey '^X^F' _vmn_deactivate_widget 2>/dev/null
fi
```

The function may evolve, but the important contract is that it changes directory using `vmn project-dir`, sources a path returned by `vmn activate-path`, and does not evaluate arbitrary command text from `vmn`. `vd` is a convenience wrapper around Python venv's standard `deactivate` function.

`vmn init --shell bash` should print the same `v` and `vd` functions with Bash readline bindings:

- `Ctrl-F`: open the VMN picker.
- `Ctrl-Shift-F`: deactivate when the terminal emits CSI-u keys.
- `Ctrl-X Ctrl-F`: portable deactivate fallback.

## 9. Edge Cases And Constraints

### Missing Paths

If an environment path disappears, mark the environment as `missing`. Do not silently delete the record.

### Duplicate Names

Names are display labels. If a command receives an ambiguous name, it must fail with a useful message listing matching ids and paths.

### Permission Errors During Scan

Permission-denied paths should be skipped and counted. The command should finish with a warning on stderr, not crash.

### Cross-Platform Paths

The v1 shell integrations target zsh/bash/fzf on Unix-like systems: Linux, macOS, and WSL2. Core path construction should still use `PathBuf` and platform-aware activation script locations:

- Unix: `<venv>/bin/activate`
- Windows: `<venv>/Scripts/activate`

Windows shell integration is not required for v1.

### Concurrency

Multiple terminals may invoke `vmn` at the same time. Use SQLite transactions and busy timeouts. Avoid long write transactions during filesystem scans and package refreshes.

### Slow Operations

Registry reads should be fast. Filesystem scans and package probing may be slow and should be explicit. `vmn info` should not run pip unless `--live` is passed.

## 10. Dependencies

Likely Rust dependencies:

- `clap`: CLI parsing.
- `rusqlite`: SQLite access.
- `serde` and `serde_json`: JSON output.
- `walkdir` or `ignore`: filesystem scanning.
- `directories`: platform-specific config/data directories.
- `time` or `chrono`: timestamps.
- `ulid`: stable, sortable environment ids.
- `which`: external executable discovery for doctor/init.

External runtime dependencies:

- Python 3 with the standard `venv` module.
- Optional `fzf` for the interactive shell picker.
- Optional zsh or bash for generated shell integration.

## 11. Packaging And Distribution

The project should be packaged as a Rust binary crate intended for installation through Cargo:

```zsh
cargo install vmn
```

The published package should install a `vmn` binary. If the `vmn` crate name is unavailable on crates.io, choose an available package name while keeping the installed binary name `vmn`.

### Cargo Manifest Requirements

Before crates.io publication, `Cargo.toml` must include:

- `name`
- `version`
- `edition`
- `rust-version`
- `description`
- `readme = "README.md"`
- `license` or `license-file`
- `repository`
- `keywords` with no more than five crates.io-compatible values
- `categories` with crates.io-compatible category slugs
- `exclude` or `include` rules if needed to avoid packaging local test fixtures or generated state

The package should use SemVer from the first release. Initial public release should be `0.1.0` unless the project reaches a stronger compatibility commitment before publication.

### Required Project Files

The repository should include:

- `README.md`: user-facing installation, setup, daily workflow, commands, examples, troubleshooting, and safety notes.
- `LICENSE` or clearly declared SPDX license in `Cargo.toml`.
- `CHANGELOG.md`: release notes starting with `0.1.0`.
- `.gitignore`: excludes build output and local generated state.
- Optional `docs/`: longer usage guides if README becomes too large.

### README Requirements

The README must make first-time adoption straightforward:

- What `vmn` does and does not do.
- Install instructions with `cargo install`.
- Runtime requirements: Python 3, optional `fzf`, optional zsh/bash shell integration.
- `vmn init --shell zsh` and `vmn init --shell bash` setup flows.
- Daily usage with `v`.
- Examples for `create`, `create --here`, `scan`, `list`, `info`, `refresh`, `doctor`, `remove`, and `prune`.
- Explanation of managed vs scanned environments.
- Explanation of SQLite registry location and config/data paths.
- Safety model for deletion.
- Troubleshooting section for missing `fzf`, ambiguous names, missing paths, and Python/pip probing failures.

### Release Gate

Before publishing:

- Run formatting, linting, and the full test suite.
- Verify `cargo install --path .` installs and runs `vmn`.
- Run `cargo package --list` and confirm no unwanted files are included.
- Run `cargo publish --dry-run`.
- Confirm README rendering and package metadata are suitable for crates.io.
- Confirm docs.rs will have useful crate-level documentation where relevant.
- Tag the release commit after publication.

Final publication is done with:

```zsh
cargo publish
```

## 12. Success Criteria

v1 is complete when:

- `cargo install --path .` installs a working `vmn`.
- `vmn init --shell zsh` and `vmn init --shell bash` create local state and print usable shell snippets.
- `v` opens fzf and activates a selected environment in the current zsh or bash session.
- `vmn create <name>` creates a managed environment and registers it.
- `vmn create <name> --here` creates and registers `./.venv`.
- `vmn scan ~/Projects` discovers existing project venvs.
- `vmn info <selector>` shows cached environment details.
- `vmn refresh <selector>` updates Python, pip, and package metadata.
- `vmn doctor` reports missing paths and dependency issues.
- `vmn remove` and `vmn prune` clean state safely.
- Unit/integration tests cover registry operations, selector resolution, scanning, command output contracts, and deletion safeguards.
- `README.md`, license, changelog, and Cargo metadata are ready for public use.
- `cargo publish --dry-run` passes.
- The crate is published to crates.io or has a documented blocker if the final name/account setup is not ready.

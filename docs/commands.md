# Command Reference

## Global Flags

| Flag | Description |
|------|-------------|
| `--workspace <PATH>` | Path to NixOS config workspace (auto-detects if omitted) |
| `-q, --quiet` | Suppress informational messages |
| `-v, --verbose` | Increase verbosity (-v verbose, -vv trace) |
| `-y, --yes` | Skip confirmation prompts |

---

## Commands

### `nixman status`

Quick workspace and system overview — workspace path, kind (flake/legacy), hostname, package count, pending changes, git status.

### `nixman check`

Validate configuration without building. Runs `nix eval` (flake) or `nix-instantiate --parse` (legacy). Catches syntax errors and assertion failures in seconds.

### `nixman doctor`

Post-rebuild health check. Tests: network (gateway reachable), DNS (resolves nixos.org), display manager, audio service, failed systemd units, filesystem usage (>90% warning).

### `nixman rebuild [mode]`

Run `nixos-rebuild`. Modes: `switch` (default), `boot`, `test`, `build`.

| Flag | Description |
|------|-------------|
| `--explain` | On failure, run the error through `nixman explain` |
| `--rollback-on-fail` | Auto-rollback to previous generation if build fails |

### `nixman option get <path>`

Read the current value of a NixOS option from config files.

### `nixman option set <path> <value>`

Set an option value. Preserves comments and formatting (AST-preserving edit).

| Flag | Description |
|------|-------------|
| `--stdin` | Read value from stdin instead of argument |
| `--dry-run` | Show unified diff of what would change |
| `--stage` | Stage the change instead of writing immediately |

### `nixman option remove <path>`

Remove an option from configuration.

| Flag | Description |
|------|-------------|
| `--dry-run` | Show what would change |

### `nixman option search <query>`

Search all available NixOS options (builds index on first use).

### `nixman option browse [prefix]`

Browse options under a dotted prefix (e.g., `services.nginx`).

### `nixman option show <path>`

Show full metadata for a specific option (type, default, description).

### `nixman packages list`

List packages declared in `environment.systemPackages`.

### `nixman packages search <query>`

Search nixpkgs for packages matching a query.

### `nixman packages add <name>`

Add a package to `environment.systemPackages`.

| Flag | Description |
|------|-------------|
| `--no-verify` | Skip package name verification against nixpkgs |
| `--file <path>` | Target file (overrides auto-detection) |
| `--dry-run` | Show what would change |
| `--stage` | Stage instead of applying immediately |

### `nixman packages remove <name>`

Remove a package from `environment.systemPackages`.

| Flag | Description |
|------|-------------|
| `--file <path>` | Target file (overrides auto-detection) |
| `--dry-run` | Show what would change |
| `--stage` | Stage instead of applying immediately |

### `nixman pending list`

Show all staged changes waiting to be applied.

### `nixman pending apply`

Apply all staged changes to disk.

### `nixman pending discard`

Discard all staged changes.

### `nixman intent propose`

Propose a batch of changes and get a validated plan with conflict detection.

| Flag | Description |
|------|-------------|
| `--set <path=value>` | Options to set (repeatable) |
| `--add-package <name>` | Packages to add (repeatable) |
| `--remove-package <name>` | Packages to remove (repeatable) |

### `nixman intent show`

Show the last proposed plan.

### `nixman intent apply`

Apply the validated plan.

### `nixman intent discard`

Discard the current plan.

### `nixman try apply`

Apply temporary changes with auto-revert. Uses `nixos-rebuild test` (doesn't change boot default). A systemd timer reverts changes after the timeout.

| Flag | Description |
|------|-------------|
| `--set <path=value>` | Options to set temporarily (repeatable) |
| `--timeout <seconds>` | Seconds before auto-revert (default: 120) |

### `nixman try confirm`

Confirm temporary changes — makes them permanent via `nixos-rebuild switch`.

### `nixman explain [error]`

Explain a Nix error in plain English with fix commands. Recognizes: option renamed/removed, assertion failures, undefined variables, infinite recursion, package collisions, syntax errors, hash mismatches, missing attributes, permission denied, disk full.

| Flag | Description |
|------|-------------|
| `--stdin` | Read error from stdin (for piping) |

### `nixman migrate`

Scan config files for deprecated options across NixOS versions.

| Flag | Description |
|------|-------------|
| `--fix` | Auto-fix renameable options |

### `nixman services list`

List all systemd services with status.

### `nixman services get <unit>`

Show status of a specific service.

### `nixman services start|stop|restart <unit>`

Control a systemd service.

### `nixman services logs <unit>`

Show journal logs for a service.

| Flag | Description |
|------|-------------|
| `-n, --lines <N>` | Number of lines (default: 50) |

### `nixman generations list`

List all NixOS system generations.

### `nixman generations diff <from> <to>`

Show package differences between two generations.

### `nixman generations rollback <number>`

Activate a previous generation.

### `nixman generations gc`

Delete old generations and run garbage collection.

| Flag | Description |
|------|-------------|
| `--keep <N>` | Number of most-recent generations to keep |

### `nixman history`

Enhanced generation history with optional package change context.

| Flag | Description |
|------|-------------|
| `--diff` | Show package changes between consecutive generations |

### `nixman flake list`

List all flake inputs with URLs, revisions, and age.

### `nixman flake show`

Show current flake metadata (hostname, nixpkgs rev, lock hash).

### `nixman flake update [input]`

Update one or all flake inputs. Invalidates the option index cache.

### `nixman diff`

Show uncommitted git changes in the workspace.

| Flag | Description |
|------|-------------|
| `--staged` | Show only file-backed staged changes |

### `nixman hm status`

Show Home Manager workspace status.

### `nixman hm option get|set|remove|search <...>`

Same as `nixman option` but targeting Home Manager's `home.nix`.

### `nixman hm packages list|search|add|remove <...>`

Same as `nixman packages` but targeting `home.packages`.

### `nixman hm rebuild <mode>`

Run Home Manager rebuild (switch/build/boot/test).

### `nixman workspace detect`

Auto-detect NixOS configuration location and display workspace info.

### `nixman workspace wizard`

Run first-time setup wizard to create a new flake workspace.

### `nixman schema`

Output full command schema as JSON (for AI agent integration).

### `nixman completions <shell>`

Generate shell completions (bash, zsh, fish).

---

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Error |
| 2 | Usage error (bad arguments) |
| 3 | No-op (requested change already applied) |

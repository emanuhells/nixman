<div align="center">

# nixman

NixOS config management. Safe by default. Scriptable by design.

[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![CI](https://img.shields.io/github/actions/workflow/status/emanuhells/nixman/ci.yml?style=flat-square)](https://github.com/emanuhells/nixman/actions)
[![NixOS](https://img.shields.io/badge/NixOS-25.11-5277C3?style=flat-square&logo=nixos)](https://nixos.org)

[Report a bug](https://github.com/emanuhells/nixman/issues)

</div>

---

## Quick Start

Add to your flake:

```nix
{
  inputs.nixman.url = "github:emanuhells/nixman";
}

# configuration.nix
{ inputs, ... }: {
  imports = [ inputs.nixman.nixosModules.default ];
  programs.nixman.enable = true;
}
```

Or run directly:

```bash
nix run github:emanuhells/nixman -- status
```

## Features

| | Feature | Description |
|---|---------|-------------|
| 🛡️ | `nixman try` | Experiment with changes — auto-reverts if you don't confirm |
| 🏥 | `nixman doctor` | Post-rebuild health check (network, DNS, display, audio, services, filesystems) |
| ✅ | `nixman check` | Pre-rebuild validation — catches conflicts in seconds |
| 💬 | `nixman explain` | Nix errors → plain English plus fix commands |
| 🔄 | `nixman migrate` | Detect and fix deprecated options across NixOS versions |
| 📝 | AST-preserving edits | Comments and formatting survive every change |
| 📂 | Multi-file resolution | Works with modular configs — picks the right file without asking |
| 📦 | Package management | Search nixpkgs, add/remove with verification, `--dry-run`, and `--stage` |
| 🏠 | Home Manager | Manage user configs with `nixman hm` — same safety features, separate config |
| 🤖 | MCP server | AI-native tool interface — use nixman from Claude, Cursor, OpenCode |
| ⚙️ | Agent-native CLI | `--stdin`, `--yes`, `--dry-run`, `--stage`, `schema` command, exit code 3 for idempotent no-ops, all JSON output |

## Comparison: nixman vs nixos-cli

`nixos-cli` (nix-community) is the main alternative for NixOS CLI tooling. Here's how they differ:

| Feature | nixman | nixos-cli |
|---------|--------|-----------|
| Config file editing | ✅ AST-preserving | ❌ |
| `nixos-rebuild` wrapper | ✅ | ✅ |
| Safety (try/check/doctor/explain) | ✅ | ❌ |
| Package management | ✅ | ❌ |
| Home Manager support | ✅ | ❌ |
| MCP server | ✅ | ❌ |
| Generation management | ✅ | ✅ TUI |
| Option search | ✅ CLI | ✅ TUI |

nixman focuses on safe, scriptable configuration changes that leave your Nix files intact. `nixos-rebuild` wrappers are table stakes — the real differentiator is what happens *before* you build.

## Architecture

```
nixman/
├── crates/
│   ├── nixman-core/      # Domain logic (parser, intent engine, builders)
│   ├── nixman-cli/       # CLI binary (clap, 20 commands, ~2.4 MB)
│   └── nixman-mcp/       # MCP server (stdio/HTTP)
├── nix/                  # Nix packaging (cli.nix, mcp.nix, module.nix)
└── flake.nix
```

**`nixman-core`** holds all domain logic — AST editing via `rnix-parser`, option index, intent engine, builders. **`nixman-cli`** wraps it in 20 clap commands. **`nixman-mcp`** exposes the same tools via MCP stdio/HTTP transport for AI agents.

## Contributing

```bash
git clone https://github.com/emanuhells/nixman
cd nixman
nix develop
cargo build -p nixman-cli
cargo test --workspace
```

We review PRs within 48 hours. See [CONTRIBUTING.md](CONTRIBUTING.md) for the full guide.

## License

MIT — see [LICENSE](LICENSE)

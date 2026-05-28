# Contributing to nixman

## Prerequisites

| Tool | Notes |
|------|-------|
| [Nix](https://nixos.org/download) | Required — provides the entire dev toolchain |
| Flakes enabled | Add `experimental-features = nix-command flakes` to `~/.config/nix/nix.conf` |
| Git | Standard version control |

You do **not** need Rust pre-installed — `nix develop` provides everything.

---

## Development Setup

```bash
git clone https://github.com/emanuhells/nixman
cd nixman
nix develop
cargo build -p nixman-cli
cargo test --workspace
```

---

## Workspace Layout

```
crates/
├── nixman-core/   # Domain logic library (parser, builders, intent engine)
├── nixman-cli/    # CLI binary (clap, 20 commands)
└── nixman-mcp/    # MCP server (stdio/HTTP for AI agents)
nix/
├── cli.nix        # Nix derivation for CLI
├── mcp.nix        # Nix derivation for MCP server
└── module.nix     # NixOS module (programs.nixman.enable)
```

**Where does new code go?**

- **Domain logic** (NixOS operations, parsing, package management) → `crates/nixman-core/`
- **CLI commands** → `crates/nixman-cli/src/commands/`
- **MCP tools** → `crates/nixman-mcp/src/server.rs`

---

## Code Style

- **Formatter:** `cargo fmt --all` before committing
- **Linter:** `cargo clippy --workspace`
- **Naming:** standard Rust conventions
- **Error handling:** use typed errors; avoid `unwrap()` outside tests

---

## Running Tests

```bash
cargo test --workspace
```

---

## Commit Conventions

[Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <short summary>
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`

Scopes: `packages`, `services`, `nix_parser`, `generations`, `workspace`, `ci`, etc.

---

## Pull Request Process

1. Fork and create a feature branch from `main`
2. Implement your change with tests
3. Run: `cargo fmt --all && cargo test --workspace`
4. Commit using Conventional Commits
5. Open a PR against `main` — CI must pass
6. One maintainer approval required to merge

---

## Reporting Issues

Include:
- nixman version (`nixman --version`)
- NixOS version (`nixos-version`)
- Steps to reproduce
- Actual vs. expected behaviour

---

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).

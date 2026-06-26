# Better Codex

Better Codex is a fork of OpenAI's open source Codex CLI, oriented toward a new
terminal application rather than a lightly customized copy of the upstream CLI.

The goal is to keep the useful Codex backend pieces, including model access,
session orchestration, tool execution, sandboxing, and protocol code, while
building a full-screen TUI that feels like a standalone app inside the terminal.
The product direction is closer to OpenCode, btop, or other dense terminal
workspaces than to a prompt-first command-line assistant.

## Direction

- Build a terminal-native application with persistent layout, panels, keyboard
  navigation, status surfaces, and inspectable agent activity.
- Reuse Codex's Rust backend where it is still the right foundation.
- Avoid coupling new UI work to inherited CLI flows when those flows get in the
  way of an app-like experience.
- Keep the codebase smaller and more focused by removing upstream automation,
  release plumbing, and repository-service files that are not useful for this
  fork.

## Development

Most Rust work happens inside `codex-rs/`.

```sh
cd codex-rs
just fmt
just test -p codex-tui
```

Follow `AGENTS.md` for repository-specific rules. In particular, new work should
prefer clear backend/UI boundaries, focused crates or modules, and local
development workflows over inherited upstream CI assumptions.

## License

The original project is licensed under Apache-2.0. See [LICENSE](LICENSE).

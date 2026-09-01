# AGENTS.md

`AGENTS.md` files give coding agents durable instructions for a repository or
subtree. Better Codex loads them through the retained Codex backend and includes
the applicable instructions when a session starts work in that directory.

Put broad project conventions in the repository root. Add a nested
`AGENTS.md` only when a subtree needs genuinely different build, testing, or
style guidance. Keep instructions current and verifiable: document enduring
constraints and ownership boundaries rather than one-off task steps.

This repository's own [root `AGENTS.md`](../AGENTS.md) defines the architecture,
Rust conventions, testing workflow, and API compatibility rules contributors
must follow.

See the [Codex `AGENTS.md` guide](https://developers.openai.com/codex/guides/agents-md)
for discovery and precedence details inherited by Better Codex.

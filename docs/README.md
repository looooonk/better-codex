# Better Codex documentation

This directory documents the Better Codex fork. The application retains many
Codex backend interfaces, so some pages link to upstream Codex documentation
for the underlying configuration or protocol. Those links are references, not
statements that Better Codex has the same terminal experience or release path.

## Using Better Codex

- [Getting started](getting-started.md)
- [Installing a release or building from source](install.md)
- [Configuration](config.md)
- [Authentication](authentication.md)
- [Sandboxing and approvals](sandbox.md)
- [Skills](skills.md)
- [Project instructions with `AGENTS.md`](agents_md.md)
- [Slash commands](slash_commands.md)
- [Non-interactive execution](exec.md)
- [Execution policies](execpolicy.md)

The full-screen application also shows contextual key bindings along the
bottom of each focused view. Prefer those hints over memorizing a global key
map because available actions depend on the active view.

## Contributing

- [Contributor workflow](../CONTRIBUTING.md)
- [Repository map and maintenance boundaries](repository-guide.md)
- [Repository-specific agent instructions](../AGENTS.md)
- [Release process](../RELEASING.md)
- [App-server API reference](../codex-rs/app-server/README.md)
- [Python SDK](../sdk/python/README.md)
- [TypeScript SDK](../sdk/typescript/README.md)

The upstream Codex repository remains useful when investigating retained
backend behavior. Better Codex-specific UI, packaging, and contributor guidance
in this repository takes precedence when the two projects differ.

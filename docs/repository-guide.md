# Repository guide

Better Codex is a full-screen terminal application built on the Codex Rust
backend. The most important maintenance boundary is between reusable agent and
execution behavior and the application-specific TUI. New UI state should not
leak into backend crates, and retained backend code should not dictate the
fork's user experience.

## Directory map

| Path                              | Responsibility                                                                        |
| --------------------------------- | ------------------------------------------------------------------------------------- |
| `codex-rs/tui`                    | Full-screen dashboard, session workspace, views, input, and TUI orchestration         |
| `codex-rs/cli`                    | Command-line entry point and dispatch into the TUI, exec mode, servers, and utilities |
| `codex-rs/core`                   | Retained agent runtime; avoid expanding it when a focused crate can own new behavior  |
| `codex-rs/app-server*`            | Public app-server protocol, transports, clients, daemon, and server implementation    |
| `codex-rs/exec-server*`           | Connected execution-server protocol and implementation                                |
| `codex-rs/protocol`               | Shared conversation and session protocol types                                        |
| `sdk`                             | Retained Python and TypeScript integration surfaces                                   |
| `scripts`                         | Local formatting, packaging, installation, and development helpers                    |
| `.github`                         | Better Codex issue forms, release automation, and CI support                          |
| `bazel`, `patches`, `third_party` | Cross-platform builds and vendored dependency support                                 |

Most Rust crates have narrower ownership than this table can show. Read the
nearest module and crate documentation before choosing a destination. Prefer a
small public API and private implementation modules over adding convenience
code to a central crate.

## Naming boundaries

Published archives install a `better-codex` launcher. The Cargo workspace and
several internal binaries and crate names still use `codex` for compatibility
with the retained backend. Do not mechanically rename internal identifiers or
document `codex` as the user-facing executable unless a command is explicitly
for development.

The Python and TypeScript packages retain their published OpenAI package names.
They are compatibility surfaces, not the primary Better Codex distribution.

## Choosing where a change belongs

- Put presentation, focus, key handling, and view state in `codex-rs/tui`.
- Put reusable behavior in the narrowest existing backend crate that owns the
  concept. Introduce a focused crate before defaulting to `codex-rs/core`.
- Make new app-server API changes in v2 and update its public documentation and
  generated fixtures in the same change.
- Keep connected app-server and exec-server behavior portable across macOS and
  Linux unless the feature is explicitly platform-specific.
- Keep release, installation, and top-level documentation centered on Better
  Codex. Link upstream only for retained backend behavior that is not usefully
  duplicated here.

## Generated files

Do not hand-edit generated schemas or SDK protocol types. Use the owning
generator after changing its source:

| Source change                    | Regeneration command                                                            |
| -------------------------------- | ------------------------------------------------------------------------------- |
| Core configuration types         | `just write-config-schema`                                                      |
| App-server protocol              | `just write-app-server-schema`                                                  |
| Experimental app-server protocol | `just write-app-server-schema --experimental`                                   |
| Hook schema                      | `just write-hooks-schema`                                                       |
| Python SDK protocol types        | `cd sdk/python && uv run python scripts/update_sdk_artifacts.py generate-types` |
| Rust dependencies                | `just bazel-lock-update`                                                        |

The Python SDK generator reads schemas from its pinned runtime package. Update
that pin deliberately before regenerating types for a new runtime protocol.

## Validation

Run validation from the repository root unless a command says otherwise:

```sh
just fmt
just test -p <crate-you-changed>
just fix -p <crate-you-changed>
```

User-visible TUI changes require `insta` snapshot coverage. Review generated
snapshots rather than accepting them blindly. Shared `core`, `common`, or
`protocol` changes may require the complete test suite; see `AGENTS.md` for the
approval and sequencing rules.

The repository uses both Cargo and Bazel. When adding compile-time file access
such as `include_str!`, update the crate's Bazel data declarations as well as
its Cargo inputs.

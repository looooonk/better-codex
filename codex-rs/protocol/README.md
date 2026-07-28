# codex-protocol

This crate defines the protocol types used by the Codex backend, including both
internal types for communication between `codex-core` and `codex-tui`, and
external types used with `better-codex app-server`.

This crate should have minimal dependencies.

Ideally, we should avoid "material business logic" in this crate, as we can always introduce `Ext`-style traits to add functionality to types in other crates.

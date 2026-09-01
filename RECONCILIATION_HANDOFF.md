# Upstream reconciliation completion record

This file preserves the outcome of the reconciliation that was originally split
across handoff branches. It is a completion record, not a set of instructions for
resuming those branches. After final cleanup, `main` is the only authoritative
development branch.

## Frozen comparison

- Frozen upstream target: `86b1123ff6b5d089a146be4e603a324cf454223a`
- Previous semantic target: `678157acaa819d5510adfe359abb5d0392cfe461`
- Better Codex baseline: `603b1e9080ef42eb80f55e2d011a9f2010a79d60`
- Frozen comparison size: 916 upstream commits

The target is a semantic checkpoint, not a request to turn Better Codex back into
the upstream CLI. Upstream release, CI, publishing, legacy TUI, desktop, web, IDE,
telemetry-only, and unrelated platform work remain excluded.

## Consolidation outcome

The formerly separate implementation streams are integrated into one history:

- response IDs, durable metadata, indexed response copying, redaction, trust, and
  managed-auth enforcement;
- live command and apply-patch approval routing, bounded Guardian evidence, and a
  disabled-by-default Guardian V2 authority boundary;
- negotiated MCP 2026 support, bounded discovery and transport, OAuth hardening,
  stable runtime bindings, elicitation handling, and recovery behavior;
- portable plugin manifests, secure bundle handling, bounded search, and unified
  skill ownership in the skills extension;
- persisted history, sections, usage, rollout lineage, revert recovery, migration
  compatibility, and durable queue lifecycle state;
- Better TUI queue management through the app-shell/backend boundary; and
- bounded Code Mode protocol, host, gRPC transport, session lifecycle, and
  app-server routing.

The independent response-copy feature branch and all five upstream-checkpoint
branches are represented in the consolidated ancestry. Their old branch
names are no longer implementation dependencies and are removed only after the
candidate is validated, pushed to `main`, and rechecked against their recorded
remote tips.

## Preserved Better Codex boundaries

- The upstream Codex backend remains infrastructure and reference material. The
  Codex agent harness was not modified as part of this reconciliation.
- Better's full-screen app shell remains the product UI. Upstream chat-widget,
  composer, onboarding, picker, and status-card architecture was not restored.
- Backend concerns stay behind app-server, extension, state, thread-store, and
  transport boundaries instead of leaking into the app shell.
- Existing bounded context, redaction, fail-closed approval, managed-network,
  foreign-path, and failure-atomic persistence invariants remain authoritative.
- MCP 2026 behavior is negotiated; legacy MCP behavior remains compatible.
- Guardian V2 remains gated and non-authoritative wherever the complete shared
  fail-closed decision path is unavailable.
- Code Mode transports are additive and bounded. Non-loopback access requires the
  configured authentication and transport protections.

## Durable state decisions

The consolidated persistence path keeps rollout IDs distinct from thread IDs,
uses immutable replacement rollouts, materializes lineage transactionally, and
preserves subscriptions across reverts. Queue admission and recovery use explicit
durable states and compare-and-swap ownership rather than composer-local state.
Mixed-version migration guards prevent old writers from silently corrupting queue
claims or history projection checkpoints.

The queue remains bounded to 100 items and 1 MiB of input. Recovery preserves
ordering for ordinary input, abort, and continuation items, including shutdown,
revert, and partially materialized history cases.

## Validation posture

Acceptance uses focused crate and integration suites because this consolidation
touches many independently owned surfaces. The recorded focused coverage includes:

- managed-auth config, login, CLI, TUI, realtime, proxy, and app-server behavior;
- protocol, rollout, thread-store, state migration, history projection, revert,
  request serialization, and durable queue recovery;
- MCP legacy/modern negotiation, discovery bounds, transport recovery, OAuth,
  runtime routing, plugin APIs, archive handling, and skill ownership;
- Guardian and approval handoff races, redaction, and fail-closed behavior;
- Better TUI queue behavior and reviewed snapshots; and
- Code Mode protocol, runtime, host, transport, termination, backpressure, and
  app-server/CLI routing.

Final compatibility review also covered legacy composite-rollout recovery,
failure-atomic revert publication, exact retained World State identity and stale
revocation, collision-free skills catalog identity, durable queue admission, and
exactly-once nested command completion.

The recorded final gates include:

- 1,478 `codex-tui` tests with reviewed and accepted snapshots;
- 80 `codex-code-mode-host` tests and the selected Code Mode, Guardian, skills,
  state, protocol, rollout, and thread-store suites;
- the final combined `codex-core`, extension API, skills extension, rollout, and
  thread-store package gate;
- 1,354 app-server, app-server-protocol, and app-server-client tests; and
- focused retained-context, skills identity, legacy rollout, revert, queue, and
  nested-approval regressions.

The app-server gate had one transient zsh subprocess initialization timeout under
parallel load; its configured retry passed, and the exact case also passed alone.
There were no final test failures. The generated-artifact pass regenerated the
config schema and both stable and experimental app-server schemas and verified the
Bazel lock. No Cargo dependency change remained to regenerate. Scoped checks are
followed by the workspace fix pass and repository formatter.

The complete workspace `just test` was not run because `AGENTS.md` requires
explicit user approval and that approval was not given. This limitation is
intentional and must remain visible; it is not evidence that the full workspace
suite passed.

## Recovery bundle

The complete pre-consolidation ref snapshot is stored outside the repository:

- Path: `/Users/loooonk/Projects/better-codex-pre-consolidation-2026-09-01.bundle`
- SHA-256: `2d8f245449adc6d7782de048a7293c1510b0e7f5087f367d485d14b7e19e9240`
- Verified contents: all nine pre-consolidation refs

The bundle is the recovery path after branch deletion. It is not removed or
rewritten during repository cleanup.

## Final repository operation

After the focused gates, generated artifacts, format checks, and ancestry checks
pass, the consolidation candidate fast-forwards `main` and is pushed to GitHub.
The recorded remote tips are re-read immediately before deletion so concurrent
updates cannot be discarded. The final operational step is deleting the six
non-`main` GitHub branches, removing the clean auxiliary worktrees, and deleting
the exact local non-`main` branch names.

Completion means all of the following are true:

- local `main`, `origin/main`, and GitHub `main` resolve to the same commit;
- every former local and remote branch tip is an ancestor of `main`;
- only `main` remains locally and on GitHub;
- only the primary worktree remains and it is clean; and
- the recovery bundle still verifies with the checksum above.

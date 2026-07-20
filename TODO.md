# Harness parity TODO

This checklist turns the harness divergence audit dated 2026-07-20 into explicit
implementation decisions. The audit compares Better Codex at `b393335bb` with
upstream Codex at `678157aca`; revalidate every unchecked item against the current
trees before changing code.

The target is upstream-compatible harness behavior, with deliberate exceptions for
Better-only correctness, safety, resource bounds, and development-quality
improvements that do not redefine Codex behavior.

## Legend

- **P0**: direct safety/security issue or fundamental model-input/inference difference.
- **P1**: material behavioral divergence.
- **P2**: conditional, reliability, observability, or lower-frequency divergence.
- **CHANGE** (`[ ]`): Better Codex should be changed toward upstream behavior.
- **KEEP** (`[x]`): the divergence was reviewed and should remain intentional.

## Safety and inference parity

- [x] **P0 · CHANGE — Enforce MCP model visibility at dispatch.** Reject calls when
  current tool metadata is missing or the tool is hidden from the model; do not
  dispatch stale or forged calls.
- [x] **P0 · CHANGE — Forbid dangerous commands under `Never`.** Match upstream by
  rejecting dangerous unmatched commands even when the permission profile is
  `Disabled` or `External`.
- [x] **P0 · CHANGE — Port upstream forced-`rm` detection.** Recognize long, split,
  and clustered force flags, executable paths, wrappers, and nested shell syntax.
- [x] **P0 · CHANGE — Restore the bundled GPT-5.6 window to 272,000 tokens.** The
  current 372,000-token fallback delays compaction by about 90,000 tokens and is
  not justified as a TUI-specific improvement.
- [x] **P0 · CHANGE — Sync the GPT-5.6 base instructions.** Include upstream's
  destructive-action safeguards and `HOME`/`CODEX_HOME` variable rules.
- [x] **P0 · CHANGE — Reconcile Guardian with upstream.** Port the current policy
  source, trust model, catalog integration, tool-mode behavior, and permission
  instructions so identical actions receive equivalent review outcomes.

## Models, prompts, and context

- [x] **P1 · CHANGE — Sync generic skill-usage flags.** Enable the upstream generic
  skill workflow for GPT-5.4, GPT-5.4-mini, GPT-5.2, and Codex Auto Review.
- [x] **P2 · CHANGE — Sync model capability metadata.** Add current reasoning-summary
  and catalog-authored review/permission fields so live and fallback catalogs behave
  consistently.
- [x] **P2 · CHANGE — Sync memory-maintenance model preferences.** Use GPT-5.6 Luna
  for extraction and GPT-5.6 Terra for consolidation.
- [x] **P2 · CHANGE — Narrow recommended-plugin prompting.** Do not proactively
  interrupt loosely related tasks with install suggestions; follow upstream's
  discovery and exact-match guidance.
- [x] **P1 · CHANGE — Sync image-generation instructions and completion behavior.**
  Remove the Better-only silence rule and align code-mode wait/return guidance with
  the current upstream tool contract.
- [x] **P1 · CHANGE — Match upstream default collaboration-mode instructions.**
  Supply the same Default-mode developer fragment as Codex instead of leaving it
  null, including upstream's incremental update and clearing behavior.
- [x] **P1 · CHANGE — Match upstream additional-context handling.** Remove Better's
  item-count and aggregate-token admission limits, which can silently discard
  entries. Accept all supplied entries as Codex does, while retaining upstream's
  per-value truncation behavior.
- [x] **P1 · KEEP — Bound explicit skill prompts.** Retain per-skill and per-turn
  limits, name/path validation, and visible truncation warnings.
- [x] **P1 · CHANGE — Match upstream inter-agent context handling.** Remove Better's
  outbound, per-item, aggregate, and child-completion admission limits. Record every
  delivered communication in model history as Codex does instead of omitting it.
- [x] **P1 · KEEP — Bound developer instructions and MCP thread hints.** These caps
  prevent configuration or server metadata from consuming unbounded model context.
- [x] **P1 · KEEP — Bound combined `AGENTS.md` instructions.** Retain the shared byte
  and token budgets across global and project instructions.
- [x] **P1 · KEEP — Bound environment and world-state rendering.** Retain caps on
  roots, rules, domains, environments, subagent lines, and escaped field values.
- [x] **P1 · CHANGE — Match upstream extension-context handling.** Remove Better's
  fragment-count and aggregate-token admission limits, which can silently discard
  extension-provided context and world-state fragments. Accept every fragment as
  Codex does.
- [x] **P1 · KEEP — Structure and bound hook context.** Retain typed fragments,
  role/slot validation, per-item caps, aggregate caps, warnings, and omission
  markers.
- [x] **P1 · KEEP — Bound hook execution resources.** Retain stdout/stderr limits and
  the hook fan-out ceiling.
- [x] **P1 · CHANGE — Match upstream `tool_search` handling.** Remove Better's local
  result-count and serialized-size caps, including bounded reserialization during
  history reconstruction. Return every coalesced result selected by the requested
  search limit as Codex does.
- [x] **P2 · KEEP — Keep narrow interactive-input and task-name bounds.** The 1–3
  question, 2–3 option, and 64-character V2 task-name limits keep model-visible
  interaction bounded and predictable.
- [x] **P1 · KEEP — Keep the 64 MiB app-server file-read cap.** Continue checking both
  initial metadata and streamed growth.
- [x] **P1 · CHANGE — Port upstream exec-server robustness bounds.** Add JSON tree,
  retained-output-chunk, directory-entry, and capability/skill discovery-walk limits.

## Context lifecycle and compaction

- [x] **P0 · KEEP — Preserve current-turn-aware pre-turn compaction.** Continue
  capturing the incoming turn and refreshed context before budget evaluation, then
  restoring those exact items after the summary.
- [x] **P1 · KEEP — Preserve explicit-user provenance IDs.** Do not return to
  content-shaped heuristics that can misclassify genuine user messages after
  compaction.
- [x] **P1 · KEEP — Refresh `AGENTS.md` on every ordinary turn.** Same-path edits
  should become model-visible without requiring an environment change or restart.
- [x] **P1 · KEEP — Preserve complete world-state diffs and tombstones.** Retain
  subagent-only change detection and cleared-state markers for collaboration and
  developer instructions.
- [ ] **P1 · CHANGE — Port current upstream world-state fragments.** Add full rendered
  permission hashing, bounded host-skill snapshots, deferred-executor guidance,
  current collaboration revocation wording, and broad per-step refresh.
- [ ] **P1 · CHANGE — Port buffered auto-compaction fallback.** Support the configured
  reserve, one-time fallback prompt, and buffered/full-window boundary behavior.
- [ ] **P2 · CHANGE — Broaden previous-model compaction fallback.** Retry/fallback for
  the same transient, status, context, usage, and server failures as upstream.
- [ ] **P2 · CHANGE — Align prompt-cache identity.** Default to the Responses metadata
  session ID rather than the thread ID so root and child cache reuse matches upstream.
- [ ] **P2 · CHANGE — Record cache-write token usage.** Keep cache accounting in sync
  with upstream in addition to existing cache-read accounting.
- [ ] **P1 · CHANGE — Align `get_context_remaining`.** Report base-window tokens
  remaining rather than tokens until compaction, matching the upstream tool contract.

## Multi-agent behavior

- [ ] **P1 · CHANGE — Restore the default `collaboration` namespace.** Model-visible
  tools should use `functions.collaboration.*`; do not reserve the upstream default
  as an invalid custom namespace.
- [ ] **P1 · CHANGE — Sync multi-agent configuration vocabulary and defaults.** Port
  per-session concurrency, default child model, default reasoning effort, and current
  backend-aware enablement behavior; retire superseded fork-only settings where safe.
- [ ] **P1 · CHANGE — Align spawn schema exposure.** Advertise validated model and
  reasoning overrides by default and expose roles only when configured.
- [ ] **P1 · CHANGE — Align child model and role application.** Preserve explicit
  model/reasoning choices through role loading, validate final combinations, and
  allow upstream-supported overrides on full-history forks.
- [ ] **P1 · CHANGE — Remove recent task messages from `list_agents`.** Match the
  smaller upstream identity/status result and avoid unnecessarily re-exposing child
  prompts.
- [x] **P1 · KEEP — Retain the 64-character task-name limit.** This is a sensible
  model-visible context bound even though upstream currently accepts longer names.
- [ ] **P1 · CHANGE — Match upstream inter-agent payload handling.** Remove Better's
  message and completion-prefix caps so send, follow-up, and child-completion payloads
  retain the same content Codex would deliver.
- [ ] **P2 · CHANGE — Port audio and local-audio agent messages.** Support upstream's
  current media forms throughout subagent communication.
- [ ] **P1 · CHANGE — Restore cold-resume child identity and roles.** Rehydrate V2
  descendants, persisted roles, routing identity, and paginated child history.
- [ ] **P1 · CHANGE — Align the final-answer mailbox boundary.** Leave non-triggering
  child mail for the next turn instead of forcing another sample after a terminal
  answer.
- [ ] **P2 · CHANGE — Sync remaining upstream agent dispatch behavior.** Port
  model-backed availability rules, proactive-delegation wording, review-agent turns,
  extension execution, and current job/CSV role defaults.

## Tools and MCP runtime

- [ ] **P1 · CHANGE — Port the current MCP runtime lifecycle.** Adopt centralized
  runtime management, complete startup timeout coverage, pre-planning construction,
  serialized stdio writes, reusable catalogs with opt-out, executor capability-root
  discovery, and upstream metadata visibility rules.
- [ ] **P1 · CHANGE — Preserve encrypted MCP content precedence.** Prefer
  `_meta["codex/encryptedContent"]` over structured content for replay/privacy-safe
  model and rollout representation.
- [x] **P1 · KEEP — Keep Better's `request_user_input` limits and deadline semantics.**
  The bounded schema and use of the requested 60–240 second duration are clearer and
  more faithful than treating the field as a fixed-duration switch.
- [ ] **P2 · CHANGE — Align plugin-install execution mechanics.** Mark installation
  non-parallel and follow upstream list/search/install sequencing.
- [ ] **P2 · CHANGE — Make `write_stdin` parallel-capable per session.** Use
  process-level locking so writes to independent terminals do not serialize globally.
- [ ] **P2 · CHANGE — Port structured web-search results.** Preserve current upstream
  standalone result structure and metadata across supported tool surfaces.

## Execution, sandboxing, approvals, and hooks

- [ ] **P1 · CHANGE — Materialize sandbox roots per execution environment.** Resolve
  selected-environment workspace roots and permissions for every tool attempt rather
  than using turn-wide host roots.
- [ ] **P1 · CHANGE — Align network attribution and cancellation.** Attribute every
  sandbox execution and cancel the owning call when managed-network policy denies it.
- [x] **P1 · KEEP — Fail closed without a managed-network sandbox.** Do not remove
  Better's guard when enforcement is requested but no sandbox boundary exists.
- [ ] **P1 · CHANGE — Launch managed-network proxies on remote executors.** Port the
  upstream executor-side proxy path while retaining the no-sandbox fail-closed guard.
- [x] **P2 · KEEP — Preserve sandboxed custom `argv[0]`.** Better's macOS and Linux
  wrapper handling fixes a known upstream correctness gap.
- [x] **P1 · KEEP — Preserve strict CLI bypass conflict validation.** Continue
  rejecting contradictory root/subcommand approval and sandbox combinations instead
  of silently letting bypass win.
- [x] **P1 · KEEP — Preserve hook output, context, and fan-out bounds.** Also retain
  cleanup of hook spill files during shutdown.
- [ ] **P1 · CHANGE — Add root `SessionEnd` hook semantics.** Flush the rollout and
  invoke the root-only lifecycle hook during teardown as upstream does.
- [x] **P2 · KEEP — Preserve concurrent exec-server process reads.** Long reads should
  not block writes, termination, or unrelated RPCs.

## Sessions, app-server, and persistence

- [ ] **P2 · CHANGE — Port upstream raw-response completion events.** Preserve the
  response ID and token usage from every completed model response and expose the
  upstream `RawResponseCompleted` event so malformed generations can be traced.
- [ ] **P1 · CHANGE — Port paginated rollout and history infrastructure.** Add bounded
  context-suffix loading, SQLite history materialization, paginated thread/child
  reads and resumes, and explicit unsupported-operation checks.
- [ ] **P1 · CHANGE — Port retry/edit/fork-before-turn semantics.** Support
  `beforeTurnId`, canonical truncation, and `deferGoalContinuation` through the v2
  app-server API.
- [x] **P1 · KEEP — Preserve bounded remote event queues and request timeouts.** Keep
  connect/initialize/request deadlines, lossless critical events, lag reporting, and
  explicit pressure errors.
- [x] **P1 · KEEP — Preserve lossless in-process server requests.** Backpressure
  approvals, MCP elicitations, and interactive input rather than rejecting them when
  a client queue is temporarily full.
- [x] **P2 · KEEP — Preserve strict foreign-path handling.** Reject unrepresentable
  selected-environment working directories and retain foreign remote read paths
  instead of silently falling back to the host working directory.
- [x] **P2 · KEEP — Preserve failure-atomic persistence.** Retain symlink-resolved
  config locking, compare-and-swap writes, staged deletion recovery, and careful
  archive/metadata commit ordering.
- [x] **P2 · KEEP — Preserve exact custom interactive thread filtering.** The fork's
  `ThreadSourceKind::Custom` behavior supports its standalone TUI and does not alter
  backend reasoning.
- [ ] **P2 · CHANGE — Port resume-directory persistence.** Remember and apply the
  user's current resume-directory choice while keeping strict foreign-path checks.

## Conditional upstream capabilities

- [ ] **P2 · CHANGE — Reconcile remaining upstream runtime surfaces as they are
  exposed.** Port current audio plumbing, Bedrock/provider transport, workspace spend
  control, realtime delegation/transcript tails, and other backend capabilities that
  remain supported by Better Codex; omit only surfaces deliberately removed from the
  product.

## Repository-local instructions

- [x] **P2 · KEEP — Preserve Better Codex's root `AGENTS.md` direction.** Its TUI-first
  architecture, harness-preservation rule, and Linux/macOS support policy are
  intentional development guidance for this fork, not an installed harness change.

# Upstream reconciliation handoff

This file records the graceful-stop state for the selective reconciliation in
`TASK.md`. None of the WIP branches below is ready to merge. Resume from the
frozen upstream target and preserve the Better Codex app-shell boundaries in
`AGENTS.md` and `UPSTREAM_RECONCILIATION.md`.

## Frozen comparison

- Frozen upstream target: `86b1123ff6b5d089a146be4e603a324cf454223a`
- Previous semantic target: `678157acaa819d5510adfe359abb5d0392cfe461`
- Root reconciliation branch: `codex/reconcile-upstream-86b1123`
- Root handoff HEAD before this file: `8b7f25b1fb73589524f81fa2c45813ec1493b946`
- Last previously pushed root SHA: `d168c6ae40a8835680494beff7754c6430a1a169`

The root branch is intentionally security-incomplete and must not be merged.
It is ten implementation commits ahead of the previously pushed SHA. Those
commits cover managed-auth bootstrap/backend enforcement and rollout, replay,
trace, feedback, and diagnostic redaction.

## Completed foundations

The validation ledger in `UPSTREAM_RECONCILIATION.md` is authoritative. The
following Priority 1 foundations are implemented and had their listed scoped
validation run before the graceful stop:

- bearer and command projection redaction;
- persisted rollout and legacy replay redaction;
- app-server raw projection, rollout trace, and feedback-copy redaction;
- effective project trust;
- fail-closed managed network amendments;
- nested safety-buffering metadata;
- bounded review rejection reasons;
- managed-auth config, bootstrap, app-server, MCP-server, and cloud-task gates;
- disabled-by-default Guardian V2 feature/crate scaffold.

Stage 0 is complete. Priorities 2 through 5 have a frozen semantic inventory but
have not begun implementation.

## Recoverable WIP refs

### Managed-auth CLI and Better TUI

- Branch: `codex/handoff-managed-auth-cli-ui`
- HEAD: `b631834bf5e7b83dea40fefe6d4038b52040c1b8`
- State: clean, WIP, not formatted/fixed after the latest changes
- Ordered commits to transplant onto the root branch:
  1. `ba1a5b885` — CLI login-policy gates
  2. `5fb92fbe8` — Better TUI login surfaces
  3. `c4149d931` — policy-aware relogin cleanup
  4. `8acc7cee0` — plugin command auth policy
  5. `b631834bf` — WIP stored-only cleanup isolation and child-process tests

Do not transplant the worktree's bootstrap prerequisite; the root branch has
the equivalent bootstrap and later backend-service work already.

Validation completed on this branch:

- `just test -p codex-cli`: 327/327 passed before the latest WIP commit.
- Focused deterministic plugin policy test: passed.
- Focused stored-only configured-file-versus-ephemeral regression: passed.

Remaining work:

- Compile and run the new CLI child-process relogin/logout tests.
- Ensure stored-only selection uses exactly the configured backend. For a File
  or Keyring configuration it must not select process-local Ephemeral auth.
- Disallowed stored auth must be deleted without revocation; allowed stored
  ChatGPT auth may be revoked and then deleted; ambient access/API tokens must
  never be hydrated or revoked by cleanup.
- For a remote app-server target, use `CloudConfigBundleLoader::default()`
  locally at startup and in session archive commands. Do not clear local policy
  and then hydrate local credentials for a local cloud-bundle request.
- Add a remote-target regression with API-only local policy, stored ChatGPT
  auth, empty cache, and zero cloud-config requests.
- Run `just test -p codex-tui`, inspect every `*.snap.new`, and accept only the
  intended login-policy snapshots. No snapshots have been generated yet.
- Run scoped fixes only after tests, then final `just fmt`; do not retest after
  the final fix/format pass.

### Shared command/apply-patch decision pipeline

- Branch: `codex/handoff-session-decision-pipeline`
- HEAD: `e82b7345cdcdb09a9a7d5ce05e85472387d441a0`
- State: clean, WIP, unformatted, uncompiled, untested
- Diff size: 798 changed lines; formatter churn may cross the 800-line stage
  limit, so trim before finalizing.

Implemented in the WIP:

- one private hook -> live reviewer -> Guardian/user pipeline for shell,
  unified exec, and apply-patch;
- current-turn approval policy and reviewer lookup;
- environment-scoped heterogeneous approval cache keys;
- source-specific timeout behavior and `Abort` -> `TurnAborted`;
- Guardian rationale retrieval/removal;
- live reviewer/policy re-read before a sandbox retry;
- an auto-environment hot-swap regression for policy change between sandboxed
  execution and unsandboxed retry.

Remaining work:

- Trim roughly 15-20 lines before formatting.
- Compile/fix the new hot-swap test and run focused timeout, abort, cache,
  rationale, and retry tests.
- Audit a policy change to `Never` while hooks/review are pending and strict
  mode becoming active for an initially skipped action; both must fail closed.
- Run `just test -p codex-core`, then `just fix -p codex-core`, then final
  `just fmt` without retesting afterward.
- Defer cancellation-token and Code Mode source binding to the Guardian/action
  correlation stage. MCP, network, and execve are intentionally not in this WIP.

The complete workspace `just test` is required eventually because core and
protocol changed, but `AGENTS.md` requires asking the user before running it.

### Inspection-only refs

These branches point at the root handoff base and contain no unique edits:

- `codex/handoff-managed-auth-core-hardening`
- `codex/handoff-managed-auth-auxiliary-paths`

They exist only to preserve the inspected task split.

## Managed-auth security gate

Do not mark managed auth complete or merge the root branch until all of these
credential-use paths are closed and have zero-request/persistence coverage:

1. Explicit `better-codex logout` must use the configured stored-only source;
   it must not load an ambient token or revoke disallowed credentials.
2. An opaque Personal Access Token cannot prove a workspace locally. Whenever
   an effective ChatGPT workspace allowlist exists, reject PAT login/loading
   before `/whoami` for direct login, ambient access token, Ephemeral storage,
   and File/Keyring storage. Agent Identity JWTs may use locally validated
   claims.
3. ChatGPT OAuth refresh must validate the prospective refreshed identity
   against the current effective policy before saving or caching it. A denied
   refresh leaves the complete stored `AuthDotJson` unchanged.
4. Realtime WebSocket startup must check that API login is permitted before
   reading provider keys, experimental bearer tokens, cached API-key auth, or
   the `OPENAI_API_KEY` fallback.
5. The hidden `responses-api-proxy` command must reject stdin API-key use under
   ChatGPT-only policy before reading stdin, binding, or contacting upstream.
6. A remote TUI/app-server target must not start a local cloud-config loader
   with restrictions stripped. The remote server owns remote credentials; the
   local process must not use otherwise disallowed local credentials.

The app-server external ChatGPT `chatgptAccountId` is a client-supplied selected
workspace by protocol design; access tokens may span workspaces and requests
carry the selected allowed account header. The audit found no concrete embedded
JWT-claim bypass there. Add a selected-workspace behavior test if that area is
touched, but do not reject it based only on a different embedded claim.

Relevant source locations inspected at handoff:

- PAT paths and refresh ordering: `codex-rs/login/src/auth/manager.rs`
- realtime credential selection: `codex-rs/core/src/realtime_conversation.rs`
- proxy dispatch/input: `codex-rs/cli/src/main.rs` and
  `codex-rs/responses-api-proxy/src/lib.rs`
- remote local-cloud ownership: `codex-rs/tui/src/lib.rs`,
  `codex-rs/tui/src/session_archive_commands.rs`, and
  `codex-rs/cloud-config/src/service.rs`

## Guardian V2 and MCP/network decision handoff

The frozen Guardian V2 implementation is not authoritative: it samples only
after an accepted tool call and discards the result. Do not cherry-pick it as a
decision system. The safe sequence is:

1. Finish the shared command/apply-patch pipeline.
2. Add backward-compatible bounded audit metadata to terminal Guardian events;
   never persist raw evidence.
3. Add separate bounded assessment, evidence, and cancellation-safe sampler
   modules with one whole-review deadline, strict output parsing, and fail-closed
   timeout/cancellation/malformed-output handling.
4. Evolve the extension seam to accept a sanitized action binding containing
   thread, turn, action, attempt, evidence revision, deadline, and cancellation.
5. Make V2 authoritative only after MCP and network use the same pipeline.

Important Guardian bounds: 32 KiB total text request, below 8,000 approximate
tokens, at most 40 entries, 4 KiB output/rationale, no raw executable payload,
and no auto-allow after truncating an action over 1,000 tokens. Reviewer tools,
if later enabled, must intersect parent permissions with read-only and fail
closed when enforcement is external or unavailable.

For MCP/network routing, use frozen references #38081, #38108, #38299, and
#38492 semantically rather than wholesale. First add a stable prepared MCP-call
boundary in `codex-mcp`, then shared MCP routing with strict Guardian semantics.
Harden network pending ownership with turn/execution/generation identity before
moving it into the shared pipeline. Cross-action policy amendments must fail
closed, and live `Never` must remain a hard deny.

## Resume on another machine

After cloning the repository and configuring the same `origin`:

```sh
git fetch origin --prune
git switch --track origin/codex/reconcile-upstream-86b1123
git status --short --branch
sed -n '1,260p' RECONCILIATION_HANDOFF.md
```

To continue a WIP in a separate worktree:

```sh
git branch --track codex/handoff-managed-auth-cli-ui \
  origin/codex/handoff-managed-auth-cli-ui
git worktree add ../better-codex-auth-ui codex/handoff-managed-auth-cli-ui

git branch --track codex/handoff-session-decision-pipeline \
  origin/codex/handoff-session-decision-pipeline
git worktree add ../better-codex-decision-pipeline \
  codex/handoff-session-decision-pipeline
```

Start with managed auth. Complete and validate its WIP branch, transplant only
the ordered commits onto the root reconciliation branch, then implement the
PAT/refresh and realtime/proxy fixes as separate reviewable commits. Update row
`1J` in `UPSTREAM_RECONCILIATION.md`, commit, and push before resuming the shared
decision pipeline.

Before a Rust build, check repository size and active processes:

```sh
du -sk . codex-rs/target/debug/incremental 2>/dev/null
ps ax -o pid=,command= | rg '(cargo|rustc|just)( |$)' || true
```

Keep the repository below 75 GB. If no Rust build is active and incremental
artifacts need pruning, delete only the contents of
`codex-rs/target/debug/incremental`; never prune it while Cargo or rustc runs.

At graceful stop there were no running Cargo, rustc, or Just processes, and the
incremental directory had been emptied.

# Better Codex bug audit

This file records every confirmed finding from a read-only audit of commit
`62590d7427` (`main`). Priorities mean:

- **P0**: critical security/data-loss issue or a direct violation of the model-context hard-cap rules.
- **P1**: high-impact correctness, reliability, performance, or user-interaction defect.
- **P2**: medium-impact correctness, compatibility, usability, or maintainability defect.
- **P3**: low-impact polish or discoverability defect.

The audit confirmed **74 findings: 14 P0, 27 P1, 32 P2, and 1 P3**.

## Bug status

Each finding has a **Status** field with one of these values:

- **Assigned**: the bug has been identified and is awaiting or undergoing work.
- **Fixed**: the bug has been addressed.
- **Fixed + Reviewed**: a second pass has confirmed that the bug was addressed and properly fixed.

Validation performed:

- `just test -p codex-tui`: 1,090 tests passed.
- `just test -p codex-app-server-protocol`: 262 tests passed.
- Focused `codex-core` AGENTS/context-compaction regressions were inspected; current tests explicitly encode two defective behaviors noted below.
- The ignored local-compaction error regression was run and failed with the raw stream error instead of the expected contextual error.
- Composer layout and Ratatui buffer behavior were inspected together to verify how literal tabs are retained but hidden during rendering.
- Source audits covered the model-visible context pipeline, backend/API integrity, supported remote
  Linux/macOS behavior, implemented Windows cross-host paths, TUI interaction and rendering, testing
  gaps, breaking surfaces, and change size.

The repository remained below the requested 50 GB limit and was about 28 GB after validation.

## Confirmed findings

### 1. [P0][Data loss / TUI] One unmodified key recursively deletes a session subtree

**Status:** Fixed

`codex-rs/tui/src/app_shell.rs:1523-1525` immediately dispatches deletion when `d` is pressed in the
focused Sessions list, and `delete_selected_session` at `app_shell.rs:1792-1808` has no confirmation
or undo. The active session is guarded, but any other persisted session can be selected. The backend
then enumerates and hard-deletes the selected thread's complete spawn subtree in
`codex-rs/app-server/src/request_processors/thread_delete.rs:31-86`. The UI neither says that child
threads will also be deleted nor shows how many descendants are affected.

**Reproduction:** open Sessions, focus the list, select any non-current session that has spawned
children, and press `d` once. The parent and every descendant are permanently deleted without a
confirmation dialog.

### 2. [P0][Security / Approval UI] Consent dialogs omit security-relevant approval details

**Status:** Fixed

`codex-rs/tui/src/app_shell/approval.rs:36-98` builds command consent from only the command, reason,
and cwd. It discards `network_approval_context` (including the host), parsed command actions,
additional permissions, proposed exec/network-policy amendments, and `available_decisions` from
`codex-rs/app-server-protocol/src/protocol/v2/item.rs:1434-1491`. Generic Approve authorizes the
current command and any requested per-command permissions, but does not apply the undisclosed
persistent policy amendments. The separate permissions dialog reduces a complete requested profile
to `network` and/or `filesystem`; Approve returns the full roots, access modes, and network policy for
the turn.

**Reproduction:** request command approval for a managed-network host or with an additional
filesystem permission, or issue a permissions request for a sensitive root. The dialog shows only
the command/reason/cwd (and can show `<unknown command>` for a network presentation) or the generic
`network, filesystem` summary; pressing Enter approves security-relevant details that were not
displayed.

### 3. [P0][Model context / Injection] `additionalContext` has unbounded keys and item count

**Status:** Fixed

Only each value is truncated to roughly 1,000 tokens. The number and length of keys are unrestricted,
and keys are interpolated into XML-like markers in
`codex-rs/context-fragments/src/additional_context.rs:5-92`. The app-server accepts the map in
`codex-rs/app-server-protocol/src/protocol/v2/turn.rs:55-86` and forwards it through
`codex-rs/app-server/src/request_processors/turn_processor.rs:85-106,488-535,889-903`. A single long key can
therefore exceed the 10K-token per-item rule, many entries have no aggregate cap, and marker
characters in a key can corrupt the wrapper structure.

**Reproduction:** call `turn/start` with an `additionalContext` key containing 50,000 characters or
with thousands of distinct keys. The complete keys are injected into model-visible context; a key
such as `foo>bar` also changes the generated marker syntax.

### 4. [P0][Model context / Token usage] Deferred `tool_search` results have no hard output cap

**Status:** Fixed

The tool accepts an unrestricted `usize` limit and the schema has no maximum
(`codex-rs/core/src/tools/handlers/tool_search_spec.rs:7-22`). The handler serializes full matching
tool definitions, including parameter schemas, into one result
(`codex-rs/core/src/tools/handlers/tool_search.rs:131-179` and
`codex-rs/core/src/tools/context.rs:150-188`). History normalization does not truncate this output
(`codex-rs/core/src/context_manager/history.rs:370-412`). One result can therefore exceed 10K tokens
and remains in every subsequent inference request.

**Reproduction:** connect an MCP server exposing many large deferred tool schemas and call
`tool_search` with a large limit such as 100. The single retained tool output contains all full
schemas without a byte/token ceiling.

### 5. [P0][Model context / Token usage] Explicit skill bodies can exceed hard caps individually or in aggregate

**Status:** Fixed

Legacy skill injection reads an entire `SKILL.md` and places it into one fragment
(`codex-rs/core-skills/src/injection.rs:63-100` and
`codex-rs/core-skills/src/skill_instructions.rs:5-40`). App-server installs only executor and
orchestrator skill providers (`codex-rs/app-server/src/extensions.rs:82-95`), so selected host skills
still use this unbounded legacy body. A same-name host body is filtered only when a selected
non-host entry was injected (`codex-rs/core/src/session/turn.rs:602-675`). The extension path caps
each executor/orchestrator body at 8 KiB (`codex-rs/ext/skills/src/extension.rs:262-288`); discovery
has finite per-source limits, but selected bodies have no aggregate byte/token budget and can still
exceed 10K tokens in combination. Each near-8-KiB fragment also crosses the 1K-token threshold for
additional manual review.

**Reproduction:** through app-server or the legacy core path, invoke a host skill whose `SKILL.md`
is larger than 40 KiB. Alternatively invoke enough near-8-KiB executor/orchestrator skills in one
turn to exceed 10K tokens in aggregate. Inspect the next request; no turn-level hard cap applies.

### 6. [P0][Model context / Multi-agent] Child completions and inter-agent messages are unbounded

**Status:** Fixed

The 1,000-token cap in `codex-rs/core/src/session_prefix.rs:10-43` applies only to error text.
Successful child final answers, `send_message`, and `followup_task` bodies are stored unchanged via
`codex-rs/core/src/context/inter_agent_completion_message.rs:5-40`,
`codex-rs/core/src/session/mod.rs:1862-1905,2919-2947`,
`codex-rs/core/src/tools/handlers/multi_agents_v2/message_tool.rs:34-95`, and
`codex-rs/protocol/src/protocol.rs:737-846`. This allows a single child result or message to exceed
the 10K-token item cap and makes repeated messages grow history without a bound.

**Reproduction:** make a subagent return more than 10K tokens, or send a comparably large message to
another agent. The full body appears as a retained model-visible fragment in the parent/recipient
thread.

### 7. [P0][Model context / Injection] MCP notes can inject an unbounded developer-priority thread hint

**Status:** Fixed

The notes integration joins returned `thread_hint` text without truncation and injects it as
developer content in `codex-rs/core/src/session/mod.rs:3382-3418`. Token budgeting in
`codex-rs/core/src/context/token_budget_context.rs:10-65` does not impose a hard maximum on that
fragment. A buggy or compromised notes provider can consume the whole context window or inject
arbitrary high-priority instructions.

**Reproduction:** configure the notes MCP integration to return a very large or adversarial
`thread_hint`, then start a turn. The complete response is placed in the developer message before
inference.

### 8. [P0][Model context / AGENTS.md] Project-instruction limits are not global or reliably bounded

**Status:** Fixed

The global Codex-home AGENTS file is read in full
(`codex-rs/codex-home/src/instructions/mod.rs:24-60`). `project_doc_max_bytes` has no safe upper
ceiling and its budget is reset for each selected environment
(`codex-rs/core/src/config/mod.rs:203-206,852-853,3841` and
`codex-rs/core/src/agents_md.rs:53-143,276-404`). The resulting pieces are combined into one user
instruction item (`codex-rs/core/src/context/user_instructions.rs:3-29`). Multiple remote
environments or a large configured limit can therefore make one injected item exceed 10K tokens.

**Reproduction:** set a very large `project_doc_max_bytes`, use several environments with large
AGENTS files, or place a large global AGENTS file under Codex home. Start a thread and inspect the
initial user-instruction item; the effective total is not constrained by one hard cap.

### 9. [P0][Model context / Hooks] Hook context has no aggregate hard cap

**Status:** Fixed

Each hook-supplied additional-context or Stop continuation fragment is capped at about 2,500 tokens
(`codex-rs/hooks/src/output_spill.rs:11-87`), but discovery and execution permit an arbitrary number
of hooks. Additional-context strings are retained as separate developer messages via
`codex-rs/core/src/hook_runtime.rs:595-615`. Stop continuation fragments are capped individually and
then combined into one user message by `codex-rs/protocol/src/items.rs:549-569`, so that item can
itself exceed 10K tokens. Post-tool feedback is not affected because feedback from all matching
hooks is joined before one shared cap is applied. There is no per-event or per-turn aggregate budget
in `codex-rs/hooks/src/engine/mod.rs:169-270`.

**Reproduction:** register many matching hooks that each emit additional context or a blocking Stop
reason near the per-hook spill threshold. Trigger the event and inspect the next request; all
additional-context messages are retained, or all Stop fragments appear in one oversized item.

### 10. [P0][Model context / Environment] Environment metadata accepts unbounded paths and policy entries

**Status:** Fixed

`codex-rs/core/src/context/environment_context.rs:33-71,115-243` renders every workspace root,
managed filesystem permission entry (including paths and glob patterns), and allowed/denied domain
pattern with no byte/token ceiling. Environment IDs, cwd values, and shell names are also unbounded
in `codex-rs/core/src/context/world_state/environment.rs:181-243`; network domains are joined without
XML escaping. App-server accepts unbounded `runtimeWorkspaceRoots` in
`codex-rs/app-server-protocol/src/protocol/v2/turn.rs:98-102`. Large lists or strings can therefore
create a single oversized or structurally corrupted environment item.

**Reproduction:** start a turn with thousands of runtime workspace roots, managed permission
entries, or domain patterns, or with very long environment identifiers and paths. The complete
environment block is forwarded to the model.

### 11. [P0][Model context / Configuration] Raw developer-mode instruction fields have no hard caps

**Status:** Fixed

Thread developer instructions, collaboration instructions, multi-agent guidance, usage hints, and
realtime instructions are accepted as raw strings and rendered without a shared size bound. Entry
points include `codex-rs/app-server-protocol/src/protocol/v2/thread.rs:98-100,382-384`,
`codex-rs/core/src/session/mod.rs:3254-3259,3439-3462`, and
`codex-rs/core/src/config/mod.rs:999,2515-2530,3933`; update/render paths include
`codex-rs/core/src/context/collaboration_mode_instructions.rs`,
`codex-rs/core/src/context/multi_agent_mode_instructions.rs`, and
`codex-rs/core/src/context/realtime_start_with_instructions.rs`, plus
`codex-rs/core/src/context_manager/updates.rs:104-124` and
`codex-rs/core/src/session/multi_agents.rs:9-56`. Any one field can exceed
10K tokens, and several can be injected together.

**Reproduction:** start/update a thread with an oversized developer or collaboration instruction,
or configure oversized usage/realtime guidance. The raw string is retained as developer-priority
model context.

### 12. [P0][Security / Sandbox] Managed-network enforcement is bypassed when `sandbox` is omitted

**Status:** Fixed

The exec-server wire contract says an executor must fail closed when `enforceManagedNetwork` is true
but managed-network details are unavailable
(`codex-rs/exec-server-protocol/src/protocol.rs:140-148`). `prepare_exec_request` instead returns an
unsandboxed `SandboxType::None` request immediately whenever `params.sandbox` is `None`, before it
examines either managed-network field (`codex-rs/exec-server/src/process_sandbox.rs:29-41`). The
bypass occurs whether or not managed-network details are supplied, turning an explicit enforcement
requirement into unrestricted execution.

**Reproduction:** call exec-server `exec` with `sandbox: null`, `enforceManagedNetwork: true`, and a
network-capable command. The process launches without a sandbox instead of being rejected or forced
through managed networking.

### 13. [P0][Model context / Multi-agent] Agent task names have no length limit

**Status:** Fixed

`AgentPath` validates the character set but not length
(`codex-rs/protocol/src/agent_path.rs:125-146`), and the spawn tool schema has no maximum
(`codex-rs/core/src/tools/handlers/multi_agents_spec.rs:78-108`). A legal lowercase task name can
exceed the 10K-token item cap by itself. The spawn result is subject to generic function-output
truncation, but the original FunctionCall arguments are not
(`codex-rs/core/src/context_manager/history.rs:370-411`), and the complete path can also appear in
child AgentMessage metadata, session/world-state context, and completion envelopes.

**Reproduction:** spawn an agent with a 50,000-character lowercase task name and inspect the next
model request. The name is accepted and remains complete in the retained tool-call arguments and
other agent-path context even if the immediate tool result is truncated.

### 14. [P1][CLI safety] Root bypass and resume/fork approval flags evade the declared conflict

**Status:** Fixed

Clap conflict declarations apply only within one flattened argument scope. A root
`--dangerously-bypass-approvals-and-sandbox` can therefore be combined with a resume/fork-local
`--ask-for-approval on-request`. Parsing succeeds, after which `tui::run_main` silently forces
`DangerFullAccess` plus `Never`, discarding the apparently safer subcommand setting. See
`codex-rs/tui/src/cli.rs:135-138`, `codex-rs/cli/src/main.rs:2454-2472`,
`codex-rs/utils/cli/src/shared_options.rs:131-163`, and `codex-rs/tui/src/lib.rs:821-830`.

**Reproduction:** run
`codex --dangerously-bypass-approvals-and-sandbox resume --ask-for-approval on-request --last`.
Unlike the same-scope combination, it parses and starts with approvals disabled.

### 15. [P1][CLI safety] Dangerous bypass silently overrides an explicit sandbox selection

**Status:** Fixed

The dangerous root flag can also be combined with `--sandbox workspace-write`. The shared CLI types
do not declare these as conflicting, and both TUI and exec paths replace the explicit sandbox with
danger-full-access (`codex-rs/utils/cli/src/shared_options.rs:37-49`,
`codex-rs/tui/src/lib.rs:821-830`, and `codex-rs/exec/src/lib.rs:293-299`). A command line that visibly
requests a sandbox therefore runs unsandboxed.

**Reproduction:** run `codex --dangerously-bypass-approvals-and-sandbox --sandbox workspace-write`.
The command is accepted and the explicit workspace-write policy is discarded.

### 16. [P1][API compatibility] Config responses omit fields that generated clients require

**Status:** Fixed

`ConfigReadResponse.layers` and `ConfigLayer.disabled_reason` use
`skip_serializing_if = "Option::is_none"` in
`codex-rs/app-server-protocol/src/protocol/v2/config.rs:291-296,363-368`. Generated TypeScript declares
both properties as required `... | null`. The runtime handler also constructs `layers: None` when
layers are not requested (`codex-rs/app-server/src/config_manager_service.rs:139-155`). Valid server
responses therefore violate their own generated v2 client types.

**Reproduction:** issue `config/read` with `includeLayers: false`, or read a layer with no disabled
reason. The JSON omits the required property instead of returning `null`.

### 17. [P1][Model correctness / AGENTS.md] AGENTS instructions never refresh during a normal thread

**Status:** Fixed

The initial session loads AGENTS once (`codex-rs/core/src/session/session.rs:909-923`). Per-turn
refresh is behind the disabled deferred executor, while `AgentsMdManager` caches by environment
selection rather than file contents (`codex-rs/core/src/agents_md_manager.rs:31-44`). An existing
regression test in `core/tests/suite/model_visible_layout.rs:201-329` explicitly expects a newly
created AGENTS file to be absent from the next request. The model can keep obsolete project rules
after edits or directory changes.

**Reproduction:** start a thread, create or edit an applicable `AGENTS.md`, then submit another turn
(or switch the turn cwd). Inspect the request: the new instructions are missing and old instructions
remain effective.

### 18. [P1][Model correctness / Compaction] Automatic pre-turn compaction ignores the incoming prompt

**Status:** Fixed

The threshold estimate considers only retained history, with a source TODO acknowledging that the
new turn input is excluded (`codex-rs/core/src/session/turn.rs:153-157,798-823`). A large prompt can
push the actual request well over the window after the preflight decides no compaction is needed.
The same ordering flaw means that, when retained history does trigger pre-turn compaction, its
request omits the incoming user message and turn-input contributions that have not yet been built;
captured world-state or context-override diffs can still be present. The raw user input is added only
for the post-compaction inference. Existing tests at `core/tests/suite/compact.rs:4621-4744` and
`compact_remote.rs:3347-3588` encode this ordering.

**Reproduction:** bring a thread just below its auto-compaction threshold and submit a large prompt;
the pre-turn check does not compact for the prompt's size and the provider may reject the request.
Alternatively, start above the retained-history threshold and inspect the pre-turn compaction
request: it summarizes without the pending question.

### 19. [P2][Model correctness / World state] Newly known shell metadata is suppressed when the prior snapshot lacks it

**Status:** Fixed

`EnvironmentSnapshot::has_same_diff_value` compares optional shells with `zip(...).is_none_or(...)`
in `codex-rs/core/src/context/world_state/environment.rs:300-309`. When the prior snapshot has
`shell: None`, a newly resolved `Some(shell)` produces an empty zip and is considered unchanged. The
world-state baseline still advances, so the newly learned shell is not retried. This is most
reachable when resuming or reconstructing an environment whose persisted shell was unknown;
normally resolved environments cache shell metadata from their first snapshot. Treating a later
`None` as unchanged can intentionally preserve the last known shell and is not independently a
defect. `codex-rs/core/src/context/world_state/environment_tests.rs:196-222` explicitly expects the
missing update.

**Reproduction:** resume or reconstruct an environment with `shell: None`, then resolve its shell
without changing cwd or status. The next environment diff contains no shell update.

### 20. [P1][Model correctness / Multi-agent] Subagent-list changes do not trigger environment diffs

**Status:** Fixed

The environment snapshot stores `subagents`, but the change predicate checks only date, timezone,
network, filesystem, and per-environment values
(`codex-rs/core/src/context/world_state/environment.rs:93-144`). Spawning or removing a subagent
therefore does not update the model-visible environment unless some unrelated field also
changes.

**Reproduction:** spawn or remove a child agent while cwd and environment metadata remain stable,
then inspect the next model request. The subagent list is stale or absent.

### 21. [P1][Model correctness / Collaboration] Clearing collaboration instructions has no tombstone

**Status:** Fixed

`build_collaboration_mode_update_item` returns no item when the new mode has empty developer
instructions (`codex-rs/core/src/context_manager/updates.rs:64-83`). Because history is incremental,
the prior collaboration policy remains model-visible. Re-enabling it later adds another copy instead
of replacing the stale one.

**Reproduction:** start with a collaboration mode that has instructions, update to a mode with none,
and submit another turn. The previous collaboration instructions remain in request history.

### 22. [P2][Model context / Capabilities] Re-enabling apps or plugins duplicates generic guidance

**Status:** Fixed

`AppsInstructionsState` and `PluginsInstructionsState` render generic usage guidance only on
unavailable-to-available transitions and emit nothing for available-to-unavailable transitions
(`codex-rs/core/src/context/world_state/apps_instructions.rs:38-49` and
`plugins_instructions.rs:38-49`). The generic fragments do not assert that a particular capability is
currently installed, so retaining one after disablement is not itself stale availability state.
However, incremental history keeps that fragment, and re-enabling the capability class appends an
identical copy instead of recognizing the retained guidance.

**Reproduction:** start a thread while apps or plugins are available, disable all of that class and
submit, then re-enable one and submit again. The later request contains two retained copies of the
same generic instructions.

### 23. [P1][History integrity] Ordinary user text matching an internal wrapper is misclassified

**Status:** Fixed

Internal fragment recognition relies on marker text alone. A genuine user message such as
`<turn_aborted>ordinary note</turn_aborted>` is recognized as an internal contextual fragment.
`parse_turn_item` then omits it from reconstructed user turns, while history-boundary and compaction
logic may treat it as internal context rather than user intent. See
`codex-rs/core/src/context/contextual_user_message.rs:49-75`,
`codex-rs/context-fragments/src/fragment.rs:116-130`, and
`codex-rs/core/src/event_mapping.rs:50-87,153-165`.

**Reproduction:** submit the literal text `<turn_aborted>ordinary note</turn_aborted>`, then inspect a
reconstructed transcript or `thread/read` turn list (or compact the history). The submitted message
is absent as an ordinary user turn or handled as contextual state.

### 24. [P0][Architecture / Model context] Extension APIs bypass the required contextual-fragment boundary

**Status:** Fixed

The extension `PromptFragment` and `WorldState` APIs accept arbitrary `String` content directly
(`codex-rs/ext/extension-api/src/contributors/prompt.rs:1-49` and
`codex-rs/ext/extension-api/src/contributors/world_state.rs:29-99`). World-state contributions carry
extension-supplied roles and markers, but neither API requires a typed contextual fragment or
applies a common hard cap. Session plumbing accepts their arbitrary bodies/markers and adapts them
into the internal fragment envelope
(`codex-rs/core/src/context/world_state/mod.rs:112-153` and
`codex-rs/core/src/session/mod.rs:993-1009,3126-3174,3345-3379`). New/custom extensions can therefore
violate the repository's hard item-size and typed-fragment invariants by construction; several
unbounded findings above are symptoms of this parallel injection path.

**Reproduction:** register an extension returning a prompt/world-state string larger than 10K
tokens. The entire body reaches model history without a shared cap at the extension boundary.

### 25. [P1][Availability / Hooks] Hook discovery permits unbounded process fan-out

**Status:** Fixed

Hook discovery accepts an arbitrary number of matching commands and the dispatcher starts them all
without a concurrency ceiling (`codex-rs/hooks/src/engine/discovery.rs:63-73,441-461` and
`codex-rs/hooks/src/engine/dispatcher.rs:89-115`). A large configuration can exhaust processes, file
descriptors, memory, and CPU on a single event; each completion can also inject the output described
in finding 9.

**Reproduction:** configure hundreds or thousands of hooks for one event and trigger it. They are
launched concurrently rather than queued under a bounded worker limit.

### 26. [P1][Availability / Hooks] Hook output is buffered without a memory cap before truncation

**Status:** Fixed

The command runner uses `wait_with_output` and holds complete stdout/stderr in memory
(`codex-rs/hooks/src/engine/command_runner.rs:59-65,101-110`). Truncation/spilling occurs only afterward in
the hook engine. A noisy hook can therefore OOM the process even though the eventual model fragment
is truncated.

**Reproduction:** run a hook that continuously writes a very large stdout/stderr stream before
exiting. RSS grows with the complete output before spill logic runs.

### 27. [P2][Windows remote execution correctness] Foreign Windows cwd silently becomes the host cwd

**Status:** Fixed

When a remote turn cwd cannot be represented as a native path, `TurnContext` falls back to the local
host cwd (`codex-rs/core/src/session/turn_context.rs:691-697`). The remote command path continues to
carry its foreign `PathUri`, but host-native legacy consumers such as hooks, MCP, child configuration,
review, and permission matching can then operate in an unrelated directory. This is especially
reachable for a Windows `C:\\...` cwd handled by a Linux/macOS app-server. Windows cross-host
execution is outside this fork's currently declared Linux/macOS deployment matrix, but the
implemented compatibility path fails silently rather than rejecting the unsupported combination.

**Reproduction:** connect a non-Windows app-server to a Windows exec environment, start a turn in
`C:\\repo`, and invoke a cwd-sensitive hook or MCP operation. It runs against the app-server's host
directory instead of failing localization or using the remote cwd.

### 28. [P1][Availability / Remote transport] Remote notifications accumulate in an unbounded queue

**Status:** Fixed

`RemoteAppServerClient` creates `mpsc::unbounded_channel::<AppServerEvent>()` at
`codex-rs/app-server-client/src/remote.rs:213-215`. Every notification and server request is drained
from the WebSocket into it (`remote.rs:319-356,945-955`) without consumer backpressure. Each WebSocket
message may be as large as 128 MiB (`remote.rs:65-67,788-791`). A slow or paused TUI therefore allows
the socket task to grow RSS until OOM.

**Reproduction:** connect to a remote app-server that emits output deltas faster than the client
consumes them, or stop polling events while keeping the connection open. Memory grows with the
unbounded event backlog.

### 29. [P1][Availability / Remote transport] Ordinary JSON-RPC requests have no deadline

**Status:** Fixed

`request_json_rpc` sends a command and awaits its oneshot indefinitely
(`codex-rs/app-server-client/src/remote.rs:635-655`). Only initialization and shutdown have explicit
timeouts; a pending entry otherwise remains until a response or disconnect. A peer that stays
connected but never answers can hang any user operation forever.

**Reproduction:** connect to a remote app-server, make it accept a `thread/start`, settings, or
session request without responding, and keep the WebSocket open. The request never returns.

### 30. [P1][TUI responsiveness] Backend calls block the sole input, render, and event loop

**Status:** Fixed

The main shell loop awaits key handling and server-event handling inline
(`codex-rs/tui/src/app_shell.rs:277-387`). Submit, resume, delete, settings, and approval handlers all
await backend RPCs from this path. While one is slow or hung, the TUI cannot redraw, process a resize,
close a modal, or handle Esc/Ctrl-C. This compounds the missing remote deadlines in finding 29.

**Reproduction:** connect to a remote app-server, pause it after connection, then press Enter to
submit or invoke a Sessions/settings action. The entire UI freezes until the transport resolves.

### 31. [P1][Reliability / TUI] Recoverable action errors terminate the full-screen application

**Status:** Fixed

The same top-level loop propagates ordinary RPC/action failures with `?` at
`codex-rs/tui/src/app_shell.rs:287,312,333,387`. A rejected `turn/start`, failed approval response,
rename, delete, or settings update unwinds `run`, restores the terminal, and exits rather than showing
an in-app error and preserving the draft/modal. Transport policy and application lifecycle share one
fatal error boundary.

**Reproduction:** make app-server return a JSON-RPC error for `turn/start` or for resolving an
approval, then perform that action in the TUI. The application exits to the shell instead of showing
a recoverable error.

### 32. [P1][Security UI / Stale state] Permission-profile updates are ignored by the dashboard

**Status:** Fixed

`ThreadSettingsUpdated` carries both `sandbox_policy` and `active_permission_profile`, but the TUI
event reducer ignores them (`codex-rs/tui/src/app_shell/events.rs:170-183`). The Status dashboard
continues rendering the original `shell.permission_profile`
(`codex-rs/tui/src/app_shell/dashboard_workspace.rs:31-52`). Users can therefore be shown a safer or
more permissive sandbox than the thread currently has.

**Reproduction:** update the active thread's permission profile from another client or backend and
emit `thread/settings/updated`. Open Status; the displayed permission profile remains stale.

### 33. [P1][Feature failure / TUI] External-clock current-time reminders make turns fail

**Status:** Fixed

The TUI advertises experimental app-server capability
(`codex-rs/tui/src/lib.rs:359-370,514-535`) but rejects every `CurrentTimeRead` as unsupported
(`codex-rs/tui/src/app_shell/events.rs:478-510,664-677`). App-server requests the client clock in
`codex-rs/app-server/src/current_time.rs:85-140`, and core converts rejection to a fatal turn error in
`codex-rs/core/src/session/time_reminder.rs:71-105`.

**Reproduction:** enable `[features.current_time_reminder]` with `clock_source = "external"`, start
the TUI, and submit a turn. The `currentTime/read` request receives `-32000` and the turn fails with
`failed to read current time`.

### 34. [P1][Tool interaction / TUI] `request_user_input` auto-resolution is ignored

**Status:** Fixed

The protocol carries `auto_resolution_ms`
(`codex-rs/app-server-protocol/src/protocol/v2/item.rs:1622-1629`), and the tool promises a 60-240
second nonblocking timeout. `PendingUserInput::from_request` copies the questions but drops the timer
(`codex-rs/tui/src/app_shell/user_input.rs:29-42`); no replacement timer exists. A request meant to
continue automatically instead blocks the agent indefinitely.

**Reproduction:** trigger `request_user_input` with `autoResolutionMs: 60000` and do not answer. The
modal remains after a minute and the turn never resumes.

### 35. [P2][Tool interaction / TUI] The promised free-form Other choice and option explanations are hidden

**Status:** Fixed

The tool contract says the client adds `Other (free-form)` and requires every option to include a
tradeoff description (`codex-rs/core/src/tools/handlers/request_user_input_spec.rs:14-64`). The TUI
renders only up to three labels and never renders descriptions or the Other affordance
(`codex-rs/tui/src/app_shell/input_request_view.rs:206-249`). Arbitrary text happens to be accepted
when `is_other` is set, but users are not told that and cannot compare the supplied tradeoffs.

**Reproduction:** trigger a normal `request_user_input` question with two described choices. The
modal shows label-only choices and no Other option, although typing an undocumented free-form answer
is accepted.

### 36. [P1][Interactive requests / Protocol] A second concurrent request is auto-rejected

**Status:** Fixed

`handle_server_request` permits only one approval, elicitation, or user-input request. If another
arrives, it immediately sends JSON-RPC `-32000` rather than queuing it
(`codex-rs/tui/src/app_shell/events.rs:441-503`). Parallel tools or child-agent operations can thus be
denied without any user decision.

**Reproduction:** make two commands request approval concurrently, or overlap an approval and MCP
elicitation. Only the first modal appears; the second backend operation receives an automatic error.

### 37. [P1][Interactive requests / Lifecycle] Resolved requests leave stale modals on screen

**Status:** Fixed

On `ServerRequestResolved`, the TUI only appends a status line
(`codex-rs/tui/src/app_shell/events.rs:311-315`). It does not match the request ID against and clear
`pending_approval`, `pending_elicitation`, or `pending_user_input`. App-server legitimately emits this
notification when a turn ends or is interrupted, so a dead consent dialog can outlive its request.

**Reproduction:** open an approval dialog, interrupt or complete the turn from another client so the
server resolves it, then return to the TUI. The old modal remains; answering it attempts to resolve a
request that no longer exists and may trigger finding 31.

### 38. [P1][Security / Approval UI] Long commands hide their suffix without ellipsis or scrolling

**Status:** Fixed

The request panel is capped at 12 rows (`codex-rs/tui/src/app_shell/shell_layout.rs:141-165`).
`visible_segment_indices` preserves leading title lines and action lines but silently drops the
middle/tail (`input_request_view.rs:67-121`), and approval key handling has no scroll path. A dangerous
suffix can be completely invisible while Enter still approves the whole command.

**Reproduction:** at a 40-80 column terminal, request approval for a command whose benign prefix
wraps for more than nine rows and whose dangerous operation is at the end. The suffix is absent, with
no truncation marker, yet Approve remains active.

### 39. [P1][MCP functionality / TUI] Structured elicitation forms cannot be completed

**Status:** Fixed

Any MCP form with properties sets `can_accept = false`, and the response type always uses
`content: None` (`codex-rs/tui/src/app_shell/elicitation.rs:24-54,80-96`). OpenAI forms are also
hard-disabled. The TUI offers only Decline/Cancel, so any server requiring even one field cannot
proceed.

**Reproduction:** have an MCP server elicit a form with one required text property. The TUI displays
no field editor and no Accept action.

### 40. [P1][Remote session lifecycle / TUI] Thread archive, delete, close, and status notifications are ignored

**Status:** Fixed

The event reducer explicitly discards `ThreadStatusChanged`, `ThreadArchived`, `ThreadDeleted`,
`ThreadUnarchived`, and `ThreadClosed` (`codex-rs/tui/src/app_shell/events.rs:391-431`). Changes from a
second client leave both the active-thread view and Sessions list stale. A remotely deleted active
thread remains apparently usable until the next action fails.

**Reproduction:** keep a thread open in the TUI and delete or archive it from another app-server
client. The TUI does not close/switch the active session or refresh the list; the next submit targets
stale state.

### 41. [P1][Availability / TUI output] The TUI can retain and repeatedly rewrap roughly 156 MiB per command

**Status:** Fixed

Each `TranscriptLine` keeps the complete streamed `full_text` even after compacting its card
(`codex-rs/tui/src/app_shell.rs:483-515,3047-3071`). The full-output view invalidates its cache on
every delta, then normalizes ANSI and word-wraps the complete string synchronously
(`codex-rs/tui/src/app_shell/tool_output.rs:23-109`). Core can emit up to 10,000 8-KiB deltas before
its live-event cap (`codex-rs/core/src/exec.rs:68-80,1111-1138`), allowing roughly 156 MiB across
stdout and stderr before the upstream limit is reached, much more than the final retained exec
result. The TUI applies no smaller byte cap, so one command can consume hundreds of MiB including
string copies and make every update increasingly expensive.

**Reproduction:** run a command that produces high-volume output and open Full Tool Output while it
streams. RSS grows and input/rendering lag increases as the entire accumulated output is repeatedly
rewrapped.

### 42. [P1][Availability / TUI diffs] Session diffs grow and clone without a bound

**Status:** Fixed

`DiffStore` retains every turn, item, parsed file, and aggregate diff for the session with no cap
(`codex-rs/tui/src/app_shell/diff_view.rs:19-109`). Parsing occurs synchronously, and opening or
refreshing Session Edits clones all files again
(`codex-rs/tui/src/app_shell/diff_view_controller.rs:22-24,62-77,113-127`). Unlike the transcript's
line cap, resumed/long sessions can accumulate unbounded diff memory and produce large UI stalls.

**Reproduction:** create or resume a long session containing many multi-megabyte diffs, then open
Session Edits. RSS spikes and the event loop stalls while all parsed files are cloned.

### 43. [P2][Data integrity / Config API] `expectedVersion` is not an atomic compare-and-swap across processes

**Status:** Fixed

`apply_edits` reads and checks the version at
`codex-rs/app-server/src/config_manager_service.rs:220-236`, validates, and only much later writes at
lines 330-335. The editor independently rereads and atomically replaces the file without a lock or
CAS (`codex-rs/core/src/config/edit.rs:696-738,978-984`). One app-server serializes its own config
writes across connections, but that guard is process-local. Two app-server processes sharing a
Codex home, or one app-server and an external editor, can both pass the same version check before the
write. Overlapping edits are then last-writer-wins while both RPCs can report success. For
non-overlapping edits, the writer's reread can preserve both on disk, but response validation and the
returned version still come from the stale pre-write layer (`config_manager_service.rs:337-350`) and
can omit the intervening change.

**Reproduction:** run two app-server processes against the same Codex home and write different
values to the same key with the same current `expectedVersion`. Both writes can pass validation and
return success while only the later value remains on disk.

### 44. [P1][Availability / Exec server] A long `process/read` blocks every later request on the connection

**Status:** Assigned

The exec-server connection loop routes requests sequentially
(`codex-rs/exec-server/src/server/processor.rs:108-145`). `process/read` accepts arbitrary `u64`
`waitMs` and awaits it inline (`codex-rs/exec-server/src/local_process.rs:382-460`). A silent
long-poll therefore prevents `process/write`, `process/terminate`, and all unrelated requests from
being processed; extreme values can also overflow `Instant + Duration`.

**Reproduction:** start a silent process, issue `process/read` at its latest sequence with a very
large `waitMs`, then issue `process/terminate` on the same connection. Termination is not handled
until the read expires.

### 45. [P2][Persistence integrity] Archive/unarchive report success or failure after divergent partial writes

**Status:** Assigned

Archive renames the rollout and discards the result of `mark_archived`
(`codex-rs/thread-store/src/local/archive_thread.rs:41-60`). Unarchive also renames first, may then
fail while touching/parsing the file, and discards `mark_unarchived`
(`thread-store/src/local/unarchive_thread.rs:56-100`). Disk and SQLite can disagree, and a failed RPC
may already have moved the file.

**Reproduction:** inject a state-DB write failure after archive has looked up and renamed the rollout;
the RPC succeeds while the DB still lists it active. Alternatively place a malformed rollout at a
valid archived filename and unarchive it; the rename succeeds before parsing fails, so the RPC
returns failure after the rollout has already moved.

### 46. [P1][Data integrity] Thread deletion is non-transactional and can fail after data is gone

**Status:** Assigned

App-server deletes every subtree rollout before state rows
(`codex-rs/app-server/src/request_processors/thread_delete.rs:39-79`). Local storage removes rollout
files before fallible name-index cleanup (`thread-store/src/local/delete_thread.rs:68-82`), and state
cleanup removes logs/memory/goals before the later SQLite transaction
(`state/src/runtime/threads.rs:1007-1107`). An I/O/SQLite error returns failure and emits no deletion
notification after irreversible partial deletion; a subtree may be only partly removed.

**Reproduction:** inject a name-index or state-DB failure partway through deleting a thread with
children. The RPC returns an error, but some rollout files and auxiliary state are already gone.

### 47. [P2][Data integrity / Settings] TUI settings updates are split across non-atomic writes

**Status:** Assigned

Model, effort, service-tier, and approval changes first persist global config and then issue a
separate `thread/settings/update` (`codex-rs/tui/src/app_shell/settings/controller.rs:300-431`). Some
paths also mutate local UI state before either call completes. If the second RPC fails, global config,
the active thread, and the rendered selection disagree; finding 31 then exits the TUI before the user
can reconcile them.

**Reproduction:** change a setting while making `config/write` succeed and
`thread/settings/update` fail. Restart or inspect another client: the global default changed while
the active thread did not.

### 48. [P2][Review correctness / TUI] Diff lines permanently hide changes beyond the visible width

**Status:** Assigned

Each diff side is ellipsis-truncated to its column
(`codex-rs/tui/src/app_shell/diff_view_view.rs:233-265`). Left/Right select files, and
`DiffViewState` has no horizontal offset or key binding (`app_shell/diff_view.rs:236-278`). A change
only at character 200 can render as two identical prefixes, with no way to inspect the actual edit.

**Reproduction:** change only the far-right suffix of a long line and open Edits in a normal-width
terminal. The old/new visible text looks identical; no key or mouse action reveals the suffix.

### 49. [P1][Availability / Input] Multi-megabyte paste has no size cap and blocks rendering

**Status:** Assigned

Composer insertion accepts an arbitrary paste, performs whole-string normalization/insertion, and
subsequent renders measure and wrap the complete buffer
(`codex-rs/tui/src/app_shell/composer.rs:157-175` and
`app_shell/composer_render.rs:21-327`). There is no paste/prompt byte ceiling or asynchronous path.
One accidental large clipboard can freeze the TUI and later create an oversized turn request.

**Reproduction:** bracket-paste a multi-megabyte string into the composer. The event loop stalls
during insertion and subsequent frames, with no warning or cancellation path.

### 50. [P2][Navigation / Sessions] Session browsing has no pagination beyond the first 20 rows

**Status:** Assigned

The list limit is 20 and every request sends `cursor: None`
(`codex-rs/tui/src/app_shell/sessions.rs:17,59-76`). `has_more` only produces a `+` indicator; no key
or mouse route consumes a next cursor, and page replacement keeps only those first rows
(`sessions.rs:83-92,264-289`). Search can find a specifically matching older session, but each broad
search result is itself limited to its first 20 matches.

**Reproduction:** create more than 20 active or archived sessions. Sessions displays `20+ sessions`,
but Down/PageDown stops at item 20 and there is no Load More/page action. A uniquely named older
session may be found with targeted search, but it cannot be reached by browsing.

### 51. [P2][Tool contract] `request_user_input` cardinality rules are prose-only and invalid calls reach the UI

**Status:** Assigned

The tool promises one to three questions and two to three options, but `JsonSchema::array` cannot
express min/max items (`codex-rs/tools/src/json_schema.rs:40-74,144-150`) and normalization only checks
that options are nonempty (`codex-rs/core/src/tools/handlers/request_user_input_spec.rs:108-136`). The
TUI then renders only the first three options while the parser still accepts hidden indexes. Zero
questions produces a nonsensical `(1/0)` modal and can submit an empty answer map. More than three
questions are accepted and shown sequentially despite violating the advertised contract.

**Reproduction:** issue the tool call with `questions: []`, four questions, or four options. Empty
questions show `1/0`; a fourth option is accepted but hidden by the UI; four questions are accepted
despite the declared maximum and are shown sequentially.

### 52. [P2][Approval protocol] The TUI ignores all richer or restricted approval decisions

**Status:** Assigned

Approve always serializes plain `Accept`, and permissions always use `scope: Turn`
(`codex-rs/tui/src/app_shell/approval.rs:122-158`). The UI ignores `available_decisions`,
`acceptForSession`, exec/network policy amendments, cancel, session permission scope, and strict
review controls exposed by the v2 protocol. It can offer a decision the server explicitly did not
advertise, and it cannot make a persistent approval even when supported.

**Reproduction:** send a command approval with
`availableDecisions: ["acceptForSession", "decline"]`. The TUI still offers generic Approve and sends
plain `accept`; a turn-scoped permission also prompts again for a matching command in a subsequent
turn.

### 53. [P2][MCP consent / TUI] Long URL elicitations truncate away the URL and cannot be inspected

**Status:** Assigned

The elicitation combines message and URL, then renders at most 42 graphemes
(`codex-rs/tui/src/app_shell/elicitation.rs:24-35` and
`input_request_view.rs:252-273`). There is no scroll, expansion, or copy action. A long explanatory
message can hide the destination entirely while Accept remains active.

**Reproduction:** trigger an MCP URL elicitation whose message exceeds 42 characters. The URL is not
visible, yet the modal still permits acceptance.

### 54. [P2][MCP interaction / TUI] The modal advertises Enter for Accept, but Enter does nothing

**Status:** Assigned

The renderer labels the action `Accept ↵` (`codex-rs/tui/src/app_shell/input_request_view.rs:255-267`),
while `elicitation_choice_from_key` accepts only `a/A`, `d/D`, and `c/C`
(`codex-rs/tui/src/app_shell.rs:3947-3956`).

**Reproduction:** trigger an acceptable URL or empty-form MCP elicitation and press Enter. The modal
and request remain unresolved; only the undocumented `a`/`A` shortcut or clicking Accept resolves
it as accepted.

### 55. [P2][Session resume compatibility] Custom interactive session sources disappear from normal pickers

**Status:** Assigned

Rollout discovery includes custom sources such as `Custom("atlas")` and `Custom("chatgpt")`
(`codex-rs/rollout/src/lib.rs:25-31`), but app-server's `ThreadSourceKind` cannot represent `Custom`
(`codex-rs/app-server-protocol/src/protocol/v2/thread.rs:1153-1169`). The TUI always filters with an
explicit CLI/VS Code source kind (`codex-rs/tui/src/resume_picker.rs:1843-1854` and
`tui/src/lib.rs:568-598`). Such sessions cannot be found by picker, `--last`, or name, even though an
exact UUID can resume them.

**Reproduction:** create a rollout with source `Custom("atlas")`, then open Resume or use
`codex resume --last`. It is absent; supplying its exact thread ID still works.

### 56. [P2][Windows remote protocol] Read-command file actions are dropped across OS path conventions

**Status:** Assigned

When app-server and executor use different path conventions, command item construction cannot
localize a foreign `PathUri` and deliberately drops Read actions
(`codex-rs/app-server-protocol/src/protocol/item_builders.rs:140-168`; covered by tests in
`item_builders_tests.rs:4-40`). Output still appears, but file references lose their interactive open
action.

**Reproduction:** connect a Linux/macOS app-server to a Windows executor (or vice versa), run a Read
command, and inspect the resulting `ThreadItem.commandActions` from an app-server client. The action
present for same-convention paths is absent. The current Better Codex TUI does not render command
actions on either platform, so this is a Windows cross-host protocol/client defect rather than a TUI
difference.

### 57. [P2][Input routing / TUI] Paste behaves differently from typing across modal editors

**Status:** Assigned

Paste bypasses the keyboard dispatcher and always calls composer-oriented `insert_text`
(`codex-rs/tui/src/app_shell.rs:363-368`). That method returns during dashboard or MCP editing
(`app_shell.rs:1359-1375`), so paste is discarded in session search/rename, settings editors, and MCP
fields even though typed characters work. It omits `diff_view`, so paste while a diff is open mutates
the hidden composer and appears only after closing the diff. A request-user-input modal can also lose
paste if dashboard focus remains set when the request arrives.

**Reproduction:** focus Sessions, press `/` or `n`, and paste text; nothing is inserted. Separately,
open Edits, paste, close Edits, and observe the pasted text unexpectedly appear in the composer.

### 58. [P2][Visual / Input] Literal tabs are hidden even though they are submitted

**Status:** Assigned

Paste normalization preserves `\t` (`codex-rs/tui/src/app_shell/composer.rs:157-175`), but cursor
calculation and Ratatui rendering treat the control grapheme as zero-width
(`app_shell/composer.rs:42-52`). The tab is therefore collapsed rather than expanded to a terminal
tab stop, while `submission_text` still sends the literal byte. Users cannot see that their prompt
contains extra whitespace.

**Reproduction:** bracket-paste `a<TAB>b` into the composer. It displays as `ab`, but the resulting
`turn/start` input still contains the tab.

### 59. [P2][Diff correctness] The same file is duplicated across session turns

**Status:** Assigned

`session_file_refs` deduplicates item files only against an aggregate diff within the same turn; it
does not merge the same path across turns (`codex-rs/tui/src/app_shell/diff_view.rs:111-132`). Session
file counts and stats therefore count repeated edits as separate files, and Session Edits shows
multiple non-net versions instead of one coherent session diff.

**Reproduction:** modify the same file in two turns, then open Session Edits. The file appears twice
and the dashboard file count is inflated.

### 60. [P2][Diff correctness] Git C-quoted paths are parsed incorrectly

**Status:** Assigned

`diff_path_token` stops a quoted path at the first quote and only attempts to unescape `\"`; it does
not parse Git's backslash/octal C quoting. Header normalization merely trims surrounding quotes
(`codex-rs/tui/src/app_shell/diff_model.rs:401-424`). Filenames containing quotes, tabs, newlines, or
quoted non-ASCII bytes can be truncated or assigned to the wrong file. The later replacement of
`\"` cannot repair an escaped quote because tokenization has already stopped at that quote, breaking
selection and deduplication; other C escapes are never decoded.

**Reproduction:** produce a Git diff for a filename requiring C quoting, such as one containing a
tab or quote, and open Edits. The displayed/associated path is malformed or truncated.

### 61. [P2][Visual / Performance] Carriage returns corrupt CRLF and progress output

**Status:** Assigned

Both compact transcript and Full Tool Output blindly replace every `\r` with `\n`
(`codex-rs/tui/src/app_shell/transcript_view.rs:403-408` and
`app_shell/tool_output.rs:63-77`). CRLF consequently becomes two line breaks, while progress programs
that repaint one line with carriage return become an ever-growing list. This is both visually wrong
and amplifies output memory/rendering cost.

**Reproduction:** run `printf 'a\r\nb\r\n'`; blank lines appear. Run a progress loop or `tqdm` that
uses `\r`; every repaint becomes another line instead of updating one line.

### 62. [P2][Visual / Output] Full Tool Output collapses tab-separated fields

**Status:** Assigned

The transcript view expands tabs, but the full-output path sends literal tab controls through ANSI
processing and Ratatui (`codex-rs/tui/src/app_shell/tool_output.rs:63-77`). Ratatui does not render a
tab stop, so fields collapse and the detailed view disagrees with the compact card.

**Reproduction:** run `printf 'left\tright\n'` and open Full Tool Output. It renders as `leftright`
instead of separated fields.

### 63. [P2][Prompt fidelity / TUI] Submission strips meaningful leading and trailing whitespace

**Status:** Assigned

`ComposerState::submission_text` applies `.trim()` to the entire prompt
(`codex-rs/tui/src/app_shell/composer.rs:59-60`), and queue/history use the same transformed value.
This changes pasted code, indentation-sensitive formats, trailing blank lines, and prompts that
deliberately begin/end with whitespace; the UI gives no indication that content was rewritten.

**Reproduction:** paste a prompt whose first line starts with spaces or whose final blank line is
material, submit it, and inspect `turn/start`. The leading/trailing whitespace is gone.

### 64. [P2][Queue interaction / TUI] Clearing a queued edit cannot delete or cancel it

**Status:** Assigned

`save_queued_message_edit` writes back only when the edited message is nonempty
(`codex-rs/tui/src/app_shell/composer.rs:273-290`). Clearing all text therefore preserves the old
queued entry. There is no other removal action, queued messages block session switching, and the old
message may later submit automatically.

**Reproduction:** during a turn, type a message and press Tab to queue it; press Alt-Up, clear all
text, then Alt-Down or Enter. The supposedly cleared original remains queued and is sent later.

### 65. [P2][macOS input] Home, End, and forward Delete are deliberately unbound

**Status:** Assigned

Plain Home/End/Delete actions are compiled only on non-macOS
(`codex-rs/tui/src/text_input.rs:448-455`), with a test explicitly asserting that macOS leaves them
unbound (`text_input_tests.rs:145-151`). Terminals that emit these keys (including Fn-based Mac
bindings) cannot move to a line boundary or delete forward, even though the TUI receives the events.

**Reproduction:** on macOS, focus any text editor and press Home, End, or Fn+Delete. Nothing happens;
the same events work on Linux.

### 66. [P2][Error reporting / Compaction] Local compaction drops the operation context on failure

**Status:** Assigned

The local compaction path emits the raw stream error and discards its task-level contextual result
(`codex-rs/core/src/compact.rs:300-316` and `core/src/tasks/compact.rs:78-81`). The remote path adds a
useful compaction prefix (`compact_remote.rs:172-178`). An ignored regression test at
`core/tests/suite/compact.rs:3556-3632` currently fails because local behavior returns the raw error.

**Reproduction:** invoke backend compaction through `thread/compact/start` (or a focused `Op::Compact`
test) and force the provider stream to disconnect. The emitted local error is the generic stream
failure rather than an error identifying context compaction as the failed action. The new app shell's
`/compact` palette action is not wired, as separately noted in finding 74.

### 67. [P2][Resource leak / Hooks] Codex never removes spilled hook-output files

**Status:** Assigned

Every oversized hook result creates a unique spill file
(`codex-rs/hooks/src/output_spill.rs:20-24,38-60,91-95`), but production ownership has no cleanup
path. Repeated hooks grow the temp/cache area and leave command output on disk after the turn/session
ends until an external or OS cleanup removes it.

**Reproduction:** repeatedly run a hook whose output crosses the spill threshold, then inspect the
spill directory after the sessions exit. Each generated file remains.

### 68. [P2][Documentation / API compatibility] App-server endpoint summaries use incorrect wire field names

**Status:** Assigned

Two README endpoint-summary lines do not match the v2 wire contract:

- MCP OAuth documents `authorization_url` in `codex-rs/app-server/README.md:233`, while the protocol
  field is `authorizationUrl` (`app-server-protocol/src/protocol/v2/mcp.rs:205-210`).
- Feedback documents `conversation_id` at README line 240, while the actual field is `threadId`
  (`app-server-protocol/src/protocol/v2/feedback.rs:8-20`). Unknown fields are ignored, so following
  the endpoint summary can silently lose thread association rather than producing a clear error.

**Reproduction:** implement a client from either README summary. The OAuth client expects the wrong
response field, while feedback sends a request field that the server accepts and ignores without
associating a `threadId`.

### 69. [P2][Execution correctness] Sandboxed launches silently drop custom `arg0`

**Status:** Assigned

The exec protocol promises a process-visible argv0 override
(`codex-rs/exec-server-protocol/src/protocol.rs:134-136`), and unsandboxed execution preserves it.
Sandbox transformation omits it; a TODO acknowledges the loss
(`codex-rs/exec-server/src/process_sandbox.rs:115-153`). Program behavior can therefore differ solely
because sandboxing was enabled. This applies to the supported Linux/macOS remote launch paths;
sandboxed Windows remote launches are rejected rather than transformed.

**Reproduction:** on a Linux/macOS executor, execute a program that prints `argv[0]` with
`arg0: "custom"`, once unsandboxed and once sandboxed. Only the unsandboxed process sees `custom`.

### 70. [P2][Persistence integrity] Thread metadata RPCs can fail after SQLite already committed

**Status:** Assigned

`update_thread_metadata` first commits `apply_metadata_update`
(`codex-rs/thread-store/src/local/update_thread_metadata.rs:72-83,200-373`) and only afterward performs
fallible rollout, name-index, and Git compatibility writes at lines 85-165. If any later write fails,
the RPC reports failure while SQLite already exposes the new metadata; reindex/recovery can later
disagree or revert it.

**Reproduction:** make a rollout or name-index write fail after the SQLite update. The client gets an
error, but a fresh database-backed list shows the requested metadata.

### 71. [P2][Interactive requests / Backpressure] A full in-process queue rejects consent requests

**Status:** Assigned

The in-process client classifies every `ServerRequest` as nonessential
(`codex-rs/app-server-client/src/lib.rs:115-125`). If its bounded event queue is full, `try_send` drops
the request and calls `reject_server_request` with `-32001` (`app-server-client/src/lib.rs:208-227,537-550`).
Approvals and tool questions can therefore fail before the user sees them, and downstream code maps
that transport rejection to a denial or tool failure.

**Reproduction:** saturate the in-process event queue with notifications while a command approval or
`request_user_input` arrives. No modal appears; the backend receives `queue is full`.

### 72. [P2][Remote resources / Protocol] `fs/readFile` allows payloads that cannot cross the transport

**Status:** Assigned

Local FS accepts files up to 512 MiB (`codex-rs/exec-server/src/local_file_system.rs:30,533-550`).
App-server reads the whole file and base64-encodes it
(`codex-rs/app-server/src/request_processors/fs_processor.rs:64-76`), while remote WebSockets cap a
message at 128 MiB (`codex-rs/app-server-client/src/remote.rs:65-67,788-791`). A binary file around
96 MiB approaches or exceeds the wire limit once base64 and JSON framing are added; larger
allowed files can transiently require far more memory.

**Reproduction:** use remote `fs/readFile` on a roughly 100-MiB file. The server reads and
base64-copies it, then the response exceeds the WebSocket maximum and the request/connection fails.

### 73. [P2][Maintainability / Reliability] The central TUI controller has become an unsafe change unit

**Status:** Assigned

`codex-rs/tui/src/app_shell.rs` is 3,961 lines, `ShellState` has 78 fields, and `handle_key` alone is
382 lines. Its 12,391-line test file reaches into the same private state. Relative to
`origin/sync/openai-2026-07-12`, those two files contain 10,153 insertions. Input routing, transport
error policy, session lifecycle, transcript reduction, approvals, and output rendering remain
coupled. The context-dependent paste defects (finding 57) and fatal error boundary (finding 31)
illustrate the reliability risks already present in this change unit. A coherent first extraction is
a unified modal/input router plus a recoverable command-result boundary, followed by session and
transcript reducers.

### 74. [P3][Discoverability / TUI] The command palette advertises an intentionally inert action

**Status:** Assigned

The palette includes `Compact context` with the detail `Context compaction action is not wired yet`
and `enabled: false` (`codex-rs/tui/src/app_shell/command_palette.rs:152-157`). Its dispatch branch is
empty but unreachable while the entry remains disabled (`app_shell.rs:1880-1903,1958-1962`). The
entry remains navigable, and Enter only emits its `not wired yet` detail as a status line. This
presents a core workflow as a dead control rather than omitting it or implementing it.

**Reproduction:** open the command palette, navigate to Compact context, and press Enter. The palette
stays open and a status line reports that the action is not wired.

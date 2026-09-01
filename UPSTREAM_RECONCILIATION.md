# Upstream reconciliation record

This engineering record accompanies `TASK.md`. It freezes the upstream comparison,
records intentional Better Codex boundaries, and preserves the semantic references
used by the implementation. It is not a request to restore the upstream CLI or TUI.

## Frozen comparison

- Better Codex start: `603b1e9080ef42eb80f55e2d011a9f2010a79d60`
- Previous semantic audit target: `678157acaa819d5510adfe359abb5d0392cfe461`
- Frozen upstream target: `86b1123ff6b5d089a146be4e603a324cf454223a`
- Frozen target timestamp: `2026-08-14T05:49:33Z`
- Frozen target subject: `Enable parallel tool calls for all model prompts (#38499)`
- Commits in the comparison range: 916

`678157aca` is an ancestor of the frozen target. The target is one commit beyond
the previously recorded `636e505c5cd809bdce37314f77130ffb4e45c46b`.

## Consolidation outcome

The selected semantic ports and Better-specific adaptations are integrated into a
single consolidation history. The history includes the independent indexed-copy
feature and every implementation stream that had been split across the five
upstream-checkpoint branches. The old branches are recovery landmarks only until
their guarded deletion; they are not work that must be resumed.

The integrated families are managed authentication and redaction; live approval
and Guardian hardening; bounded MCP 2026 negotiation, runtime, plugins, and unified
skills; durable response/history/persistence, usage, revert, and queues; Better TUI
queue adaptation; and bounded Code Mode protocol, host, transport, and app-server
routing. The detailed classifications below remain the design and compatibility
record for those choices.

The upstream target remains a frozen semantic checkpoint. Better's standalone app
shell, backend boundaries, bounded context, fail-closed safety rules, and agent
harness boundary remain authoritative.

## Inventory boundaries

The comparison found 2,573 upstream-changed paths, 440 Better-changed paths since
`3efdfe8f`, and 118 paths in both sets. The overlap is concentrated as follows.

| Area | Overlapping paths | Reconciliation rule |
| --- | ---: | --- |
| `codex-rs/core` | 31 | Semantic ports only; keep harness and bounded context invariants |
| `codex-rs/tui` | 15 | Adapt through app-shell APIs; never restore upstream UI orchestration |
| `codex-rs/cli` | 12 | Keep Better's standalone entry point and strict option conflicts |
| `codex-rs/app-server*` | 13 | Port v2 backend and wire semantics; adapt the app-shell client |
| `codex-rs/login` | 6 | Port managed-auth and routed HTTP behavior |
| `codex-rs/codex-mcp` | 4 | Centralize runtime and tool mutation in the connection manager |
| workspace manifests and locks | 4 | Regenerate Cargo and Bazel locks together |
| all other crates and top-level files | 33 | Review by owning priority; exclude release and maintenance churn |

Better Codex replaced the upstream TUI rather than extending it: 791 upstream TUI
paths were deleted and 379 app-shell-oriented paths were added after the literal
import. In particular, upstream `app.rs`, `chatwidget.rs`, `bottom_pane`, status-card,
onboarding, theme-picker, and upstream snapshot changes are reference material. The
retained adapter surface is `tui/src/app_server_session*`, while product behavior is
owned by `tui/src/app_shell*`.

The retired checklist from the parent of `3efdfe8f` was reviewed. Its intentional
KEEP decisions remain in force: bounded explicit skills, developer/MCP hints,
`AGENTS.md`, environment/world-state and hook content; current-turn compaction;
user provenance; per-turn instruction refresh; complete world-state tombstones;
fail-closed managed networking; sandboxed custom `argv[0]`; strict CLI conflicts and
foreign paths; failure-atomic persistence; bounded remote queues and request
timeouts; lossless interactive backpressure; and custom-thread filtering. Its only
unchecked conditional item (audio, Bedrock, spend control, and realtime expansion)
is excluded unless a selected priority has a strict dependency on it.

## Classification

The selected upstream work is classified into five implementation streams plus
prerequisites. Repository release/publishing, Windows/WSL product expansion,
upstream TUI, desktop/web/IDE surfaces, issue/CI maintenance, Bedrock expansion,
audio UX, and internal-only telemetry are excluded. Later follow-up commits in the
frozen range supersede earlier implementations when they preserve Better's stronger
bounds.

### 1. Approvals, Guardian, safety, trust, and redaction

**Port immediately:** fail-closed network amendments (`3834c47c` #36037), bearer
redaction (`bab7c2dc` #36908), nested safety-buffering metadata (`9558d830`
#37882), and strict MCP automatic review (`4e5a08fe` #38492).

**Adapt:** approve-for-me (`b7a61066` #36373); command presentation/persistence
redaction (`fcf636a4` #36893); effective project trust (`17801b42` #36960 and
`683716ce` #38390); managed authentication (`2994f545` #37132); managed/cyber
model review (`757c151a` #37055, `208f05b2` #37511, `c2bcb9a2` #37513,
`e734a1a5` #37516, `2e3a1702` #37519); step/current-policy review
(`dd43a996` #37618, `d06dc732` #37851, `f2a6f258` #38046); shared MCP and
network decisions (`67afc796` #38081, `2230d644` #38108, `357696c5` #38299);
and non-interactive delegates (`95aada11` #38205). Plugin-boundary work from
`56b82e67` #37027 is split with stream 3.

PR #37057 is a release/0.146 backport (`7558bede`), not an ancestor of frozen
main, and is excluded in favor of main's #37055 implementation. Upstream trust,
cyber-default, and approval TUI code is excluded; Better's existing scrollable
approval and trust surfaces remain behind backend adapters.

Guardian V2 is retained as a focused experimental extension: scaffold (`fe614a63`
#38336), risk persistence (`72fa74fb` #38363), Luna sampling/pooling
(`a7b8c074` #38368, `91101249` #38383, `66919805` #38406), parent-permission
confinement (`a7e9fb54` #38377), classification/action correlation (`d167a360`
#38409), bounded transcript/action context (`73862481` #38414, `53eaa297`
#38441), and gated Node evidence (`053dda6b` #38397, `507ef0b3` #38427,
`4d9f3021` #38454). Upstream's 320 KiB transcript allowance violates the task;
Better caps a single review fragment below 10K tokens, flags any >1K-token item for
manual review, redacts every evidence form, and bounds image count/bytes. The frozen
V2 sampler discards its result, so it remains non-authoritative until a synchronous,
action-correlated, fail-closed decision bridge is implemented and tested.

The shared backend decision representation was introduced compatibly: legacy
`"denied"` still deserializes while the new bounded rejection-reason shape is emitted.
The active approval policy is read at decision time, never cached in a binding or
startup snapshot. Raw executable material exists only at the execution boundary;
live items, rollouts, replay, raw projections, diagnostics, and the TUI receive the
same redacted presentation.

Prerequisites include stale-review cleanup (`5c18cc0a` #34371), rejection reasons
(`e52c35b0` #34400), the V2 flag (`bb24b67d` #35049), network cancellation
(`63fe5a6b` #35267), strict elicitations (`287e1020` #36365), current-turn policy
(`6a828ca2` #36912, adapted without removing `LiveApprovalPolicy`), centralized
session decisions (`778b8698` #37128), resume restoration (`66225461` #37368),
step prefix rules (`420accf1` #37641), conservative restrictions (`93beee91`
#38183), and current rejection behavior (`020f6c96` #38256).

### 2. MCP, tools, OAuth, catalog, and elicitation

**Port:** the rmcp 3 bridge (`61de0d8f` #35720, then `a05bcda3` #36001);
negotiated 2026 discovery/client behavior (`be2e4afc` #35724, `f2bee854`
#35725); cached, concurrent, optional, and reusable startup (`3bbf1fe7` #35590,
`d9e1c9cd` #35742, `84ccb293` #35777, `fbf666fa` #35937, `85c082cc`
#36011, `45c9c74e` #38493); runtime assembly and step bindings (`b293412c`
#36119, `9a46fd33` #36120, `385fe95c` #36360, `952e87d3` #37101);
strict names (`1e489ada` #37020, `f21dc463` #37022, `1fe6be97` #37035,
`31711668` #37053); call IDs/events (`248d8c0e` #37477, `41014b11`
#37494); app-server extensions (`ee46c5ba` #36910); elicitation failures
(`e20616d2` #38035); safe HTTP catalog caching (`0ca43990` #37970); and
OAuth/transport hardening (`b2543af0` #38040, `6dc3ac87` #38052,
`4c89139d` #38089, `379cb684` #38245, `b87327f4` #38436, `1da59ad2`
#38448). Conformance gates use `61dc1d97` #36810 without its CI machinery.

**Adapt:** the upstream connection-set architecture into focused private
`codex-mcp` modules, retaining Better's encrypted-content precedence, cached
executor capability roots, bounded `request_user_input`, branding, and live
approval-policy lookup. Hosted plugin event consumption and the generic bounded
event transport are integrated through that runtime boundary. Upstream TUI,
release, and blanket core rewrites are excluded.

The fork began hard-pinned to rmcp 1.8 and MCP 2025-06-18. The consolidation keeps
legacy wire behavior while updating the APIs and both lockfiles. Modern mode is
opt-in/negotiated and enforces at most 100 pages, 2,048 ordinary catalog items,
8,192 app tools, 64 KiB cursors, repeated-cursor rejection, total timeouts, and
message-size limits (`3e3ae088` #36039 and `58256999` #36534). Stable bindings
capture catalog revisions but must revalidate visibility and must never capture a
stale approval policy. Environment-scoped OAuth retains readable legacy local
credentials; per-client extension state, headers, custom CAs, proxies, and callback
ports may not leak across sessions or environments.

Prerequisite connection/runtime commits are `2d85e6d3` #34522, `f6aad1f3`
#34561, `65f8bf68` #34588, `516f1e2a` #34708, `e497325a` #34930,
`e19e6531` #34952, and `34b935e3` #34957. Other required foundations include the
feature flag (`65ae4c26` #34747), credential isolation (`bf4d3f51` #36306,
`164b3bfe` #36310), thread-bound calls (`c4f2746c` #36355), late startup
(`17df7545` #36895), remote handshake timeout (`e244a9d9` #37168), stable handler
reuse (`e1831db7` #37273), and lazy-cache supersession (`1151b23f` #37261,
`7093e8c4` #38217).

### 3. Plugins, skills, apps, and extension architecture

**Adapt:** Agent Plugin manifests and publishing (`a28374e0` #35105,
`32329b28` #35254); experimental remote/local search (`64b2a300` #36402,
`a850875a` #36409, `d75f94a9` #36919); portable install/storage and MCP config
(`2b5bdcf6` #36544, `bd12b3a9` #36796); eligibility (`12b961d4` #35837,
correcting TASK's nonexistent #36837); deferred/custom tools and independent apps
(`12288240` #36856, `d4fb78bf` #36857, `18f03c1e` #36900); runtime isolation
(`56b82e67` #37027); unified skill ownership, aliases, package reads, and settings
(`c5d94319` #37452, `33e365b1` #37457, `3b366654` #37461, `b3278e96`
#37466, `ce22ea97` #37488, `ba94150c` #37489, `beac16cc` #37503,
`45f8cafa` #37505, `09f47c87` #37808, `34ecac1f` #37810, `1c042dd4`
#37812, `bfb7790e` #37832, `680934ad` #37833, `3c60d4da` #37838,
`7d486ffa` #37979, `3d4d253f` #37984, `69ae7829` #38167, `130c7c93`
#38261, `5664a5c0` #38268); and bounded annotations/delegation/app search
(`3711943d` #38467, `5620bab6` #38475, `cbe85e11` #38484).

Claude marketplace inference (`8d34c066` #34979) and Cursor migration
(`da2c7ca8` #36361) are adapted with deduplication. Bedrock marketplace
`48ebbf53` #34931 is excluded; the provider-neutral auth-routed catalog
`f898ebca` #38429 is used instead. Upstream chat UI, telemetry sidecars, release
machinery, and unrelated CLI UI are excluded.

`codex-plugin` remains the focused authority/provider model; `codex-core-plugins`
keeps legacy manifests, installed metadata, atomic replacement, and existing archive
bounds while adding portable root manifests, data roots, `mcp.json`, symlink
skipping, and duplicate-entry rejection. Shared models, parsing, selection,
annotations, and explicit prompt budgets moved to `codex-skills`; provider/runtime
ownership moved to `codex-skills-extension`. `codex-core-skills` was removed after
every consumer migrated.

Better's 3,600-byte per explicit skill and 32,000-byte aggregate turn limits,
validation, and visible omission/truncation warnings apply to every provider.
Delegation instructions remain bounded typed context fragments. Resumed rollouts keep
old `skills.read` arguments during transition. Other prerequisites include symlink
hardening (`720c9d68` #36967), explicit-only orchestrator skills (`1b90b1d1`
#36976), cache freshness (`5d89ab65` #37000), loader unification (`a4b129eb`
#37439, `e75a1888` #37440, `e58d9ef4` #37444), and bounded Cursor paths
(`f344a80a` #37747).

### 4. App-server, protocol, persistence, history, queue, revert, and usage

**Adapt:** sections and ordering (`85c6da1c` #35722, `ad6fc66b` #36007,
`c42ea41e` #36380, `1549756b` #37898); acknowledged and durable admission
(`bf7804c2` #36385, `e2c08379` #36410, `989a0b05` #36947, `b87981a5`
#36952, `da2803c7` #38092, `cbb7e82a` #38275); persisted history and identity
(`63002bdb` #37871, `722784e9` #37926, `4496ba3f` #38033, `99915080`
#38045, `4ef836f8` #38127, `8d4d5738` #38244, `361fe2d2` #38272,
`4b07886d` #38274); usage (`842fae26` #38270, `f1a1fce2` #38281,
`1e71e35d` #38282); durable revert and recovery (`b1373b74` #38292,
`363427b5` #38303, `42bb50d5` #38413, `4343b2bd` #38440,
`c6dee5f4` #38463); and the experimental bounded queue API (`9341b383`
#38456).

**Exclude:** old TUI picker, transcript, `/status`, and naming implementations
(`a1286d12` #36036 and `8bfa49e3`/`3b8d22ec`/`dbcd837c`/`449f099f`
#36948-#36951). Their backend semantics are adapted to the app shell.

The authoritative queue is independent of composer widgets and retains upstream's
100-item and 1 MiB input bounds. Better closes the start/delete crash window with
explicit durable lifecycle state, compare-and-swap ownership, and idempotent
recovery. Durable revert keeps rollout IDs distinct from thread IDs, uses immutable
replacement rollouts and transactional lineage materialization, reloads runtime
state, and preserves subscriptions. Usage remains separate from context pressure
and rate-limit displays.

State migration `0042` remains an upstream compatibility gate because upstream
uses it to remove legacy agent-job tables (`687f05cb` #34413) while Better still
has consumers. Upstream migrations `0049` through `0052` are preserved exactly,
while Better restores compatibility and adds durable queue state in the reserved
`10001` through `10003` range. A checksum-gated startup repair remaps databases
created by the pre-consolidation `0049` through `0051` drafts without altering
genuine upstream histories. Migration tests cover Better-at-0041, the historical
draft numbering, and upstream-advanced databases. Other retained prerequisites are item IDs
(`4a443994` #34645), single-writer ownership (`5c94796d` #34986), and the frozen
target's lineage/migration fixes (`6bb6e904`, `aac9f842`, `4bb7ee34`, `3a6f747d`,
and `2bd8727a`).

### 5. Code Mode, exec-server, environment, permissions, and remote transport

**Port:** the final gRPC message/service/session semantics (`8073dbb2` #37510,
`61a3dd43` #37530, `c0ad3ab0` #37745, `9be95745` #37906, `f8821d85`
#37922, `1e557a55` #38041, `ba2fb483` #38072, `85f33177` #38087,
`bde723ae` #38257, `5104cb64` #38288); retained WebSocket foundations
(`0dfa778d` #35078, `f61b51dd` #35098, `60c722e0` #36812); environment
readiness, primary selection, uploads, and inheritance (`462ed19a` #35652,
`0a6616f4` #35874, `6c13b113` #35875, `250de82b` #35878, `fe01054a`
#35895); step routing and permission policy (`3d805abd` #36121, `bdda5da5`
#36133, `bf4d3f51` #36306, `ef293f7a` #36329, `66ebeb70` #36357,
`b258c028` #36811, `30d99232` #37031, `bac3ef1d` #37038, `ed2f985a`
#37040); and remote sandboxing/selection (`511262b9` #37480, `dd43a996`
#37618, `ee7815da` #37862, `a603d7ca` #37875, `34db7e55` #38043,
`b43de776` #38067, `f4936d7a` #38086, `c30a3e49` #38356,
`4f703217` #38416, `781445f7` #38423, `535795f7` #38461).

**Adapt:** all transports remain additive. Better has in-process and
length-prefixed stdio Code Mode, but contrary to the task's initial premise it has
no Code Mode WebSocket transport. The new host transport therefore sits behind a
capability-negotiated abstraction. Dispatch is bounded and every cell/callback is
keyed by its owning turn. Plain HTTP/2 is loopback-only; non-loopback TCP requires
authentication and TLS. gRPC uses the configured HTTP client, proxy, custom CA,
environment authentication, redaction, timeouts, and diagnostic bounds. The final
environment selection/config model is merged with Better's approval-policy hot swap,
strict `PathUri`, world-state bounds, concurrent exec reads, and fail-closed network
guard.

**Defer/exclude:** the mechanical `codex-code-mode-runtime` extraction
(`97576b17` #36217) remains deferred unless dependency isolation needs it, and the
embedded fallback is retained. Old TUI, Windows-only transport/sandbox branches,
release/CI, and telemetry-only work remain excluded. The shared V8 sandbox
invariant from `2e32d958` #36374 is retained.

The existing 64 MiB frames and bounded request/cell/session caches are retained.
The v1 gRPC `max_heap_size_bytes` field remains reserved for wire compatibility
but is deprecated and rejected before session creation until the embedded runtime
can enforce it. Better does not yet ship a generated gRPC client that could
preflight this unsupported limit.
Retained prerequisites include per-session limits (`9d00bb01` #37114),
stalled-request invalidation (`8e3b5d3e` #36830), default yields (`d0c8f422`
#37352), executor-local config and
capability negotiation (`95c7265e` #37408, `646f7c0a` #37654), and deterministic
yield tests (`e0de12a1` #38321). Proto generation is isolated because Better's Bazel
Rust rules predate upstream's prost support; Cargo/Bazel source availability and
both lockfiles must agree without importing unrelated toolchain changes.

### Prerequisites and intentionally excluded work

`86b1123ff` (#38499) was classified and reviewed as a harness/model-configuration
prerequisite, but was not used to justify Better-specific harness changes.
Mechanical prerequisite refactors were taken only when a selected stream required
them. All other commits in the frozen range remain excluded when they affect only
the product surfaces and repository machinery listed above.

## Breaking and migration audit

The following surfaces remain explicit post-consolidation compatibility
requirements.

| Surface | Compatibility requirement |
| --- | --- |
| app-server v2 | Optional request fields remain nullable; experimental queue and Guardian fields are gated; schemas, generated TypeScript, README examples, and public JSON-RPC tests move together |
| raw response items | Existing event names and persisted envelopes continue to deserialize; new completion, timestamps, risk, and usage metadata are additive |
| CLI and config | Existing Better options keep strict conflicts; trust, managed auth, MCP protocol/OAuth, plugins, transport, and environment settings have safe defaults and regenerated schema |
| rollout/session resume | Pre-risk, pre-envelope, pre-queue, and paginated histories load; interrupted/reverted lineages have explicit states and distinct IDs |
| SQLite | Migrations are forward-only, failure-atomic, and tested from old and partially migrated state |
| MCP negotiation | 2025-06-18 remains compatible; 2026-07-28 and app-server extensions are negotiated, never assumed |
| tools and namespaces | Canonical names and collisions are strict without invalidating saved encrypted MCP output or stable bindings |
| remote paths and permissions | Foreign paths never silently become host paths; active-step permissions constrain Guardian, MCP, Code Mode, uploads, patches, and streaming |

## Integrated delivery record

The numbered stages were implementation boundaries, not permanent branches. They
were integrated in dependency order and then reconciled across stream boundaries.

| Family | Integrated result | Retained boundary |
| --- | --- | --- |
| Foundation and auth | Response identity/metadata, copy ordinals, redaction, trust, managed auth, and live command/apply-patch decisions | Existing wire forms remain readable; raw commands stay at execution boundaries; remote app-server owns remote credentials |
| Guardian | Bounded risk evidence, cancellation-safe sampling, and approval-pipeline integration | Guardian V2 stays gated and non-authoritative where the complete shared decision path is unavailable |
| MCP | rmcp 3 bridge, negotiated 2026 discovery, bounded transports/catalogs, stable bindings, OAuth hardening, elicitation, app-server status, and recovery | 2025-06-18 remains compatible; modern behavior requires both request and negotiation; delayed decisions re-read live policy |
| Plugins and skills | Portable manifests and roots, secure archives, bounded remote search, package reads, aliases, and unified skill ownership | Better plugin authority and explicit skill/context budgets remain authoritative; obsolete `codex-core-skills` ownership is removed |
| Persistence and usage | Stable IDs, history envelopes, fractional timestamps, sections, usage, rollout lineage, migration compatibility, and durable revert | Writes remain failure-atomic; legacy histories and advanced databases remain readable; usage is distinct from context pressure |
| Durable queue and app shell | Persisted admission, lifecycle ownership, recovery, revert ordering, and TUI queue management | Queue state is backend-owned, bounded to 100 items and 1 MiB, and presented through Better's app shell |
| Code Mode | Bounded gRPC protocol/session/host transport and app-server/CLI routing | Existing transports remain additive; unsupported heap limits fail before session creation; non-loopback access requires configured protections |

Cross-stream consolidation repaired envelope compatibility, persistence fixtures,
authorization/redaction semantics, skill aliases, queue ownership, history
projection idempotency, legacy composite-rollout recovery, retained World State
revocation and identity, exactly-once nested command completion, and Code Mode
routing without restoring excluded upstream product surfaces.

## Validation record

Validation was intentionally focused by owning crate and integration surface:

- Auth and safety validation covered config, login, CLI, TUI, realtime, proxy,
  app-server, redaction, approval races, and Guardian fail-closed behavior.
- MCP and extension validation covered legacy/modern negotiation, response
  correlation, transport and OAuth recovery, bounded discovery, runtime bindings,
  plugin APIs and archives, search, and unified skill loading/rendering.
- Persistence validation covered protocol/history compatibility, old and advanced
  migrations, lineage and revert, thread-store projection, request serialization,
  durable queue state and recovery, and Better TUI queue behavior.
- The final Code Mode gate covers `codex-code-mode-protocol`, `codex-code-mode`,
  `codex-code-mode-host`, app-server routing and queue coexistence, and CLI routing,
  including yield timing, subscription backpressure, and provider termination.
- User-visible TUI changes used snapshot coverage and pending snapshots were
  reviewed before acceptance.

The recorded final gates include 1,478 `codex-tui` tests, 80
`codex-code-mode-host` tests, the selected Code Mode, Guardian, state, and protocol
suites, a combined core/extension/rollout/thread-store package gate, and 1,354
app-server/protocol/client tests. Focused final regressions cover retained A-to-B
and A-to-B-to-A state, current-empty revocation, collision-free skills catalog
identity, canonical nested command decline, durable revert, legacy rollout
normalization, and queue admission/recovery. The app-server gate had one transient
zsh subprocess initialization timeout under parallel load; its configured retry
passed, and the exact case passed alone. There were no final test failures.

The final generated-artifact pass regenerates the config schema and stable and
experimental app-server schemas, verifies the Bazel lock, and confirms that no
Cargo dependency change requires lock regeneration before the workspace fix pass
and `just fmt`.

The complete workspace `just test` was not run. `AGENTS.md` requires explicit
user approval for that suite, and approval was not given. Focused suites are the
recorded acceptance basis; this document does not claim a green full-workspace run.

## Recovery and branch cleanup

The complete pre-consolidation ref snapshot is preserved at
`/Users/loooonk/Projects/better-codex-pre-consolidation-2026-09-01.bundle` with
SHA-256
`2d8f245449adc6d7782de048a7293c1510b0e7f5087f367d485d14b7e19e9240`.
The bundle was verified with all nine pre-consolidation refs and is retained after
branch cleanup.

Once focused validation, generated artifacts, formatting, ancestry, and remote-tip
guards pass, the candidate fast-forwards `main`. Deleting the six exact non-`main`
GitHub branches, removing the clean auxiliary worktrees, and deleting the exact
local non-`main` branches is the final operational step. No old branch is needed to
continue the implementation after that point.

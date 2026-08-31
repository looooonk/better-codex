# Upstream reconciliation record

This engineering record accompanies `TASK.md`. It freezes the upstream comparison,
records intentional Better Codex boundaries, and gives each implementation commit a
stable reference. It is not a request to restore the upstream CLI or TUI.

## Frozen comparison

- Better Codex start: `603b1e9080ef42eb80f55e2d011a9f2010a79d60`
- Previous semantic audit target: `678157acaa819d5510adfe359abb5d0392cfe461`
- Frozen upstream target: `86b1123ff6b5d089a146be4e603a324cf454223a`
- Frozen target timestamp: `2026-08-14T05:49:33Z`
- Frozen target subject: `Enable parallel tool calls for all model prompts (#38499)`
- Commits in the comparison range: 916

`678157aca` is an ancestor of the frozen target. The target is one commit beyond
the previously recorded `636e505c5cd809bdce37314f77130ffb4e45c46b`.

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

Guardian V2 lands as a focused experimental extension: scaffold (`fe614a63`
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

The shared backend decision representation is introduced compatibly: legacy
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
approval-policy lookup. Hosted plugin event consumption waits for stream 3, while
the generic bounded event transport lands here. Upstream TUI, release, and blanket
core rewrites are excluded.

The current fork is hard-pinned to rmcp 1.8 and MCP 2025-06-18. The first stage
keeps legacy wire behavior while updating APIs and both lockfiles. Modern mode is
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
bounds while portable root manifests, data roots, `mcp.json`, symlink skipping, and
duplicate-entry rejection are added. Shared models, parsing, selection, annotations,
and explicit prompt budgets move to `codex-skills`; provider/runtime ownership moves
to `codex-skills-extension`. `codex-core-skills` is removed only after every consumer
migrates.

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

The authoritative queue will be independent of composer widgets and will retain
upstream's 100-item and 1 MiB input bounds. Upstream still has a crash window
between starting a turn and deleting its queue row, so Better will add explicit
pending/inflight/terminal state or an equivalent idempotent CAS. Durable revert
requires rollout IDs distinct from thread IDs, immutable replacement rollouts,
lineage materialization, a SQLite compare-and-swap, runtime reload, and subscription
preservation. Usage remains separate from context pressure and rate-limit displays.

State migration `0042` is a compatibility gate: upstream uses it to remove legacy
agent-job tables (`687f05cb` #34413), while Better still consumes them. The stage
must either migrate those consumers first or preserve compatibility at a later
version without reusing an upstream migration number/checksum. Tests must open both
Better-at-0041 and upstream-advanced databases. Other prerequisites are item IDs
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

**Adapt:** all transports remain additive. Better currently has in-process and
length-prefixed stdio Code Mode, but contrary to the task's initial premise it has
no Code Mode WebSocket transport. New host transports therefore start behind a
capability-negotiated abstraction. Dispatch becomes bounded and every cell/callback
is keyed by its owning turn. Plain HTTP/2 is loopback-only; non-loopback TCP requires
authentication and TLS. gRPC uses the configured HTTP client, proxy, custom CA,
environment authentication, redaction, timeouts, and diagnostic bounds. The final
environment selection/config model is merged with Better's approval-policy hot swap,
strict `PathUri`, world-state bounds, concurrent exec reads, and fail-closed network
guard.

**Defer/exclude:** defer the mechanical `codex-code-mode-runtime` extraction
(`97576b17` #36217) unless dependency isolation needs it, and retain the embedded
fallback. Exclude old TUI, Windows-only transport/sandbox branches, release/CI, and
telemetry-only work. Port the shared V8 sandbox invariant from `2e32d958` #36374.

The existing 64 MiB frames and bounded request/cell/session caches are retained.
Prerequisites add per-session limits (`9d00bb01` #37114), stalled-request invalidation
(`8e3b5d3e` #36830), default yields (`d0c8f422` #37352), executor-local config and
capability negotiation (`95c7265e` #37408, `646f7c0a` #37654), and deterministic
yield tests (`e0de12a1` #38321). Proto generation is isolated because Better's Bazel
Rust rules predate upstream's prost support; Cargo/Bazel source availability and
both lockfiles must agree without importing unrelated toolchain changes.

### Prerequisites and intentionally excluded work

`86b1123ff` (#38499) is classified as a harness/model-configuration prerequisite.
It will be reviewed for parity but not used to justify Better-specific harness
behavior. Mechanical prerequisite refactors are taken only when a selected stream
requires them. All other commits in the frozen range are excluded when they affect
only the product surfaces and repository machinery listed above.

## Breaking and migration audit

The following surfaces require explicit compatibility checks before their owning
stage can be complete.

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

## Staged delivery plan

Each numbered stage is independently compiled, tested, fixed, formatted, committed,
and pushed. Non-mechanical stages stay below 800 changed lines, and complex logic
stages stay below 500 lines; a numbered stage may therefore land as multiple
lettered commits.

0. Freeze and classify the upstream delta (this record; no production changes).
1. Reconcile shared review decisions, redaction, project trust, and managed auth.
2. Add Guardian V2 as a focused extension and persist bounded risk evidence.
3. Upgrade rmcp and negotiate MCP 2026-07-28 with bounded discovery.
4. Stabilize MCP runtimes, bindings, names, events, OAuth, and app-server status.
5. Add portable Agent Plugin package models and secure installation/search.
6. Move skill ownership into the skills extension and remove obsolete ownership.
7. Add persistence primitives, ID/envelope migrations, sections, usage, and reverts.
8. Add durable submission queues and adapt the Better app shell.
9. Add bounded gRPC Code Mode protocol, service, TCP transport, and app-server route.
10. Centralize active-step environment and permission routing across remote work.
11. Run migration, cross-workstream, remote-platform, boundedness, and performance validation.

## Validation ledger

| Stage | Upstream references | Compatibility impact | Scoped validation | Status |
| --- | --- | --- | --- | --- |
| 0 | `678157aca..86b1123ff` | None; documentation only | ancestry, commit count, overlap audit | Complete |
| 1B | `bab7c2dc` (#36908) | More complete bearer redaction; no wire/config change | `just test -p codex-secrets` (8 passed), `just fmt` | Complete |
| 1I | `17801b42` (#36960), `683716ce` (#38390) | Automatic trust now follows effective managed permissions; existing explicit trust remains compatible | two focused `codex-app-server` v2 tests, `just fmt` | Complete |
| 1A | `3834c47c` (#36037), `63fe5a6b` (#35267), `020f6c96` (#38256) | A failed allow amendment denies/cancels the owning call and cannot approve the host for the session | `just test -p codex-core network_approval` (25 passed; Linux regression compiled/ignored on macOS), `just fmt` | Complete |
| 1C | `9558d830` (#37882) | Typed `response.metadata` safety-buffering events are accepted while a present legacy top-level value remains authoritative | `just test -p codex-api`, focused `codex-core` safety-buffering integration tests (2 passed), scoped fix, `just fmt` | Complete |
| 1D | `e52c35b0` (#34400), `67afc796` (#38081) | Legacy `"denied"` decisions still load; new writes carry an optional UTF-8-safe 4 KiB reason, and MCP-only amendments fail closed outside MCP paths | `just test -p codex-protocol` (267 passed), `just test -p codex-app-server-protocol` (263 passed), focused core approval tests (42 passed), scoped fixes, `just fmt` | Shared type complete; MCP routing pending |
| 1G | `fcf636a4` (#36893) | Live, declined/completed, and replayed command items use one redacted presentation; approval/execution retain raw commands | `just test -p codex-app-server-protocol` (262 passed), two focused public v2 app-server tests, scoped fixes, `just fmt` | Complete |
| 1J | `2994f545` (#37132) | Local login-method and ChatGPT-workspace restrictions reject unverifiable PATs before network access, validate refreshed identity before cache/storage, gate realtime and proxy startup before credential or I/O reads, and keep remote cloud-config ownership on app-server | `just test -p codex-config` (212 passed), `just test -p codex-login` (161 passed), `just test -p codex-cli` (329 passed), `just test -p codex-tui` (1,441 passed; 2 snapshots reviewed), focused `codex-core` realtime policy test (1 passed); full core run compiled and reached 2,618 passed before baseline helper/timeouts (`test_stdio_server` missing; 35 failed, 25 timed out, 18 skipped, 354 not run after interrupt), scoped fixes, `just fmt` | Complete |
| 2A | `bb24b67d` (#35049), `fe614a63` (#38336) | Adds only a disabled under-development feature and empty focused extension crate; no decision authority, sampler, or TUI coupling | `just test -p codex-features` (56 passed), crate check, Bazel target build, lock/schema regeneration, scoped fixes, `just fmt` | Scaffold complete; authoritative bounded review path pending |

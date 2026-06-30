# Better Codex Implementation Plan

This is the full base implement plan for better-codex. Items can only be marked complete if *every part* of that item is completed.
Partially completed items cannot be marked complete.

## Design Ideology

Better Codex should feel like a modern, sleek, clean terminal application in the spirit of OpenCode and btop: dense, fast to scan, and visually deliberate without feeling decorative. The app should use a Catppuccin Mocha-inspired color system, with a colored background and flat panes laid directly on top of it.

The main conversation is the product's center of gravity. Anything that is not part of the conversation itself belongs in the right dashboard, which should take up roughly 30% of the horizontal space: status, navigation, settings, workspace state, approvals, tools, plans, model controls, session controls, and background activity. The conversation surface should stay focused on user turns, assistant turns, and structured transcript blocks.

Panes should be borderless, non-rounded, differently-colored rectangles. Avoid boxed widgets, rounded-card metaphors, and heavy border lines. Use color, spacing, alignment, section headers, and contrast to make regions legible while keeping the surface flat.

## Stage 1: Standalone App Shell

- [x] Replace the inherited inline chat-first experience with a fullscreen terminal app entry point.
- [x] Keep Codex's agent harness behind the app-server protocol boundary.
- [x] Support the smallest useful conversation loop: start or resume a thread, submit text turns, stream assistant text, and exit back to the terminal.
- [x] Add a right dashboard that owns every non-conversation surface, takes roughly 30% of the horizontal space, and starts with session status, model, thread id, token totals, and stage markers.
- [x] Reject unsupported interactive requests clearly so the backend does not hang while approval UI is still missing.

## Stage 2: Conversation Surface

- [x] Build a dedicated transcript model for the app shell instead of rendering directly from streaming events.
- [x] Render completed thread history on resume and fork, including user messages, assistant messages, command summaries, plans, and errors.
- [x] Add scrolling, selection, copy mode, and keyboard navigation for transcript items.
- [x] Replace raw text wrapping with styled markdown rendering that can preserve code blocks, tables, file links, and streamed markdown safely.
- [x] Add snapshot coverage for the main transcript states and narrow terminal widths.

## Stage 3: Composer

- [x] Add a multiline composer with cursor movement, history recall, paste handling, and clear visual focus states.
- [x] Support attachments and mentions through compact picker overlays.
- [x] Add command palette entries for common app actions such as model switching, permissions, resume, fork, clear, and compact.
- [x] Add interruption and mid-turn steering controls.
- [x] Persist draft input per thread.

## Stage 4: Approval And Tool UI

- [x] Implement approval dialogs for command execution, file changes, permission escalation, MCP elicitation, and tool user-input requests.
- [x] Show command output, file patches, and tool progress as structured transcript blocks.
- [x] Add keyboard-first approve, deny, edit, and explain flows.
- [x] Preserve a clear audit trail in the transcript after each decision.
- [x] Add integration tests for approval request handling through app-server events.

## Stage 5: Dashboard

- [x] Replace the placeholder dashboard with borderless, flat live panels for token usage, context-window pressure, rate limits, active turn status, and model/service tier.
- [x] Add plan progress rendering from plan events and goal state.
- [x] Add workspace status: current branch, dirty files, changed files by type, current cwd, and selected writable roots.
- [x] Add tool, approval, background task, and subagent activity panels so non-conversation state does not leak into the main conversation surface except as transcript artifacts.
- [x] Make dashboard sections responsive so narrow terminals collapse to tabs or an overlay.

## Stage 6: Session Navigation

- [x] Add an app-native thread list with search, archive/unarchive, delete, resume, fork, and rename inside the dashboard/navigation surface.
- [x] Add dashboard-native navigation for sessions, workspace, settings, and help instead of a separate left rail.
- [x] Support subagent navigation as first-class app state instead of a chat command.
- [x] Add persistent route state so the app can reopen to the last active view.

## Stage 7: Settings And Onboarding

- [x] Rebuild login, trust-directory, model migration, theme, permissions, MCP, plugin, and external-agent migration flows in the new app architecture.
  - [x] Move the login/auth selection flow, including ChatGPT device-code login and API key entry, into the app shell instead of running the inherited onboarding screen before the shell starts.
  - [x] Add an app-shell-native trust-directory startup flow with trust, continue-untrusted, and exit choices.
  - [x] Persist app-shell trust-directory decisions through the app-server config write path and reload config after a persisted decision.
  - [x] Replace the inherited model migration prompt with an app-shell-native migration surface and route accepted/declined decisions through the app-server config helpers.
  - [x] Add app-shell settings controls for model, reasoning effort, service tier, approval policy, theme, animations, and tooltips.
  - [x] Validate editable app-shell settings values and show inline feedback for invalid values, including unknown syntax themes.
  - [x] Persist app-shell model, reasoning, service tier, approval policy, theme, animations, and tooltip changes through app-server-backed config writes.
  - [x] Add app-shell integration settings rows that refresh MCP server and plugin inventory through app-server APIs.
  - [x] Add app-shell MCP management flows beyond inventory refresh, including auth/login actions and add, edit, disable, or remove server actions as applicable.
  - [x] Add app-shell plugin management flows beyond inventory refresh, including browse, install, enable, disable, update, and auth-required actions as applicable.
  - [x] Replace the inherited external-agent migration picker/import flow with an app-shell-native flow for detecting, selecting, importing, and reporting Claude Code migration items.
  - [x] Add app-shell snapshot and interaction coverage for each rebuilt flow, then remove the old flow reachability from startup and command handling.
- [x] Keep configuration writes behind existing app-server/config helpers.
- [x] Add settings pages with editable fields and validation feedback.
- [x] Add first-run and unsafe-workspace flows that feel native to the app shell.

## Stage 8: Visual System

- [x] Define a compact Catppuccin Mocha-inspired design system for terminal surfaces: flat panes, tables, tabs, lists, dialogs, badges, progress indicators, and key hints.
- [x] Replace bordered/rounded panel styling with borderless, non-rounded, differently-colored rectangular regions that sit flat on the background.
- [x] Use the existing TUI style rules within the Catppuccin Mocha palette: default foreground, cyan for focus/status, green for success, red for failures, magenta for Codex.
- [x] Add layout regression snapshots for desktop-sized, narrow, and short terminal viewports.
- [x] Audit modules so new UI code stays split by feature and does not grow central orchestration files.

## Stage 9: Hardening

- [x] Add app-shell integration coverage for start, resume, fork, turn submit, streaming, approval, interruption, and shutdown.
- [ ] Validate Linux, macOS, and Windows behavior, including alternate screen restoration after panic or fatal backend disconnect.
  - [x] Restore terminal modes and leave alternate screen from the TUI drop path, explicit exit path, and terminal restore guard.
  - [x] Install a terminal-restoring panic hook before forwarding to the previous panic hook.
  - [x] Add unit coverage for panic-hook restore ordering and the alternate-screen leave sequence.
  - [x] Surface app-server disconnection in the app shell as terminal status/transcript state and return a fatal exit when the backend event stream ends.
  - [x] Add an end-to-end terminal/PTY regression that proves alternate screen, raw mode, cursor, mouse, focus, and paste modes are restored after a panic.
  - [x] Add an end-to-end terminal/PTY regression that proves alternate screen and terminal modes are restored after a fatal backend disconnect.
  - [ ] Run and record validation for the app shell on Linux, macOS, and Windows, including inline mode and alternate-screen mode.
  - [ ] Confirm Windows virtual-terminal handling, input-buffer cleanup, and color probing still leave the terminal usable after fatal exits.
- [x] Add performance checks for large transcripts and long streaming turns.
- [ ] Remove inherited upstream UI paths once the new app reaches feature parity for daily development.
  - [x] Route the normal TUI launch path into the new app-shell run loop instead of the inherited `App::run` chat UI.
  - [ ] Remove the inherited `App` and `ChatWidget` runtime implementation once no remaining launch, startup, or command path depends on it.
  - [x] Port or replace remaining inherited pre-shell UI flows, including login onboarding, model migration, external-agent migration, and startup hook review surfaces.
  - [ ] Remove obsolete inherited slash-command, bottom-pane, history-cell, and transcript-rendering surfaces after equivalent app-shell behavior exists.
  - [ ] Migrate relevant tests and snapshots from legacy chat UI modules to app-shell coverage, then delete snapshots for removed surfaces.
  - [ ] Prune unused modules, public exports, and dependencies left behind by the inherited UI removal.
  - [ ] Verify resume, fork, settings, approvals, MCP/plugin interactions, and session navigation still work after legacy UI removal.

## Live User Input

Agents may continuously work through this plan until every unchecked item is complete. While an agent loop is running, the user may run the program and update this section with fixes, regressions, or feature requests discovered during live use. Treat these entries as user-supplied implementation tasks: triage them against the staged plan, keep them as checkboxes, and mark them complete only after the requested behavior has been implemented and verified.

- [x] Truncate outputs to 4 lines when they go over that limit (not by character count).
- [x] Fix error where rectangle boxes for tool calls and outputs break with random floating letters, the boxes protruding outside of the conversation box, etc.
- [ ] Fix context decreasing way too fast; better-codex's context decreases drastically faster (decreases to around 46% even with a simple summarization request).

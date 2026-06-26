# Better Codex Implementation Plan

## Stage 1: Standalone App Shell

- Replace the inherited inline chat-first experience with a fullscreen terminal app entry point.
- Keep Codex's agent harness behind the app-server protocol boundary.
- Support the smallest useful conversation loop: start or resume a thread, submit text turns, stream assistant text, and exit back to the terminal.
- Add a simple right dashboard with session status, model, thread id, token totals, and stage markers.
- Reject unsupported interactive requests clearly so the backend does not hang while approval UI is still missing.

## Stage 2: Conversation Surface

- Build a dedicated transcript model for the app shell instead of rendering directly from streaming events.
- Render completed thread history on resume and fork, including user messages, assistant messages, command summaries, plans, and errors.
- Add scrolling, selection, copy mode, and keyboard navigation for transcript items.
- Replace raw text wrapping with styled markdown rendering that can preserve code blocks, tables, file links, and streamed markdown safely.
- Add snapshot coverage for the main transcript states and narrow terminal widths.

## Stage 3: Composer

- Add a multiline composer with cursor movement, history recall, paste handling, and clear visual focus states.
- Support attachments and mentions through compact picker overlays.
- Add command palette entries for common app actions such as model switching, permissions, resume, fork, clear, and compact.
- Add interruption and mid-turn steering controls.
- Persist draft input per thread.

## Stage 4: Approval And Tool UI

- Implement approval dialogs for command execution, file changes, permission escalation, MCP elicitation, and tool user-input requests.
- Show command output, file patches, and tool progress as structured transcript blocks.
- Add keyboard-first approve, deny, edit, and explain flows.
- Preserve a clear audit trail in the transcript after each decision.
- Add integration tests for approval request handling through app-server events.

## Stage 5: Dashboard

- Replace the placeholder dashboard with live panels for token usage, context-window pressure, rate limits, active turn status, and model/service tier.
- Add plan progress rendering from plan events and goal state.
- Add workspace status: current branch, dirty files, changed files by type, current cwd, and selected writable roots.
- Add background task and subagent activity panels.
- Make dashboard sections responsive so narrow terminals collapse to tabs or an overlay.

## Stage 6: Session Navigation

- Add an app-native thread list with search, archive/unarchive, delete, resume, fork, and rename.
- Add a left navigation rail for sessions, workspace, settings, and help.
- Support subagent navigation as first-class app state instead of a chat command.
- Add persistent route state so the app can reopen to the last active view.

## Stage 7: Settings And Onboarding

- Rebuild login, trust-directory, model migration, theme, permissions, MCP, plugin, and external-agent migration flows in the new app architecture.
- Keep configuration writes behind existing app-server/config helpers.
- Add settings pages with editable fields and validation feedback.
- Add first-run and unsafe-workspace flows that feel native to the app shell.

## Stage 8: Visual System

- Define a compact design system for terminal surfaces: panels, tables, tabs, lists, dialogs, badges, progress indicators, and key hints.
- Use the existing TUI style rules: default foreground, cyan for focus/status, green for success, red for failures, magenta for Codex.
- Add layout regression snapshots for desktop-sized, narrow, and short terminal viewports.
- Audit modules so new UI code stays split by feature and does not grow central orchestration files.

## Stage 9: Hardening

- Add app-shell integration coverage for start, resume, fork, turn submit, streaming, approval, interruption, and shutdown.
- Validate Linux, macOS, and Windows behavior, including alternate screen restoration after panic or fatal backend disconnect.
- Add performance checks for large transcripts and long streaming turns.
- Remove inherited upstream UI paths once the new app reaches feature parity for daily development.

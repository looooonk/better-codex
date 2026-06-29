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

- [ ] Add an app-native thread list with search, archive/unarchive, delete, resume, fork, and rename inside the dashboard/navigation surface.
- [x] Add dashboard-native navigation for sessions, workspace, settings, and help instead of a separate left rail.
- [x] Support subagent navigation as first-class app state instead of a chat command.
- [x] Add persistent route state so the app can reopen to the last active view.

## Stage 7: Settings And Onboarding

- [ ] Rebuild login, trust-directory, model migration, theme, permissions, MCP, plugin, and external-agent migration flows in the new app architecture.
- [x] Keep configuration writes behind existing app-server/config helpers.
- [ ] Add settings pages with editable fields and validation feedback.
- [ ] Add first-run and unsafe-workspace flows that feel native to the app shell.

## Stage 8: Visual System

- [x] Define a compact Catppuccin Mocha-inspired design system for terminal surfaces: flat panes, tables, tabs, lists, dialogs, badges, progress indicators, and key hints.
- [x] Replace bordered/rounded panel styling with borderless, non-rounded, differently-colored rectangular regions that sit flat on the background.
- [x] Use the existing TUI style rules within the Catppuccin Mocha palette: default foreground, cyan for focus/status, green for success, red for failures, magenta for Codex.
- [x] Add layout regression snapshots for desktop-sized, narrow, and short terminal viewports.
- [x] Audit modules so new UI code stays split by feature and does not grow central orchestration files.

## Stage 9: Hardening

- [x] Add app-shell integration coverage for start, resume, fork, turn submit, streaming, approval, interruption, and shutdown.
- [ ] Validate Linux, macOS, and Windows behavior, including alternate screen restoration after panic or fatal backend disconnect.
- [x] Add performance checks for large transcripts and long streaming turns.
- [ ] Remove inherited upstream UI paths once the new app reaches feature parity for daily development.

## Live User Input

Agents may continuously work through this plan until every unchecked item is complete. While an agent loop is running, the user may run the program and update this section with fixes, regressions, or feature requests discovered during live use. Treat these entries as user-supplied implementation tasks: triage them against the staged plan, keep them as checkboxes, and mark them complete only after the requested behavior has been implemented and verified.

- [x] Numbers in the dashboard like token count or time should be rendered with commas as delimiters (i.e. 1,000,000 over 1000000).
- [x] Scrolling does not work for the conversation log; scrolling just acts identical to the up and down arrows where it selects previous messages.
- [x] Display a narrow scrollbar to the right-side of the conversation log that shows at which point of the conversation log the user has scrolled to, instead of showing how many lines the user is above the bottom (remove that feature). The scroll bar should get shorter as the conversation length increases, but should have a minimum height.

# Better Codex To-Do List

The following is a list of changes that should be applied to better-codex soon.

- [x] Remove "root" from the list of subagents, as it is just the main agent in the "CONVERSATION" pane.
- [x] Display agents with their human-readable nickname, not the file-path--like name. However, maintain nesting behavior.
- [x] De-clutter subagent conversation panes to show less of raw tool and edit output, and make them more formatted to be digestible to the end user. For instance, the "Source" field or the "Persisted Details" are usually not needed to be displayed in their raw form.
- [x] Stop displaying the original prompt the user gave to the root Codex in the subagent panes. The user already knows that it gave the prompt. The subagent logs should simply show subagent activity that do not show up in the main "CONVERSATION" pane.

The following is a list of items that better-codex should support down the road, but not immediately.

- [ ] Support a "true-yolo" mode, where every single possible disruption to a turn is automted. This includes security checks, model API limitations due to unavailability, etc.
- [ ] Support a variety of preset themes, such as tokyo night, catppuccin, dracula, gruvbox, etc, but also allow the user to port their own themes using a config file.
- [ ] Support better-codex unique features, such as customizable system prompts, agentic behavior, etc.
- [ ] Support TUI adjustments of Codex configurations, such as subagent count, access limitations, etc.
- [ ] Add support to run multiple Codex instances in the same better-codex TUI pane using tab switching and multiplexing.

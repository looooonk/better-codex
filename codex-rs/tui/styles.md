# Better Codex TUI style

Better Codex supports themed chrome in the standalone app shell and its onboarding screens. Keep
color values in `src/app_theme.rs`; components should consume semantic roles instead of embedding
RGB values. Tokyo Night remains the default. Users can select `tokyo-night`, `gruvbox-dark`, or
`catppuccin-mocha` through `tui.app_theme`. This is a terminal-client preference and must be
persisted to the local user config even when the shell is connected to a remote app server.
Project-local config cannot set `tui.app_theme`, so opening a repository cannot replace the
client's selected appearance.

## Palette

| Role | Tokyo Night | Gruvbox Dark | Catppuccin Mocha | Use |
| --- | --- | --- | --- | --- |
| Base | `#1a1b26` | `#282828` | `#1e1e2e` | Conversation and primary workspace background |
| Dark | `#16161e` | `#1d2021` | `#181825` | Header, sidebar, and recessed chrome |
| Surface | `#24283b` | `#3c3836` | `#313244` | Composer, cards, and secondary panes |
| Elevated | `#292e42` | `#504945` | `#45475a` | Selected rows, menus, and modal surfaces |
| Diff addition | `#212922` | `#30381f` | `#26352f` | Added-line backgrounds in diff panes |
| Diff removal | `#3c170f` | `#442b24` | `#3d252f` | Removed-line backgrounds in diff panes |
| Border | `#414868` | `#665c54` | `#585b70` | Dividers, inactive outlines, and scroll tracks |
| Text | `#c0caf5` | `#ebdbb2` | `#cdd6f4` | Primary text on application-owned backgrounds |
| Muted | `#565f89` | `#a89984` | `#7f849c` | Metadata, placeholders, and secondary hints |
| Focus | `#7aa2f7` | `#83a598` | `#89b4fa` | Focus rings, active tabs, and primary actions |
| Cyan | `#7dcfff` | `#8ec07c` | `#89dceb` | Links, keyboard hints, and interactive accents |
| Purple | `#bb9af7` | `#d3869b` | `#cba6f7` | Codex identity, models, and agent accents |
| Success | `#9ece6a` | `#b8bb26` | `#a6e3a1` | Completion and additions |
| Warning | `#e0af68` | `#fabd2f` | `#f9e2af` | Pending work and caution states |
| Error | `#f7768e` | `#fb4934` | `#f38ba8` | Failures, denials, and deletions |

The application chrome palette is separate from `tui.theme`, which controls syntax highlighting.
Inherited pre-shell selection prompts retain their existing adaptive terminal styling until they
are migrated into the standalone app architecture.

## Surfaces and hierarchy

- Use Base for the main canvas, Dark for persistent navigation, Surface for contained content,
  and Elevated for transient or selected content.
- Prefer spacing and background changes over dense box borders. When a border helps, use a single
  line in Border and reserve Focus blue for the currently focused control.
- Titles are bold primary text. Supporting text and inactive controls use Muted.
- Selected rows need a visible background plus a marker or bold label. Hover, focus, and selection
  must remain distinguishable.
- Buttons and tabs use short labels. Primary actions use Focus blue; destructive actions use Error.

## Content and status

- User and assistant content use Text unless a semantic accent adds useful structure.
- Codex and subagent identity uses Purple; links and interactive affordances use Cyan.
- Use Success, Warning, and Error only for their semantic states. Do not use color as the only
  status signal; pair it with a word, icon, or shape.
- Keep keyboard hints visually quiet. Highlight the key separately when practical and keep the
  action description muted.
- Never hardcode white or black foregrounds. Use Text on application-owned backgrounds and the
  terminal default only when rendering content outside that chrome.

## Accessibility and terminal behavior

- Every interaction available to the mouse must have a visible keyboard path, and focused controls
  must be identifiable without relying on hue alone.
- Assume colors may be quantized by the terminal. Layout, labels, markers, and emphasis must still
  communicate state when accent colors become similar.
- Avoid low-contrast text created by combining Muted with dim terminal attributes. Use the Muted
  color directly.
- Keep dense views readable at narrow widths and never rely on decorative glyphs for meaning.
- Follow file-local Ratatui conventions, but use the shared design helpers whenever a semantic
  chrome role exists.

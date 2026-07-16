# Better Codex TUI style

Better Codex uses Tokyo Night for its full-screen application chrome. Keep color values in
`src/app_shell/design.rs`; components should consume semantic roles instead of embedding RGB
values.

## Palette

| Role | Color | Use |
| --- | --- | --- |
| Base | `#1a1b26` | Conversation and primary workspace background |
| Dark | `#16161e` | Header, sidebar, and recessed chrome |
| Surface | `#24283b` | Composer, cards, and secondary panes |
| Elevated | `#292e42` | Selected rows, menus, and modal surfaces |
| Border | `#414868` | Dividers, inactive outlines, and scroll tracks |
| Text | `#c0caf5` | Primary text on application-owned backgrounds |
| Muted | `#565f89` | Metadata, placeholders, and secondary hints |
| Focus blue | `#7aa2f7` | Focus rings, active tabs, and primary actions |
| Cyan | `#7dcfff` | Links, keyboard hints, and interactive accents |
| Purple | `#bb9af7` | Codex identity, models, and agent accents |
| Success | `#9ece6a` | Completion and additions |
| Warning | `#e0af68` | Pending work and caution states |
| Error | `#f7768e` | Failures, denials, and deletions |

The shell chrome palette is separate from `tui.theme`, which controls syntax highlighting.

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

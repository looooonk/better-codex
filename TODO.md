# User-Defined Task Log

This file tracks implementation tasks explicitly requested by users during live use. Add each task as a checkbox with a concise description of about two sentences, and mark it complete only after the requested behavior has been implemented and verified.

- [ ] Add feature to queue messages by pressing TAB when a turn is running, and edit queued message by pressing OPTION (ALT) + UPARROW.
  - Must support feature to queue multiple messages. The user should be able to traverse these messages through OPTION (ALT) + DOWNARROW. Editing a message keeps its position in the queue.
- [ ] AUDIT messages must have exactly 1 space before and after them. Beware double spacing errors. Consecutive AUDIT messages must also be spaced.
- [ ] Reduce the frequency of the "skipped {} best-effort backend events" system messages, such that the user now rarely sees them. This phenomenon occurs when running just test -p codex-tui frequently.
- [ ] Spinner keeps saying "retrying" even after the retry succeeds (i.e. Codex starts responding again), should revert to "thinking" ideally.
- [ ] Remove top-side dashboard entirely when the terminal has limited horizontal space. Instead, just allow the dashboard to overlay on the conversation log, since the user can use CTRL + D to display or hide it. If the horizontal space is too narrow to even display the dashboard or the conversation log, put up a very simple window saying to open the terminal in a larger window.
  - To be more specific, allow the dashboard to "push" the conversation pane more narrow if there is adequate space. If there is not, then allow the dashboard to overlay the conversation log. If there is not enough space even for the overlaid version, then simply refuse to run better-codex by asking the user to open the terminal in a larger window.
  - The minimum width is up to the agent to determine.
- [ ] Add a clickable button to toggle the dashboard on the top bar (where the "BETTER CODEX" text is). This should act as the mouse-control alternative to CTRL + D.
- [ ] Allow scrolling on the dashboard if it overflows. Check mouse position to check if scrolling should scroll dashboard or the conversation log. Scrolling on the dashboard should scroll the entire pane, excluding the "Status | Agents | Sessions | Help" tab picker, which should remain fixed.
  - For tabs in the dashboard with scrollable elements, disambiguate whether the user wants to scroll the dashboard or scroll elements inside a list (e.g. list of agents, sessions, settings, etc.) by checking their mouse position.
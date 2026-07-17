# User-Defined Task Log

This file tracks implementation tasks explicitly requested by users during live use. Add each task as a checkbox with a concise description of about two sentences, and mark it complete only after the requested behavior has been implemented and verified.

- [x] Add feature to queue messages by pressing TAB when a turn is running, and edit queued message by pressing OPTION (ALT) + UPARROW.
  - Must support feature to queue multiple messages. The user should be able to traverse these messages through OPTION (ALT) + DOWNARROW. Editing a message keeps its position in the queue.
- [x] AUDIT messages must have exactly 1 space before and after them. Beware double spacing errors. Consecutive AUDIT messages must also be spaced.
- [x] Reduce the frequency of the "skipped {} best-effort backend events" system messages, such that the user now rarely sees them. This phenomenon occurs when running just test -p codex-tui frequently.
- [x] Spinner keeps saying "retrying" even after the retry succeeds (i.e. Codex starts responding again), should revert to "thinking" ideally.
- [x] Remove top-side dashboard entirely when the terminal has limited horizontal space. Instead, just allow the dashboard to overlay on the conversation log, since the user can use CTRL + D to display or hide it. If the horizontal space is too narrow to even display the dashboard or the conversation log, put up a very simple window saying to open the terminal in a larger window.
  - To be more specific, allow the dashboard to "push" the conversation pane more narrow if there is adequate space. If there is not, then allow the dashboard to overlay the conversation log. If there is not enough space even for the overlaid version, then simply refuse to run better-codex by asking the user to open the terminal in a larger window.
  - The minimum width is up to the agent to determine.
- [x] Add a clickable button to toggle the dashboard on the top bar (where the "BETTER CODEX" text is). This should act as the mouse-control alternative to CTRL + D.
- [x] Allow scrolling on the dashboard if it overflows. Check mouse position to check if scrolling should scroll dashboard or the conversation log. Scrolling on the dashboard should scroll the entire pane, excluding the "Status | Agents | Sessions | Help" tab picker, which should remain fixed.
  - For tabs in the dashboard with scrollable elements, disambiguate whether the user wants to scroll the dashboard or scroll elements inside a list (e.g. list of agents, sessions, settings, etc.) by checking their mouse position.
- [x] Implement the feature to scroll the file list on the git diff pane. Currently, if there is a diff that changes many files, you can only access all of them with the arrow key. Instead, there should also be the option to scroll files by scrolling with the cursor on the file selector pane.
- [x] Add green/red line background highlighting for the diff lines in the git diff pane, like GitHub does.
- [x] The cursor in the "MESSAGE" pane can sometimes go outside the pane if a row is filled with characters, and the next character is a space, which should not be the case. Instead, the space should overflow to the next row.
- [ ] If a long message is pasted into the "MESSAGE" pane, such that there is a long line that overflows to multiple rows, the last row may not be visible even when scrolling all the way down. This needs to be fixed, so all parts of the pasted text can be visible through scrolling / arrow key navigation.
- [ ] Refactor the MacOS keyboard shortcuts for the "MESSAGE" pane and any text input pane to the following:
  - `CMD + LEFT`, `CMD + RIGHT`: Move the cursor to the start or end of the current line.
  - `CMD + BACKSPACE`: Delete from the cursor to the start of the line.
  - `OPTION + LEFT`, `OPTION + RIGHT`: Move the cursor to the previous or next word.
  - `OPTION + BACKSPACE`: Delete the previous word.
  - `CTRL + LEFT`, `CTRL + RIGHT`: Move the cursor to the previous or next word.
  - `CTRL + BACKSPACE`: Delete the previous word.
  - `FN`: No functionality.

# Desktop window permissions acceptance

**Status: Partial.** Runtime tests keep the app command permission equal to the registered handlers and reject window creation grants; a frontend test keeps the Focus overlay command set equal to what overlay code reaches. The cases below confirm that real windows still work under the per-window capabilities described in [Application security boundaries](../data/security/application-boundaries.md).

Run the desktop app with a development vault and record the operating system, display server or compositor, and result.

- Detach a Calendar, Projects, Notes, and Chat tab into its own window by menu and by dragging out. Each window opens at the drop point or centered, uses the theme background, and loads its view, edits, and saves normally.
- Detach a tab from a detached window, reattach tabs, close windows, and reopen a saved tab window. Focus moves to an already open window instead of creating a duplicate.
- Change zoom, minimize, maximize, drag, and resize a detached window.
- Start a Focus run that shows the overlay and one that shows the blocker on every monitor. Both render, react to pause, resume, skip, idle, and dismiss, and close when the phase ends.
- Open a Chat preview and confirm the previewed page cannot call application commands (the developer console reports an ACL error).
- On Windows, preview a local page containing `<iframe src="http://tauri.localhost/">` and confirm the frame shows a blocked-content error instead of the app, and that opening `http://tauri.localhost/` directly in the preview is refused.
- On Linux and Windows, in a thread of working folder A, preview a local page that sets a cookie, a `localStorage` key, and a service worker, and log in to an external site. In a thread of working folder B on the same port, none of them is present. Restart the app: all of them are still present for A. The login does not appear in the main window, a detached window, or the Music YouTube embed.
- Unbind folder A and bind it again, or use Clear preview data in Chat permission settings: A's preview starts logged out with no storage or service worker, an open preview tab keeps working, and the folder's old directory under `chat-previews` in the app config directory is gone (on Windows it may remain until the next app start, which removes it).

# Workspace tabs

Status: Partial. Desktop kind and project tabs, with reordering, persistence, shortcuts, closed-tab history, opening projects in a new tab, and moving a tab into its own window, are implemented. Item tabs, locking, and the mobile tab bar are in progress.

## Model

A tab is a location, not a fixed app section. Its level says how specific it is:

- Kind: Calendar, Projects, Notes, or Chat, wherever the user last was inside it.
- Project: one project inside a kind.
- Item: one project view, note, Notes folder, or Chat channel.

Tabs follow navigation: moving around inside a tab updates where it points, and the tab restores that place when it is selected again. Opening something that already has a tab focuses that tab instead of adding a duplicate. A new tab opens only on explicit intent (the + launcher, Ctrl+T, or an "Open in new tab" gesture), so ordinary navigation never multiplies tabs. Calendar has no projects or items, so it holds at most one tab.

In a project navigator, a middle click or Ctrl+click opens the project in a new tab behind the current one, and adding Shift brings the new tab to the front. Several background opens from the same tab line up in the order they were opened, as in a browser.

A fresh vault opens with Calendar and Projects. Closing the last tab asks to close the app, the same confirmation as closing the window, so the app is never left without a tab.

Tabs, their order, the active tab, and the recently closed list persist per device and per vault. They are device-local interface state, not vault data, because another device may have a different window size and different working context.

## Title bar

The + after the last tab opens a launcher with the kinds, the recently closed tabs, and the open tabs when the strip cannot show them all. Tabs can be dragged to reorder. When space runs short the strip drops labels first, then shows only the active tab; the launcher keeps every tab reachable.

A tab's context menu closes it, closes the other tabs, reopens a closed tab, and moves it into its own window. A tab can move to its own window only while it is the sole tab of its kind and another tab stays in the main window, because each kind owns one window. Closing that window closes the tab, which then appears in the recently closed list.

## Shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl+T | Open the tab launcher |
| Ctrl+W | Close the active tab |
| Ctrl+Shift+T | Reopen the most recently closed tab |
| Ctrl+Shift+W | Close the app |
| Ctrl+Tab, Ctrl+Shift+Tab | Next or previous tab |
| Ctrl+PageDown, Ctrl+PageUp | Next or previous tab |
| Alt+1 to Alt+8 | Select a tab by position |
| Alt+9 | Select the last tab |

Ctrl+Shift+T follows the browser convention for reopening a tab, so the theme picker uses Ctrl+Shift+O.

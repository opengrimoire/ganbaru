# Workspace tabs

Status: Partial. Kind, project, and item tabs, with reordering, persistence, shortcuts, closed-tab history, opening projects and items in a new tab, locking, and moving a tab into its own window, are implemented on desktop, and phones show the same tabs in their bottom bar. Notes folder tabs are planned, and opening projects and items in a new tab is desktop only for now.

## Model

A tab is a location, not a fixed app section. Its level says how specific it is:

- Kind: Calendar, Projects, Notes, or Chat, wherever the user last was inside it.
- Project: one project inside a kind.
- Item: one project view, note, or Chat channel. Notes folder items are planned.

Tabs follow navigation: moving around inside a tab updates where it points, and the tab restores that place when it is selected again. Opening something that already has a tab focuses that tab instead of adding a duplicate. A new tab opens only on explicit intent (the + launcher, Ctrl+T, or an "Open in new tab" gesture), so ordinary navigation never multiplies tabs. Calendar has no projects or items, so it holds at most one tab.

On a project, project view, note, or Chat channel row, a middle click or Ctrl+click opens it in a new tab behind the current one, and adding Shift brings the new tab to the front. Row menus that have one also offer "Open in new tab". Several background opens from the same tab line up in the order they were opened, as in a browser.

A kind tab shows the kind's name. Project tabs show the project name, and item tabs show the view, note, or channel name, kept current across renames.

A fresh vault opens with Calendar and Projects. Closing the last tab asks to close the app, the same confirmation as closing the window, so the app is never left without a tab.

Tabs, their order, the active tab, and the recently closed list persist per device and per vault. They are device-local interface state, not vault data, because another device may have a different window size and different working context.

## Locking

A locked tab stays on its project or item. Navigating somewhere outside it inside the tab opens that place in another tab, or focuses the tab that already shows it, and the locked tab keeps its place. Locking a kind tab narrows it to the project it shows, or to its item when there is no project, since a tab locked to a whole kind would restrict nothing. A lock whose project or item no longer exists is dropped when the tab is restored.

## Title bar

The + after the last tab opens a launcher with the kinds, the recently closed tabs, and the open tabs when the strip cannot show them all. Tabs can be dragged to reorder. When space runs short the strip drops labels first, then shows only the active tab; the launcher keeps every tab reachable.

A tab's context menu closes it, closes the other tabs, reopens a closed tab, locks or unlocks it, and moves it into its own window. A locked tab shows a lock after its label. A tab can move to its own window only while it is the sole tab of its kind and another tab stays in the main window, because each kind owns one window. Closing that window closes the tab, which then appears in the recently closed list.

## Phone

The bottom bar, or the rail on larger windows, shows the first four tabs and a tabs button with the open count. When the active tab sits past them it takes the last slot, so the bar always shows where the user is. The tabs button opens a sheet that switches, reorders, locks, and closes tabs, opens a new tab of any kind, and reopens recently closed tabs. The only tab has no close there, because a phone app is left with Back, not closed. Android Back first returns to the first tab, then leaves the app from it.

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

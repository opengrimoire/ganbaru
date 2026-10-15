# Workspace tabs

Status: Partial. Kind, project, and item tabs, with reordering, persistence, shortcuts, closed-tab history, opening projects and items in a new tab, locking, and moving tabs between windows from their menu, are implemented on desktop, and phones show the same tabs in their bottom bar. Notes folder tabs are planned, and opening projects and items in a new tab is desktop only for now.

## Model

A tab is a location, not a fixed app section. Its level says how specific it is:

- Kind: Calendar, Projects, Notes, or Chat, wherever the user last was inside it.
- Project: one project inside a kind.
- Item: one project view, note, or Chat channel. Notes folder items are planned.

Tabs follow navigation: moving around inside a tab updates where it points, and the tab restores that place when it is selected again. Opening something that already has a tab focuses that tab instead of adding a duplicate. A new tab opens only on explicit intent (the + launcher, Ctrl+T, or an "Open in new tab" gesture), so ordinary navigation never multiplies tabs.

On a project, project view, note, or Chat channel row, a middle click or Ctrl+click opens it in a new tab behind the current one, and adding Shift brings the new tab to the front. Row menus that have one also offer "Open in new tab". Several background opens from the same tab line up in the order they were opened, as in a browser.

Each tab shows its kind icon and one name: an item tab shows the view, note, or channel name followed by its project in parentheses, shortened when long; a kind or project tab inside a project shows just the project name; and Calendar and tabs outside a project show the kind name. The icon tells kinds apart, so the strip skips the kind name; tooltips, the launcher, and screen readers still name it. Names stay current across renames. Any kind, Calendar included, can have several tabs, so the same place can be open twice when the user asks for it.

A fresh vault opens with Calendar and Projects. Closing the last tab of a window closes that window, and in the last window open it asks to close the app, the same confirmation as closing the window, so the app is never left without a tab.

Tabs, their order, the active tab, and the recently closed list persist per device, per vault, and per window. They are device-local interface state, not vault data, because another device may have a different window size and different working context.

## Locking

A locked tab stays on its project or item. Navigating somewhere outside it inside the tab opens that place in another tab, or focuses the tab that already shows it, and the locked tab keeps its place. Locking a kind tab narrows it to the project it shows, or to its item when there is no project, since a tab locked to a whole kind would restrict nothing. A lock whose project or item no longer exists is dropped when the tab is restored.

## Title bar

The + after the last tab opens a launcher with the kinds, each opened by its number key while the launcher is open, and the open tabs when the strip cannot show them all. Closed tabs come back with Ctrl+Shift+T or the tab menu. Pressing a tab opens it, so a dragged tab is the one shown. The dragged tab itself follows the pointer along the bar and the others slide aside as it passes them; past the last tab it pushes the + ahead of it through the empty bar. Pulled off the bar, or up to the controls at its end, it floats under the pointer until dropped, and the window shows its next tab meanwhile, as it does while the tab is dragged out of the window. Bringing it back onto the bar, or cancelling the drag, puts it back in front.

Tabs size like browser tabs: each has the same fixed width and all of them shrink evenly only when the bar runs out of room, dropping the inactive close button, then the label, as they narrow. Only when they no longer fit at their narrowest does the strip show just the active tab, with the launcher keeping every tab reachable. The compact tabs setting shows icons only at any size, and a strip of empty title bar stays free for moving the window.

A tab's context menu closes it, closes the other tabs, reopens a closed tab, locks or unlocks it, and moves it into a new window. A locked tab shows a lock after its label.

## Windows

On desktop, windows are equal, as in a browser: each holds its own tabs of any kind, opens new ones, and can move a tab into a new window while it has another tab left. Like browser tabs, a tab dragged onto another window's tab strip moves there at once, before it is dropped: it opens in that window in the slot it would take, with the other tabs sliding aside, and the window it left shows its next tab. A window's last tab can move this way too; its window leaves the screen at once and closes when the drag ends (the window that runs app-wide work hides instead). A tab pulled down off the title bar, or dropped outside every window, opens in a new window at that point where the window manager allows it. A tab carries its place and lock when it moves, and pending edits are saved first so the new window shows them. Every window of a vault shares one recently closed list, which also takes the tabs of a closed window. Closing a window while others stay open closes just that window, and closing the last one asks to close the app; windows still open when the app quits reopen with their tabs on the next launch. Raising the app, for example with a second launch, brings forward a window already on screen.

The window the app starts with also hosts app-wide work such as schedulers, update checks, and Music playback. Closing it while other windows stay open hides it so that work keeps running, and it returns with fresh tabs when the app needs it again, for example to open a notification. Currently the Music panel and title bar Music controls appear only in that window.

## Phone

The bottom bar, or the rail on larger windows, shows the first four tabs and a tabs button marked with an add sign while every tab fits, or with the number of tabs past the bar. When the active tab sits past them it takes the last slot, so the bar always shows where the user is. Holding a bar tab lifts it to reorder the bar by dragging, while a tap switches to it. The tabs button toggles a panel above the bar, without a backdrop, that switches, reorders by drag, and closes tabs, leaving stepwise moves, locking, and closing other tabs to desktop; a New tab row opens a second level listing the tab kinds, so the panel stays short. Phones have no recently closed list, which suits desktop windows more than a thumb-sized panel. The only tab has no close there, because a phone app is left with Back, not closed. Android Back first returns to the first tab, then leaves the app from it.

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

# UI foundations

Status: Implemented. This document owns the shared look and behavior of panels, menus, dialogs, rows, fields, and form controls. Feature docs describe what a panel contains; this document describes how every panel is built.

## Why

Panels were restyled one at a time, so radii, shadows, text sizes, input styles, and hover rules drifted apart in small ways that add up. One set of tokens and primitives keeps the app feeling like one product, and lets a single change reach every panel. Database collection panels are the reference design.

## Where it lives

- `apps/client/src/ui-foundations.css` defines the tokens and Tailwind utilities below.
- `apps/client/src/lib/components/ui/` holds the shared components: `Checkbox`, `Switch`, `SwitchField`, `Select`, `SearchField`, `ConfirmDialog`, `ActionToast`, `Notice`, `Loading`, and the tooltip host.
- `apps/client/src/lib/components/ui/floating-width.ts` mirrors the width utilities for positioning code.
- `apps/client/src/lib/utils/scroll-edge-fade.ts` and `apps/client/src/lib/utils/menu-aim.ts` own scroll fades and hover-submenu behavior.

## Surfaces

| Utility | Use |
| --- | --- |
| `surface-floating` | Menus, popovers, pickers, hover cards, and nonmodal panels |
| `surface-dialog` | Modal dialogs and large windows such as Settings |
| `surface-content` | A tab's content surface; with overflow clipping, its content always stays inside the edge |
| `surface-content-edge` | The edge alone, laid over a content surface whose content is painted by sibling elements (Chat) |
| `surface-backdrop` | The dimmed layer behind a modal dialog |
| `surface-floating-body` | The inset between a floating surface and its rows |

- Every floating surface and dialog shares one radius, one thin border, and one barely visible shadow. Do not add `shadow-lg`, `shadow-xl`, rings, or other radii next to them.
- Rows and fields use the concentric item radius (`rounded-floating-item`), so inner corners follow the panel corner.
- Fullscreen and sheet variants on narrow or mobile layouts may drop the radius and border where they touch the screen edge.
- Floating surfaces use the `popover` color; dialogs use the `card` color. Do not add `dark:` background overrides; themes own those colors.
- The window shell has two layers: the `sidebar` color frames the app (title bar and window edge), and each tab paints one `cal-bg` content surface. A tab's toolbar row sits on that surface at `--toolbar-h` with no fill or bottom border of its own, so the header reads as the top of the page rather than a separate band. Docked desktop explorers (Projects, Notes, and Chat) belong to the frame: they take the `sidebar` color and its accent for rows, run the full height of the tab, and the `cal-bg` surface with its own toolbar sits beside them. They collapse to a narrow rail through the shared `explorer-dock` utilities, which clip fixed-width content instead of reflowing it, so all three move identically. Sheet and mobile explorers keep the content surface color. The content surface carries a hairline edge derived from the frame color, always painted as a border just inside its box so every tab's edge sits on the same pixels; a border snaps to whole device pixels, while a shadow ring changes thickness with its position at fractional display scales. The active title bar tab adds a barely visible lift. Both use fixed values, not editable tokens.
- On phones, the top bar and every tab's action row place icon buttons on one column grid: `--mobile-chrome-column` wide, with no gap and a `--mobile-chrome-inset` edge inset, and the leading column centers under the top bar pill icon. Action row icons therefore stack under the top bar icons instead of reading as a separate toolbar. Each button keeps the whole column as its touch target but paints its pressed and open fill as a round `--mobile-chrome-highlight` shape through the shared `mobile-chrome-button` utility, matching the top bar.
- Closing a panel or dialog returns focus through `captureFocusReturn` in `apps/client/src/lib/modal-focus.ts`: to the opener only when the opener had keyboard focus, otherwise to the dialog that contained the opener or nowhere. A pointer-opened panel must not leave a focused trigger, or a focused control inside a hidden panel, that Space or Enter would activate later.
- A modal panel that hosts its own controls, such as Quick notes, owns the keyboard through `activateIsolatingKeyboardLayer` in `apps/client/src/lib/modal-focus.ts`: it sees keys before the app, and while it is open, window-wide app shortcuts (tab switching, Settings) and view shortcuts gated by workspace pane focus stay inactive. Shell shortcuts such as close, zoom, theme, and the panel toggles still work. Dialogs whose keys all belong to them use `activateModalKeyboardLayer` instead.

## Rows and text

- `menu-item` is the actionable row: it carries the panel row height, padding, gap, radius, icon size, hover, keyboard focus, and disabled styling. `menu-item-destructive` marks a row that removes data.
- `menu-separator` divides groups, and `menu-label` titles a group.
- Background highlight marks only the pointer, keyboard focus, and the row whose submenu is open (`aria-expanded="true"`). The current choice shows a trailing check, never a persistent background. Menus that keep DOM focus elsewhere, such as an editor, mark the keyboard-active row with `data-highlighted`.
- Panel text uses `text-panel`, set by the surface, and secondary text (descriptions, shortcuts, counts, group labels) uses `text-panel-detail`. Avoid ad hoc rem sizes inside panels.
- A row with a secondary line (such as a title above its location) grows past the row height; mark it with `data-collection-row-two-line` inside collection panels, which otherwise pin rows to one line.
- Touch layouts raise the row height to a 44px target through `--panel-row-height`; do not add separate mobile row heights.

## Fields and controls

- `field` styles a text input, textarea, or a wrapper that holds a borderless `field-bare` input with icons. Fields are transparent with a thin border; `aria-invalid="true"` on the input marks an error border that stays while focused.
- Text entry never shows a focus contour, ring, border change, or fill; the caret is enough. The global keyboard focus outline excludes text entry for the same reason. Buttons, checkboxes, and switches keep the keyboard focus outline.
- Checkboxes use `ui/Checkbox`, a native input drawn with theme tokens. Use `indeterminate` for partly selected parents, and wrap it in a `<label>` with visible text when the label is visible.
- On and off settings use `ui/Switch` or `ui/SwitchField`. Inside floating panels the switch uses its `compact` size so it sits within the panel row. A switch that saves quickly stays enabled while saving and ignores repeated presses instead, so the cursor and colors do not flash. Choices from a list use `ui/Select`, never a native `<select>`, so menus match the rest of the app.

## Unavailable controls

- `control-unavailable` marks a control whose behavior is not implemented yet: it keeps its final appearance, shows the not-allowed cursor, carries `aria-disabled="true"`, and has no activation handler. It never dims, badges, or explains; [Contacts and invitations](../features/collaboration/README.md) records why. Do not use `disabled` or muted colors for this purpose, so unavailable controls remain distinguishable from controls that are disabled by state. `menu-item`, `field`, and `Select` (through its `unavailable` prop) keep their normal colors and hover for such controls.

## Notices

- Failures and limiting conditions use `ui/Notice`: a muted icon and plain foreground text, with retry, copy, and any extra actions as quiet trailing controls. Notices never use destructive colors, tinted boxes, or primary buttons, because a failure the user can recover from should not be louder than the content it replaces. Destructive color stays reserved for actions that remove data.
- The notice sits where the failed content would have been: `inline` inside the row, panel, or banner that owns it, `fill` centered in a failed view or pane, and `floating` as a small centered surface only when a dialog or panel with no host surface failed to open. A floating notice always offers a close button so the user can back out.
- Retry turns while the retried work is pending and shows a check when it succeeds; a handler that reports its own errors resolves `false` so a failed attempt never shows the check; copy shows a check when the message reaches the clipboard. Copy is hidden when the message is fixed text with no detail worth sharing.
- `tone="warning"` tints the icon for conditions that limit a feature without being a failure, such as an unavailable provider.

## Loading

- Pending content inside a surface whose purpose is already clear never shows visible "Loading" text. A purpose-built skeleton (such as Notes pages and database views) is preferred where the layout is known; otherwise use `ui/Loading`, which waits briefly before appearing so fast loads never flash, and announces what is loading only to assistive technology.
- `ui/Loading` uses `rows` inside lists, menus, and pickers, `fill` for a pending view or pane, and `inline` inside a line of text or a value slot.
- A standalone status line, such as a connection check among settings, uses `inline` with `showLabel` and a specific label ("Checking browser extension"), because a bare spinner there does not say what is happening. A `fill` pane with no heading or content shape to explain it, such as a starting camera view, does the same.
- A button whose action is pending keeps its label, shows a spinner in place of its icon (or before the label), and sets `aria-busy`.

## Widths

- Floating surfaces use `w-floating-sm` (compact action menus), `w-floating` (settings and option panels), or `w-floating-lg` (searchable lists, editors, and two-part rows), capped by the viewport. Content that genuinely needs another width, such as a calendar grid or an editor, may use its own.

## Placement and dismissal

- A floating surface stays inside the visible viewport. Panels positioned by script use the shared anchored helpers; panels placed by CSS beside their trigger use `keepInViewport`, which slides them back only as far as a narrow screen requires. Its `translate` makes the panel the containing block for fixed descendants, so nested fixed pickers portal out.
- Touch screens keep `:hover` on the last tapped element, so the build moves every hand-written `:hover` rule behind `@media (hover: hover)`, as Tailwind's `hover:` already is. On touch, a trigger shows its highlight only through open, active, or keyboard focus states, so it clears as soon as its surface closes.
- A second press on the trigger of an open surface only closes it. A full-screen backdrop closes on `click`, not on `pointerdown`, because a touch that closes on press lets the following click land on the trigger and reopen the surface. Outside-press handlers count the trigger as inside.

## Scrolling

- Every scrollable panel body fades the edge that has more content beyond it with `scrollEdgeFadeAction`.
- The fade is a mask, so put it on an inner scroll element, never on the bordered surface, or the border and background fade too.

## Hover submenus

- In a menu, a hovered row opens its submenu after `SUBMENU_OPEN_DELAY_MS` and a hover-opened submenu closes `SUBMENU_CLOSE_DELAY_MS` after the pointer leaves both, unless the pointer is aiming at it within `SUBMENU_AIM_TOLERANCES`.
- Cascading pickers (the Projects, Notes, and Chat navigators) switch to a hovered row immediately, or after `SUBMENU_CLOSE_DELAY_MS` while the pointer is aiming at the open side panel within `SUBMENU_AIM_TOLERANCES`. A side panel closes `SUBMENU_CLOSE_DELAY_MS` after the pointer leaves its row, bridge, and panel, and re-entering any of them cancels the close. Only the first panel of a cascading picker carries search and the order button, side by side; side panels have neither, so the panel footer keeps only creation actions. The first panel searches what its view navigates to: notes in Notes, channels in Chat, and projects elsewhere.
- Settings flyouts keep their side panel open until dismissed and switch rows with the same aim rule.
- Breadcrumb triggers that open a navigator on hover wait `SUBMENU_OPEN_DELAY_MS`, switch between sibling triggers at once while open, and close `SUBMENU_CLOSE_DELAY_MS` after the pointer leaves both the triggers and the panel. A click on a trigger, or a press or keystroke inside the panel, pins it until dismissed. Touch pointers never open on hover.
- The parent row keeps `aria-expanded="true"` and its highlight while the submenu is open. Opening one submenu closes its siblings.
- ArrowRight or Enter opens a submenu from the keyboard, and ArrowLeft or Escape returns to the parent row.

## Sortable lists

- Lists reordered in place (Project settings statuses, priorities, tags, custom fields, and options) use `createSortableDrag` from a grip handle: the dragged row lifts and follows the pointer, its siblings slide aside, and the move commits once the row settles, with no landing flash. Escape or leaving the window returns the row to its slot, and the grip keeps arrow key moves.
- Lists whose moves wait on an asynchronous store write keep native drag, since a commit after the settle would jump.
- Sidebar and picker custom orders use the same drag through `createSidebarOrderDrag`, which drags the whole row with a mouse or pen, or after a stationary touch hold so a swipe still scrolls, and updates the store synchronously before the save so the settle never jumps. A new move waits for the previous save. Row menus offer the keyboard alternative through `MoveStepMenuItems`. While a touch hold is pending or active, the long-press context menu stays closed.
- The click that follows a drop is swallowed, so releasing a lifted row never also opens it.

## Deliberate exceptions

- The Calendar event panel family (event panel, recurrence, notifications, and description editor surfaces) keeps its own shape, color, and shadow.
- A lifted sortable row takes the popover color and a stronger shadow while it follows the pointer, so it reads as picked up above its list.
- Tooltips use an inverted palette so they read as transient hints rather than panels.
- Calendar-colored pickers (icon picker, event and quick note color pickers) keep the Calendar background so they match the surface that opens them, while sharing the radius and shadow.
- The settings panels opened from the Projects, Notes, and Chat toolbars keep their own section order and layout inside shared chrome.
- Quick notes tags are created and renamed in place: the input takes the chip's shape and grows with its name instead of using a bordered `field`, so the tag row keeps its height.

New exceptions need a product reason recorded here.

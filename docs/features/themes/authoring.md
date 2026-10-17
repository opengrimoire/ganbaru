# Theme authoring

## Theme selection

Appearance settings list quick-toggle choices and all registered themes, built-ins first. The quick toggle can target custom light and dark themes instead of only the built-in pair. A keyboard-accessible theme picker previews the highlighted theme; confirming applies it and cancelling restores the previous one.

## Editor session

Opening a theme creates an in-memory authoring buffer and keeps the pre-edit snapshot. Every source, token, isolation, palette, metadata, Calendar default, reset, rebake, or direct JSON edit updates the buffer and the live preview. Direct JSON edits cannot change the theme's identity.

The real app stays visible underneath where space permits so users can test actual surfaces. Compact layouts use a sheet or full-screen editor with Save, Cancel, and scrolling always reachable.

## Save, cancel, and reset

- **Save and apply** validates and persists the whole buffer in one operation and keeps the theme active. A failed save leaves the session retryable.
- **Cancel** restores the exact pre-edit registry and active theme. A duplicate created by the session is removed.
- **Reset all to seed** replaces the buffer with the seed snapshot; it stays unsaved until Save.

Closing the application with a dirty session asks for confirmation. Because edits are never persisted before Save, discarding is always truthful.

## Organization

The editor groups source colors, app canvas, Calendar surface, event palette, Calendar details, event panel, text, actions, and semantic status colors in a stable order. Stored rows are keyed by identity, not position. Source controls explain which token families they drive, and token rows show the resolved color and isolation. Runtime-only colors never appear as editable rows.

All 32 event slots stay visible with stable identity, and editing a slot previews events that use it. Visual reordering never changes persisted slot identity.

## Contrast feedback

Warnings update live, link to the affected control where possible, and cover text and surface pairs, event labels, semantic actions, and focus visibility. Low-contrast themes can still be saved because themes are user-authored, but the risk is never hidden or labeled accessible. See [Color engine](color-engine.md#contrast).

## Accessibility

Every color field has a text value and keyboard operation. Isolation, warnings, selected state, built-in protection, and dirty state are not communicated by color alone. Live preview never steals focus from the active control.

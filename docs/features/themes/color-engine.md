# Theme color engine

The color engine derives coherent resolved tokens from a small source palette while preserving isolated user edits and stable event-slot identity.

## Source palette

Primary sources include app canvas, app ink, primary action, and semantic destructive, confirm, and warning background/text pairs. Calendar defaults derive from a selected basis:

- Light.
- Dark.
- App canvas.
- Custom Calendar basis.

The resolved Calendar canvas remains an ordinary editable token and can be isolated.

## Resolved snapshots

Source colors drive edits and rebakes. They do not participate in every paint. The registry paints the stored resolved app and Calendar snapshots, then derives runtime-only implementation colors.

This distinction makes existing themes stable across engine upgrades and allows exact import/export.

## Source-edit cascade

Changing a source recomputes the non-isolated tokens owned by that source family. Tokens in unrelated families remain unchanged. The cascade updates the in-memory authoring buffer as one coherent edit.

Semantic background/text pairs preserve their intended relationship. Derived action and status aliases cannot drift into hidden independent overrides when the editor no longer exposes such control.

## Isolation

An isolated token is user-pinned. Source edits and rebakes leave its resolved value unchanged. Turning isolation off returns the token to the next applicable source cascade; it does not immediately discard the current value unless the UI explicitly offers and explains that action.

Isolation state is part of theme persistence and export. Runtime-only colors cannot be isolated because they are not authored snapshot rows.

## Color spaces and derivation

Derivation uses perceptual operations suitable for lightness and mixing rather than raw channel addition. App-canvas Calendar defaults apply a direction-aware lightness offset so the Calendar remains distinct in both light and dark themes.

Exact constants live in source. Durable requirements are:

- Deterministic output for identical source, snapshot, isolation, and engine version.
- Valid bounded colors for every result.
- Monotonic treatment of lightness where the UI describes a lighter or darker relation.
- No mutation of isolated tokens.
- Stable semantic foreground/background pairing.

## Luminance and runtime mode

The effective app canvas determines whether the runtime uses light or dark platform treatment. Theme identity and user labels do not override actual luminance.

This decision drives native window treatment, system bar contrast, dark CSS compatibility, and controls that need a binary mode. The threshold is versioned with the engine when a change would materially alter existing themes.

## Contrast

The engine evaluates text, controls, focus rings, semantic actions, event labels, panel surfaces, and other meaningful foreground/background pairs. Warnings name the affected pair and measured problem.

Warnings do not silently change authored colors. The editor can offer a derived repair suggestion, but applying it is an explicit edit. Decorative surfaces are not presented as if they satisfy text contrast requirements.

Shared tooltips contrast with the stable containing surface, not a control's temporary hover or selection fill.

## Engine versions and rebaking

Each user theme records the engine version that produced its snapshot. When the current engine is newer, the theme continues painting from its stored snapshot and the editor offers a rebake.

Rebake derives non-isolated tokens from stored sources using the current engine. Isolated tokens, event-slot identities, user metadata, and compatible explicit Calendar choices remain unchanged. The user previews the result and must Save it.

Dismissal of an upgrade notice is separate from applying a rebake. A later engine version can present a new notice without reviving a dismissed older one.

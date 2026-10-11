# Notes links and collaboration

Status: implemented for local links, backlinks, comments, suggestions, mentions, and notifications. Shared access control and sync are planned.

## Inline links

Linked text uses semantic anchors. Hovering a link in the editor opens a small floating preview with its destination, Copy, and Edit; clicking the link opens it. Ctrl/Cmd+K or Edit opens an anchored panel with Page or URL and Link title fields, where note titles can be searched without reading database rows. Applying keeps surrounding text and formatting, and Remove link keeps the text. Link panels float outside the document layout, and invalid destinations, stale selections, and failed actions stay visible in the panel.

Pasting a single valid URL onto selected text links the existing words, including across a document selection as one undo step; code blocks and database blocks are unchanged.

Local links persist as `#notes?page=UUID` with an optional `block=UUID`; a link to a database view adds `view=UUID` beside its block and opens that database with the view selected. Links copied from the current app origin normalize to this form. Rust validates the identities before saving, and navigation checks whether the target is still available. This is not an operating-system URL scheme or a public sharing URL.

Page and database mentions render as compact inline references. Hovering shows the current title with its route above it, from project group and project through folders and parent notes; a database's route ends with the note that holds it, and its card adds a schematic preview that fetches no rows. Metadata loads only on hover intent or activation. Clicking a database mention opens its database surface. Within the vault, rich clipboard paste keeps mention identities without duplicating their destinations; missing or invalid metadata degrades to readable text or an ordinary hyperlink.

External links open through a Notes-owned native command that allows only HTTP, HTTPS, and email destinations, rejects embedded credentials, fetches nothing, and does not widen the generic opener allowlist. Local references stay within Notes navigation.

## Stable links and aliases

Local Notes links resolve to stable page identities when known. Aliases provide alternate names for pages and database rows without changing the canonical title.

Title and alias resolution can help migrate imported or typed links, but once a target is chosen the source should store stable identity. Ambiguous matches remain unresolved rather than selecting whichever title appears first.

## Backlinks

Backlinks derive from child-page blocks, page mentions, local Notes links, comments, database relations, supported local-object mentions, and alias-aware resolution. They are a rebuildable projection, not canonical relationship data.

Backlinks respect access. A page never reveals the title, count, comment, row, or project context of an inaccessible source.

## Unresolved links

Unresolved local links remain visible and indexed with their source context. Resolving one rewrites the source to a stable page ID through the normal canonical command.

Resolution validates the source still contains the expected link and the selected target is active and accessible. A stale result does not rewrite newer content.

## Link facts and graph views

Link facts derive a graph across pages, row pages, blocks, properties, comments, managed files, application objects, and external URLs. Graph and related-content views are projections and can be rebuilt from canonical data.

## Comments

Notes supports page discussions, block comment threads, and inline text-range anchors. Comments retain author snapshots, status, timestamps, attachments, emoji reactions, and soft-delete state. Comment text can mention dates, people, and pages, and can attach files. Per-user read state, muted discussions, and per-page notification levels (mentions and reminders, mentions only, or off) are local preferences rather than content.

Page discussions appear under the title and, on row pages, under the properties. Once a page has discussions, a header button summarizes the latest comment of the most recently active open discussion with the page's notification settings and a way into the All discussions panel, which filters open, resolved, or all discussions. Resolving a thread does not delete it. Inline anchors retain surrounding context so a moved or edited range can be identified or reported stale.

## Suggestions

Suggestions store original and proposed text, range and context anchors, author, version, and accepted or rejected state. Applying a suggestion checks the current block and base context. Conflicts remain visible for review rather than editing the wrong text.

## Mentions and notifications

Rich text can mention people, pages, databases, dates, projects, Project tasks, and Calendar events. Live state such as the Focus timer or the Music queue is not mentionable. Reminder and participant mentions can create local notification rows. Delivery is owned by the local application or platform scheduler and never by a hosted Notes service.

The mention catalog loads only while a mention menu is open, so opening a note never reads database rows for mentions.

Every mention is one atomic token in the editor: the caret cannot enter it, one arrow key or Shift+arrow press crosses it, one Backspace or Delete removes it whole, and formatting or partial selections widen to cover it. Its icon is decoration that follows the selection highlight but is never copied as text, while rich copy and paste keep the identity of every mention kind. Project mentions show the project's icon, project and task mentions open Projects (a task opens its project), Calendar event mentions open Calendar, and people and date mentions are not navigable. Hovering any mention with a mouse or pen opens the same kind of card: a project shows its group above its name, a task its group and project, and a Calendar event its calendar, with its date and time and location when the event is in the loaded Calendar range. A date shows the spelled date and whether its reminder is on; a person shows their name only.

The editor mention menu groups suggestions by kind (dates, people, pages, projects, Calendar), shows each page or database with its route, and collapses long groups behind a "more results" row. People always offer an invite panel with the final layout; sending invitations stays unavailable until shared access exists.

Notification previews and deep links obey the same effective page access as direct reads.

## Collaboration operations

Comment and suggestion mutations append local operation records with actor snapshots, entity versions, and explicit state transitions. Comment body edits currently use last-writer-wins metadata because there is no multi-author rich-text merge for comment bodies.

Resolve, reopen, accept, and reject operations validate base versions. Future sync must surface conflicting stale decisions rather than silently applying them.

## Access

**Planned:** shared collaboration can grant group-wide, project-wide, folder-subtree, page, comment, or task-context access. Project membership alone does not imply access to every Note. A page's Share action uses the shared participant picker from [Contacts and invitations](../collaboration/README.md) with page or subtree scope and the roles Can view, Can comment, Can edit, and Full access.

Search, backlinks, mentions, notifications, history, templates, imports, exports, Chat links, reports, and AI context packages enforce the same effective access as direct page reads.

Inviting a participant requires an explicit history-visibility choice when prior content would become readable. Revocation stops future reads, exports, notifications, key access, and context assembly without rewriting legitimate shared history.

## Sync direction

Local editing remains SQLite-canonical. Planned encrypted CRDT sync may coordinate changes over the same page and block graph, but it must preserve local canonical data, explicit access, conflict semantics, and derivative Markdown boundaries.

Exact encryption, key distribution, offline revocation, and multi-author conflict behavior belong to the [sync](../../data/sync.md) and [access-control](../../data/access-control.md) specifications.

## Daily and project notes

Project-note membership is implemented as page metadata and project-scoped navigation. Daily notes remain planned. Both are conventions over normal Notes pages, not separate Markdown folders or parallel document models.

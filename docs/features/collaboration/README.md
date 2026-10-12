# Contacts and invitations

Status: Partial. Implemented: the person identity, the signed contact card, contact requests delivered over the local LAN link, contacts with trust scopes, and blocking. Not implemented: invitations, spaces, direct conversations, contact pictures, relay delivery, consumption of once permissions, and the Contacts group in pickers. Today AI agents are still the only participants in any space. Controls whose behavior is not implemented keep their final appearance and are inert, as described in [Unavailable controls](#unavailable-controls).

This document owns the shared model for human collaboration: identity, contacts, invitations, roles, and the surfaces where people and AI agents appear. Feature documents link here instead of repeating it. Authorization rules are owned by [access control](../../data/access-control.md); delivery, encryption, and conflict handling are owned by [device linking and synchronization](../../data/sync.md).

## Why

Ganbaru AI already coordinates work between a person and configured AI agents through explicit memberships and access profiles. Human collaboration adds other people to the same model instead of a second one. One picker, one access step, and one member list serve people and agents alike, so the only visible difference between adding a colleague and adding an AI agent is that the agent also has a configurable brain and execution limits. Users who know chat and planning apps should recognize the flow immediately: pick from contacts, choose a role, done.

## Vocabulary

- **Person:** a human participant with a locally generated identity. The local person is the vault owner described in [Profile](../profile.md).
- **Contact card:** the shareable public identity of a person: public key, display name, avatar, and an optional self-hosted relay address, as a QR code or a pasteable code.
- **Contact:** a person whose contact card was accepted. Contacts are the only people who can send anything to the local person.
- **Trust scope:** what a contact may do unsolicited: invite the local person to shared spaces, or message them directly. Each permission has a duration.
- **Space:** anything a person can be invited to: a group, a project, a channel, a Calendar event, a Notes page or subtree, or a direct conversation.
- **Invitation:** a request to join one space with one role and one history boundary.
- **Role:** a named preset of capabilities for one space, or a custom set.
- **Participant:** a person or AI agent acting in a space through a membership.
- **Agent:** an AI participant owned by exactly one person. See [Agents and coordination](../chat/agents-and-coordination.md).

## Identity and contact cards

A person is a key pair generated on their own device, in the same way device linking already gives each device an identity. There is no Ganbaru account, no hosted directory, no username search, and no discovery by email or phone number. The contact card is the only entry point, and a person hands it out deliberately.

The card carries the display name and avatar color from Profile, never the full name or anything else in the vault, and is signed by the person key so a tampered card is rejected. It also carries a delivery hint: today the private LAN address and certificate fingerprint of the vault's coordinator, later a self-hosted relay address, without changing the card format. It is shown in Settings as a QR code for phones and a copyable code for desktops, the same two forms device linking uses; the card needs a complete display name before it exists. Regenerating the card invalidates the previous one for new contact requests; existing contacts are unaffected.

The person key is generated on the first device that can write the vault and follows the person to linked devices through [device linking](../../data/sync.md#contacts-and-contact-requests). The card is unavailable on a linked device until its key copy arrives.

What a contact sees of a person is limited to the display identity and to content inside spaces they share. Identity visibility follows the same boundaries as content, so a contact never learns which other spaces, contacts, or devices a person has.

## Contacts and trust

A contact request is the only unsolicited message in the system, and it can only reach a person through a card that person handed out. The recipient sees the sender's display identity and a verification code, and accepts, declines, or blocks. The verification code derives from the person key alone, so it stays the same across regenerated cards; each person sees their own code under their card, and both sides see the other's code on pending requests. Acceptance is mutual: both people become contacts of each other. A sent request stays pending for seven days while the sender's device polls the recipient's delivery hint; both sides see their pending requests in Settings, and the sender can cancel. The sender chooses the trust scope for the future contact when sending, from the defaults in Settings, and the recipient chooses theirs when accepting.

Each side then decides what the other may do without asking. The trust scope has two independent permissions:

- **Can invite me:** the contact may send invitations to shared spaces.
- **Can message me directly:** the contact may start or continue a direct conversation.

Each permission has a duration: until revoked, for a fixed period, or once. A once permission is consumed by its first use, and an expired permission silently stops delivery until renewed. Durations exist so a person can accept an invitation from someone without granting them a standing line of communication. Timed grants are stored with their expiry and shown as remaining days; editing a contact's trust restarts a timed grant from the moment it is saved.

Blocking a person removes them as a contact, rejects everything they send, and hides them from pickers. A blocked person receives no signal that they were blocked. Unblocking restores nothing automatically; a new contact request is required.

Contacts are the person's own social graph and belong to the portable vault, so they follow the person across their linked devices.

## Invitations

An invitation names one space, one role, and one history boundary: the entire retained history of the space, or only what happens from the moment the invitation is accepted. The boundary applies to channel history, page content and comments, task history, and event details alike, matching the history choice that [Notes access](../notes/links-and-collaboration.md#access) already requires when prior content would become readable.

Invitations can only be addressed to contacts with the invite permission, so the participant picker never offers strangers. Inviting someone who is not yet a contact starts with the contact card exchange.

An invitation is pending until it is accepted, declined, revoked by the sender, or expires after a bounded period. Pending invitations are visible to both sides: the recipient in the Chat sidebar and Settings, the sender in the member list of the space with a pending marker. Declining sends no reason, and the sender is told only that the invitation was declined.

Acceptance depends on the space, because consent is given once at the level where it matters:

- **Group and project:** explicit acceptance, with the role and history boundary shown before accepting.
- **Channel:** adding a project member to a channel needs no acceptance; joining the project was the consent. People outside the project cannot be added to one of its channels.
- **Calendar event:** an attendee response, accepted, tentative, or declined, using the same response model as imported attendees.
- **Notes page or subtree:** explicit acceptance, like a project invitation scoped to a page.
- **Direct conversation:** no invitation. A conversation exists as soon as a contact with the direct-message permission sends the first message. A direct conversation can include several contacts, each of whom granted that permission to the person who started it.

## Roles and capabilities

A role is a preset over one capability set. The capabilities are grouped so a person can understand a role at a glance and open the full matrix only when they choose Custom:

| Group | Capabilities |
| --- | --- |
| Conversation | Read history, participate, assign work to agents |
| Content | Create and edit Notes, create and edit tasks, edit Calendar events, share pages |
| Management | Manage channels, manage members, manage agents, edit space settings |
| Execution (agents only) | Working-folder ceiling and approval policy, as defined by [access profiles](../chat/agents-and-coordination.md) |

The built-in roles are:

- **Owner:** every capability. Each space has exactly one owner, who cannot be removed and can transfer ownership explicitly.
- **Administrator:** every capability except deleting the space and transferring ownership.
- **Member:** conversation and content capabilities.
- **Guest:** read and participate in the channels or pages they were added to, nothing else.

Every member row shows the role name and a one-line summary of what differs from the default, for example the history boundary or a custom restriction. Choosing Custom shows the matrix with the preset's values filled in, so a custom role is always an edit of a preset rather than a blank form.

Roles inherit downward and can only narrow:

- A group role is the default role in every project of the group. A project can restrict a person's inherited role but not widen it.
- A project role is the default for its channels. A channel membership can further restrict history or participation.
- An agent acts within the intersection of its owner's role and its own access profile, and never holds management capabilities.

This mirrors the rule in access control that no layer may widen another. Access profiles for agents remain what they are today; the role presets for people reuse the same capability vocabulary so one access step can present both.

## Agents and people as one model

A person has an identity and access. An agent has an identity, access, a brain (provider, model, instructions, effort), and execution limits (working folders and approval policy). Everything else is shared:

- The same picker lists contacts and agents and offers to invite a person or create an agent at the end of the list.
- The same access step presents a role preset, a summary line, a history boundary, and a Custom expansion.
- The same member rows, grouped as People and Agents, with the local person pinned first.

An agent executes only on devices of the person who owns it. Other members of a space can mention it and assign it work only when the owner enabled assignment from others; a mention from someone else never widens what the agent may do.

## Where people appear

- **Chat channels:** the channel dialog edits name, topic, section, and members together, and the header opens a details panel with the topic and members. Both use the shared picker and access step. See [Conversations](../chat/conversations.md).
- **Chat sidebar:** a Direct messages section whose more-options menu opens Contacts and Invitations in Settings > Contacts.
- **Settings > Contacts:** the person's contact card (until pressed, the QR and code show a blurred placeholder that is not derived from the real identity, so a shared screen leaks nothing), contacts with their trust scopes and expiry, pending received and sent invitations, and blocked contacts. AI agents stay under Settings > Chat while they are only usable in Chat.
- **Projects:** a Members list in project settings with roles, and the shared picker for task assignees and reviewers, limited to project members. Groups have the same Members list. See [Projects](../projects/README.md).
- **Calendar:** attendees chosen with the shared picker, each with a response chip, next to imported attendees that stay read-only. See [Event editing](../calendar/event-editing.md#meeting-metadata).
- **Notes:** a Share action on a page and on a database page that uses the shared picker with page or subtree scope and the Notes role names Can view, Can comment, Can edit, and Full access, which map onto the capability set above. See [Links and collaboration](../notes/links-and-collaboration.md#access).

## Safety and privacy

- Nothing is discoverable. Without a handed-out card there is no way to reach a person.
- Removal and revocation stop future reads, context assembly, notifications, and exports immediately, and never rewrite legitimate shared history. They cannot erase copies another person already holds, and the app says so where it matters.
- Each space has an anchor that orders membership and role changes; editing never waits for it, and an organization can run it always-on on hardware it controls. Offline edits by a removed member that the anchor had not received become late changes that an admin restores or discards. See [Device linking and synchronization](../../data/sync.md#topology-and-authority).
- Mentions, links, and references identify context and never transfer access, for people exactly as for agents.
- Anti-burnout defaults apply to people as to agents: no read receipts, typing indicators, or online presence are required for the model, and none are planned as defaults.

## Unavailable controls

Collaboration controls ship before their logic so the complete experience can be judged. A control whose behavior is not implemented keeps its final appearance and placement, shows the not-allowed cursor, is marked unavailable for assistive technology, and does nothing when activated. There is no muted color, badge, tooltip, or explanatory copy; the product does not narrate its own roadmap in the interface. The shared `control-unavailable` utility in [UI foundations](../../architecture/ui-foundations.md) is the only way to mark such a control, so removing it later is a search, not a redesign.

## Open decisions

- Whether AI agents move from Settings > Chat to Settings > Contacts once an agent can act outside Chat.
- Whether a space owner can delegate ownership transfer to administrators.
- Group direct conversations beyond a small fixed size.

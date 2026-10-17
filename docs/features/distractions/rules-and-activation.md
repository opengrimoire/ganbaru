# Distraction rules and activation

## Current activation

Browser, desktop, and Android phase rules are global active-vault configuration. They can apply during focus, short break, or long break and can pause while a focus session is manually paused. The native Focus owner publishes a short-lived phase snapshot to adapters that enforce while the main UI is absent.

Usage limits are a separate activation source and remain active outside Pomodoro phases.

## Planned activation

Work environments may later provide context-specific overrides and manual activation. A morning routine may later activate rules after a sleep-alarm flow. These are planned and must not be described as current rule ownership.

Environment overrides should reuse the same deterministic matcher while preserving current global configuration as an explicit fallback. They cannot silently migrate canonical rule storage or grant broader native authority.

## Rule modes

**Blacklist mode** allows by default and blocks selected hosts or category presets, with explicit exceptions.

**Whitelist mode** blocks ordinary browsing during an active phase and allows selected task resources and safety surfaces.

Desktop and Android app enforcement is blocklist-only. Application allowlisting is represented by absence from the selected block targets plus mandatory protected-app rules.

## Browser rule kinds

Implemented browser rules cover normalized hosts, built-in categories, custom category stacks, explicit blocked hosts, explicit allowed hosts, and exceptions. Planned richer rules may target paths, videos, playlists, channels, creators, or locally extracted task metadata.

User-authored URL patterns use a bounded application grammar, not arbitrary regular expressions. Query-string matching is off by default because queries can contain searches, document identifiers, tokens, and personal data.

## Deterministic precedence

The target matcher resolves conflicts in this order:

1. Emergency and browser safety allowlist.
2. Explicit session allow.
3. Explicit event allow.
4. Explicit event block.
5. Explicit future environment allow.
6. Explicit future environment block.
7. Category allow.
8. Category block.
9. Default policy for the selected mode.

Within one level, the most specific path or host rule should win (target behavior; the current matcher takes the first matching rule). User-authored rules win ties against built-in presets. Any recency tie-breaker must use durable rule ordering and be visible in the explanation.

Session, event, and environment levels are planned. The current matcher implements the safety allowlist, then (in blacklist mode) explicit exceptions, explicit blocked hosts, custom category stacks, and built-in categories, or (in whitelist mode) allowed hosts. Future levels must slot in without changing the relative order of these.

## URL normalization

Matching canonicalizes scheme and host, removes default ports, normalizes internationalized hosts, removes fragments, and excludes query strings by default. A host rule includes its documented subdomain behavior. Path rules remain explicit and bounded.

## Pomodoro lifecycle

Entering a configured phase publishes fresh enforcement state. Moving to a disallowed phase clears or changes the applicable rule snapshot. Manual focus pause can suspend enforcement when configured. Resume requires fresh state rather than reviving an expired snapshot.

Stopping a run, ending an event, switching vaults, clean app exit, and factory reset clear current phase enforcement. Abrupt process death is handled by short expiration so stale rules fail open.

## Content-aware rules

Content-aware matching is planned. The progression is:

1. Deterministic URL, path, video, playlist, and channel identities.
2. Minimal local metadata on explicitly supported sites.
3. Optional AI relevance only when the user selects a provider and approves the context and redaction policy.

The extension never sends complete page text directly to a remote model. Deterministic rules remain the fallback when metadata or AI is unavailable.

## Default categories

Built-in presets may cover social media, streaming, news, sports, adult content, gambling, gaming, shopping, dating, and trading. News remains disabled by default because it frequently overlaps legitimate research.

Presets are maintained rule bundles, not a browsing catalog. Users can disable a broad preset and create a precise custom stack when it causes false positives.

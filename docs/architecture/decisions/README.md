# Architecture decisions

This directory contains focused implementation decisions whose rationale should remain useful after files and private APIs change.

- [Chat dependencies](chat-dependencies.md): reviewed process, credential, terminal, rendering, editor, observation, and transport dependencies.
- [Sync engine](sync-engine.md): capture and sealing, registers with version vectors, installation writers, the operation codec, and rejected sync alternatives.
- [Sync topology and authority](sync-topology-and-authority.md): leaderless content and delivery, one anchor per space for authorization, removal cutoffs, admin keys, and relays.
- [Sync encryption](sync-encryption.md): space keys, HPKE key envelopes, XChaCha20-Poly1305, and the dependency review behind them.

Feature behavior belongs in feature specifications. Dependency versions remain authoritative in manifests. A decision document explains why a capability uses a dependency or boundary and what security constraints accompany it.

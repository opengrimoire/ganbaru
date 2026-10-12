# External AI integrations

Status: Planned. None of these surfaces are implemented.

External integrations are separate from the internal coding-agent runtime. They require their own authorization and never infer access from a shell, provider login, or Chat membership.

## BYOK assistant

A compact general-user surface for questions, summaries, and bounded actions over explicitly selected Ganbaru AI context. Users choose a hosted or local provider and see what context will be sent before an action runs.

The assistant never becomes an ambient reader of the vault. Context comes from the current surface, selected records, or a named context action. Destructive or externally visible actions require typed commands and appropriate confirmation.

## External MCP service

A separately authorized MCP service for external clients, exposing bounded resources and typed tools for Calendar, Projects, Notes, and derivative reports. It is distinct from the ephemeral loopback MCP endpoint used by internal coding-agent runs, which is application infrastructure and not a general external API.

## CLI

A `ganbaru-ai` CLI for explicit external queries and derivative exports for scripts, agents, and collaborators. It must use application-owned authorization and data services rather than treating direct database or filesystem access as permission. Documentation must not imply that the executable already ships.

## Context actions

Reusable context actions select bounded information, such as today's calendar and active focus block, one project with its open tasks and selected Notes, a chosen Notes page with its linked context, or a project status summary. They respect participant access, selected workspace, provider, budget, and redaction policy, and never build one unbounded memory of the vault.

## Multi-person behavior

Human collaboration introduces encryption, revocation, historical visibility, and per-participant authorization questions, so external AI access cannot be finalized independently of the [sync](../../data/sync.md) and [access control](../../data/access-control.md) rules.

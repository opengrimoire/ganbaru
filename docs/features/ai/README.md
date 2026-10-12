# AI integrations

Status: Partial. The local coding-agent path is implemented; the BYOK assistant and external MCP and CLI access are planned.

Ganbaru AI is fully usable without AI. Every AI path is opt-in, has an explicit authority boundary, and stays subordinate to the user's local data and organizational model.

## Integration paths

| Path | Status | Purpose |
| --- | --- | --- |
| Local coding-agent Chat | Implemented | Run user-installed coding harnesses inside an authorized project working folder or private scratch scope. |
| BYOK general assistant | Planned | A small general-purpose assistant backed by a user-selected hosted or local model. |
| External MCP and CLI access | Planned | Separately authorized, bounded data access and derivative exports for external clients and scripts. |

The local coding-agent path is not a general vault assistant. It is the execution layer beneath [Chat](../chat/README.md): Chat owns channels, participants, messages, assignments, and review, while providers own bounded reasoning and execution sessions. A provider or model is never the canonical identity of an agent, channel, task, or decision.

## Authority model

Rust resolves the effective project, conversation, participant, execution target, context package, provider, model, interaction mode, and authorization before work starts (see [Chat agents and coordination](../chat/agents-and-coordination.md#resolution-before-work-starts)). Provider-native trust or approval cannot widen Ganbaru AI authority.

Svelte renders validated canonical events and read models; it never receives a generic shell or arbitrary filesystem capability. Credentials stay behind operating-system credential references, and provider processes receive only the environment their configured runtime needs. Authorization details live in [Chat access control](../../data/access-control.md).

## Data and privacy

Execution events, provider-thread identities, messages, projections, drafts, attachments, checkpoints, command receipts, and authorization records live in the active vault according to the [data architecture](../../data/architecture.md). External paths, executable paths, provider homes, process state, and native credentials stay device-local.

Every path must explain what leaves the device:

- Local coding harnesses talk to whatever provider the user installed and authenticated.
- The planned BYOK assistant sends only the selected context to the configured provider; local providers can keep model traffic on the device.
- The planned external MCP or CLI surface requires its own authorization and never inherits internal Chat authority.

## Documentation map

- [Provider runtimes](provider-runtimes.md)
- [External integrations](external-integrations.md)
- [Chat execution and workspace](../chat/execution-and-workspace.md)

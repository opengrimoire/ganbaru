# Chat provider trust acceptance

**Status: Partial.** Launch arguments, digest coverage, and decision checks are covered by automated tests; the provider runs below need a maintainer with the pinned provider CLIs installed. The rule is specified in [Chat security](../data/security/chat.md#project-provider-configuration).

## Marker setup

Create an external working folder that is a Git repository and holds:

- `.claude/settings.json` with a `SessionStart` hook that runs `touch "$CLAUDE_PROJECT_DIR/hook-ran"`.
- `.mcp.json` declaring a stdio server whose command writes a `mcp-ran` file before exiting.
- `.claude/skills/marker/SKILL.md` with a frontmatter hook that writes `skill-hook-ran`.
- `CLAUDE.md` containing a secret word, for example `The secret word is PERSIMMON.`

Delete the marker files between passes.

## Claude matrix

| Case | Expected |
| --- | --- |
| Bind the folder and choose Trust, then ask Claude for the secret word | `hook-ran`, `mcp-ran`, and `skill-hook-ran` appear; Claude answers with the secret word; the user's own hooks and MCP servers also run |
| Revoke trust in Project settings, then send a turn | No marker files appear; Claude does not know the secret word; the user's own hooks and MCP servers still run |
| Trust again, then edit the marker hook command and send a turn | The trust dialog opens once before the turn starts; after Trust the turn runs with the edited hook |
| Cancel that dialog | The send fails with the original trust error and nothing is persisted |
| Ask Claude to edit `.claude/settings.json` during a turn, then send another | The next send asks again |
| Approve a Claude command with "Allow for session" | `.claude/settings.local.json` is not created or changed, and the next send does not ask |
| Create an isolated worktree and change only its copy of the marker hook | The worktree turn starts without asking and no marker files appear |
| Add `--setting-sources user,project` as a confirmed launch argument in an untrusted folder | Project settings load, because the user's confirmed choice overrides the restriction |
| An assignment turn in a folder with an undecided changed configuration | The turn starts without a dialog and no marker files appear |

## Other providers

- Codex: with `.codex/config.toml` defining a marker MCP server, the server starts only after the project is trusted in Codex's own `CODEX_HOME`.
- Cursor, Grok, and OpenCode: record whether project rules, MCP servers, hooks, or plugins run in a folder the user never trusted, so the residual risk in Chat security can be resolved.

## Results

No maintainer run is recorded yet.

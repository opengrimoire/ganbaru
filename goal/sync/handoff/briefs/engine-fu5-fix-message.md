Fix round for engine follow-up 5 (branch `feat/sync-hook-concurrent-groups`, head e3b653e8). A fresh review returned "fix first". Engine track; same rules as the brief (`briefs/engine-fu5-brief.md`, the cloud implementer brief, lock, CARGO_INCREMENTAL=0, unsigned commits approved by the user for this goal, no amend or rebase). Verify each finding against source first; if one is wrong, say why in the progress entry instead of changing code.

High:
1. Settles run on devices that cannot seal (`materialize.rs` settle ~715, called from `apply.rs` ~1178): only the read-only guard is checked, not `slot.may_seal` (which repairs use). The app calls `Engine::apply` with no writer while identity is missing, and a writer can become inactive or removed. Such a device writes the settled value into its row and records a capture that never seals; later operations on that row return `pending_captures` and apply stalls. Pass a can-seal flag into `execute` and skip settling when it is false. Test apply with no writer and with an inactive writer.

Medium:
2. Settles run mid-chain: when `resuming`, `finish` defers repairs because the state is half an edit, but settle still runs on that partial state. Skip settles while resuming, and make sure the chain's final pass still settles. Test.
3. Validation: `validate::group_value` does not check domain `CHECK` constraints, so a hook value that violates one fails `execute_update` and aborts apply; an error from `adapter.derived` after validation maps to `corrupt`. Make both report an invalid settle (skip the write, keep apply going), for example with a savepoint around the settle write. Test.

Low:
4. `docs/algorithms/sync/README.md`: state the requirement that the hook reads only replicated inputs and settles to a value the static priority then keeps (monotonic), otherwise devices with different rows write different values and a clock-ranked Register group can re-settle back and forth.
5. Add a test where the winner's stored value was elided and saved back before the settle.

Checks: `cargo test -p ganbaru-sync --lib -j 4 -- --test-threads=4`, `cargo test -p ganbaru-sync-replica --lib -j 4 -- --test-threads=4`, `cargo test -p ganbaru-chat --lib -j 4 sync -- --test-threads=4`, `cargo test -p ganbaru-notes --lib -j 4 sync -- --test-threads=4`, clippy `--workspace --all-targets -j 4 -- -D warnings`, `cargo fmt --check`. Before final checks, if feat/sync moved, merge it (`merge: feat/sync into feat/sync-hook-concurrent-groups`) and rerun. Update `<SCRATCHPAD>/engine-fu5-progress.md` (copy it from `goal/sync/handoff/progress/` if missing). Final report under 250 words.

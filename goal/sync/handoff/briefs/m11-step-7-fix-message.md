Fix round for milestone 11 step 7 (same worktree, branch, rules, lock, and target dir as before). A fresh Opus security review returned "fix first". Verify each finding against source first; if one is wrong, say why in the progress entry instead of changing code. Own commits, signed.

High:
1. `SyncKeystore.kt` ~135-141 maps `UnrecoverableKeyException` to INVALIDATED; on Android 12+ it wraps any keystore error from `getKey`, including transient system or busy errors, so one transient error unlinks the device and deletes every `.key` file. Map it to UNAVAILABLE unless the cause chain shows a permanent invalidation (`KeyPermanentlyInvalidatedException` or the documented permanent codes), and before unlinking for key loss confirm the Keystore is healthy (seal and open a throwaway alias); otherwise treat it as unavailable and retry later.
2. `key_wrap.rs` ~150 reads `envelope[4]` before the length check at ~153: a 4-byte `GKWR` file panics. Check the length first; add a test with truncated envelopes of every short length.

Medium:
3. `remove_unprotected_keys` (`key_protection.rs` ~99-118) deletes plaintext files before unlinking; a crash between leaves a linked device whose key reads return `Ok(None)` and never unlink. Unlink (durably) first, then delete. Test the crash order if the structure allows.
4. A phone whose Keystore key is lost while unlinked gets stuck: `pairing.rs` `identity()` maps the read error to `DEVICE_KEY_UNAVAILABLE`, only `Ok(None)` reaches `replace_lost_identity`, and the service unlink path skips unlinked devices. Make a permanent key loss on an unlinked device reach `replace_lost_identity` (or the equivalent clean reset) so it can link again. Test.
5. StrongBox fallback catches only `StrongBoxUnavailableException` (`SyncKeystore.kt` ~113-117); fall back to a non-StrongBox key on any `ProviderException` when StrongBox was requested. Record in progress that the 50 ms unwrap budget with StrongBox is unmeasured.

Low:
6. `call_in_frame` (`key_protection.rs` ~280-284) zeroes argument arrays before checking the call result, so JNI runs with a pending exception; check and clear the exception first.
7. `is_key_protection_lost` matches a substring of the whole pass error, which includes remote hub `code: message` text (`client.rs` ~759): a hostile coordinator could trigger a key wipe. Use a typed error variant instead of string matching. Test that a remote message containing the text does not unlink.
8. A lost key on one vault wipes every vault's key files while forgetting only two aliases. Restrict the wipe to the affected vault's files and the shared linking key where they are shared, or record precisely why a global wipe is needed and forget every alias it orphans.
9. Add a host test for `unlink_if_key_protection_lost`. Verify `androidx.test:runner:1.5.0` is already resolved in the app (Gradle files, version catalog, or lockfiles); if it is not, keep it but record in the progress entry that it is a new test-only dependency awaiting the maintainer's approval.
10. Docs: `docs/data/sync.md` claims loss "removes every sealed key file and its Keystore keys" and that unsealed files are "deleted" without saying the device unlinks; make both match the code after these fixes.

Then merge feat/sync (latest) with `merge: feat/sync into feat/sync-android-keystore`, and rerun: fmt, clippy workspace all targets, `cargo test -p ganbaru-sync-replica --lib`, `cargo test -p ganbaru-tauri-app --lib`, `cargo test -p ganbaru-handoff --lib`, `cargo check -p ganbaru --bin ganbaru`, the unsafe inventory check, and the Android cargo check. Update /home/victor/.cache/ganbaru-goal-scratch/milestone-11-step-7-progress.md. Final report under 250 words.

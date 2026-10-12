# Unsafe Rust

Unsafe Rust is exceptional in Ganbaru AI. It is allowed only at a reviewed native, foreign-function, or operating-system boundary whose required behavior cannot currently be preserved with safe Rust. The [`unsafe` keyword](https://doc.rust-lang.org/stable/reference/unsafe-keyword.html) creates proof obligations the compiler does not check. Calling an upstream function marked `unsafe` does not transfer those obligations upstream: Ganbaru AI remains responsible for the arguments, ownership, lifetime, aliasing, thread, callback, and cleanup invariants at every call.

## Scope

This page covers tracked, repository-owned Rust: libraries, binaries, platform-gated modules, tests, examples, build scripts, and tracked generated Rust. It includes unsafe blocks and functions, unsafe traits and implementations, external blocks, unsafe attributes, assembly, raw-pointer operations, and unsafe code emitted by repository macros.

Dependency-owned unsafe code, RustSec findings, and audit exceptions are separate concerns recorded in [Dependency audits](dependency-audits.md). A clean dependency audit does not prove first-party unsafe code sound, and the reverse is also true.

Build scripts, examples, tracked generated Rust, and `ganbaru-native-messaging` contain no first-party unsafe code. There are no unsafe traits or implementations, inline assembly, or macros that emit unsafe code. The only unsafe attributes are the `export_name` attributes on the Android JNI callbacks below.

## Policy

- Prefer a safe standard-library, workspace, or already-approved upstream API when it preserves the same behavior and security properties. Do not replace descriptor-relative, no-follow, atomic, or process-tree behavior with a weaker path-based or single-process operation merely to remove `unsafe`.
- Keep each unsafe operation in the smallest practical private wrapper, exposing a safe interface only when every unchecked requirement is established internally. A public `unsafe fn` requires rustdoc with a `# Safety` section and a reason a safe interface is impossible.
- Put an immediate `// SAFETY:` comment on every unsafe block or operation stating the concrete local proof (pointer validity and alignment, initialized output, buffer capacity, handle ownership, callback lifetime, thread affinity, or balanced cleanup). Restating the API name or saying the call was reviewed is insufficient.
- Declare every foreign block as `unsafe extern` with the exact ABI and signatures from the authoritative native headers. The [Rust Reference](https://doc.rust-lang.org/stable/reference/items/external-blocks.html) explains why declaration correctness is itself an unsafe obligation.
- Check null pointers, invalid handles, return codes, initialized lengths, integer conversions, and sentinel values before constructing a Rust reference or owned value. Transfer native ownership exactly once and prefer RAII types such as `File`, `OwnedFd`, and `OwnedHandle`.
- Foreign callbacks must not unwind across the ABI boundary. Captured state must outlive registration, shared state must be synchronized, and unregistration must happen before the last referenced allocation is released. A caught panic payload is leaked rather than dropped, because its destructor may also panic and restart the unwind.
- Treat platform documentation as part of the proof. Review upstream versions when an invariant depends on Tauri, Wry, `windows`, `objc2`, SQLx, libc, or another binding's representation or callback behavior.
- Add focused tests for the safe wrapper's success, failure, cleanup, and race-sensitive behavior. Tests support a proof but do not replace one. Platform-gated boundaries require compilation and validation on the owning target.

The five libraries that own retained boundaries (`ganbaru-tauri-app`, `ganbaru-chat`, `ganbaru-chat-providers`, `ganbaru-mobile-notifications`, and `ganbaru-mobile-media`) deny `clippy::undocumented_unsafe_blocks` and `unsafe_op_in_unsafe_fn`. These lints enforce local structure but do not prove a safety comment correct.

## Review method

1. Enumerate every tracked `.rs` file, including tests, examples, build scripts, and generated sources.
2. Search for unsafe syntax and related operations: split token forms, `extern` blocks, callback types, unsafe attributes, raw pointer creation or dereference, `MaybeUninit`, `from_raw` transfers, inline assembly, FFI declarations, and macro input that can produce unsafe tokens.
3. Separate comments, strings, fixtures, and dependency paths from executable first-party code.
4. Read the full control flow around each result, error path, callback, ownership transfer, and destructor, including safe callers, because a safe wrapper must not admit invalid inputs.
5. Compare the code with primary Rust, operating-system, library, and framework contracts, and classify each mechanism as removed, retained, or replaceable when a named upstream capability stabilizes.

## Retained boundary inventory

Each entry names the owning modules, why the boundary exists, its key safety invariants, any accepted residual risk, when to replace it, and what validation it needs.

### Android Focus authority callbacks

**Owned module:** `crates/ganbaru-mobile-notifications/src/authority.rs`.

**Contract:** Two private JNI symbols match Kotlin's static `NativeFocusAuthority.isCurrent(long, long, long)` and `NativeFocusAuthority.isProcessCurrent(long)`. Notification delivery and the app-private authority provider use the first to reject superseded native revisions before Guardian phase or completion publication. Revocation and language-copy updates use the process check, which still works after phase authority is cleared, so an older process cannot cancel a newer phase or overwrite newer copy. Guardian passes only the native process nonce (a positive value from the app's secure random source; an identity fence, not a credential) plus, for phase admission, the publication generation and execution revision. Tauri's asynchronous plugin bridge cannot provide this synchronous check.

**Safety invariants:** The unsafe attributes only fix symbol names per [JNI naming](https://docs.oracle.com/en/java/javase/21/docs/specs/jni/design.html); the bodies are safe Rust. The two JNI pointers are opaque, never dereferenced or retained, and no JNI reference crosses the boundary. Inputs follow [JNI primitive types](https://docs.oracle.com/en/java/javase/21/docs/specs/jni/types.html). One process-lifetime checker is installed before delivery starts. Invalid input, absent registration, revoked ownership, expired leases, and callback panics fail closed. The process check invokes no application callback and cannot unwind. Shrinker rules preserve the class and method names, and Kotlin reports unloaded callbacks as unavailable.

**Replacement trigger:** The Tauri mobile bridge gains an equivalent synchronous, native-source check at the Guardian publication boundary.

**Required validation:** Invalid input, absent or duplicate registration, revision replacement, process identity mismatch, revocation without an active phase, restarted processes with coincident counters, and late old-process cancellation. ARM64 compilation plus inspection of the shared-library exports and Kotlin signatures. Physical cross-process notification acceptance is a separate Android check.

### Android Music delivery callback

**Owned module:** `crates/ganbaru-mobile-media/src/authority.rs`.

**Contract:** A private JNI symbol matches Kotlin's static `NativeMusicAuthority.isCurrent(long)`. The Media3 bridge checks the native owner immediately before consuming a queued decoder effect and again after asynchronous document resolution. Deliveries are volatile and live only in the registry's process, so process exit destroys both the queue and its authority; positive delivery IDs never repeat within a process.

**Safety invariants:** Same JNI shape as the Focus callbacks: an exact [exported name](https://doc.rust-lang.org/reference/abi.html#the-export_name-attribute), opaque unretained JNI pointers, primitive inputs, and no JNI reference crossing. Checks perform no vault filesystem I/O and fail closed on unavailable ownership locks, expired leases, changed Focus phase or mode, canceled delivery, or a newer vault lifecycle revision. All checker panics are contained.

**Replacement trigger:** The Tauri mobile bridge supplies an equivalent synchronous check at SDK consumption and asynchronous source admission.

**Required validation:** Absent or duplicate registration, invalid IDs, live revocation, panic-on-drop payloads, late acknowledgement, cancellation during SDK execution, delayed source lookup, phase changes, ownership I/O contention, and freeze/resume replacement. ARM64 compilation with export and signature inspection. Physical stalled-thread, slow-provider, service destruction, and vault handoff acceptance is separate.

### Descriptor-relative Unix filesystem operations

**Owned modules:** `crates/ganbaru-chat/src/workspace/files/platform/unix.rs` and `apps/client/src-tauri/app/src/chat/execution_environment.rs`.

**Contract:** These modules use POSIX [`openat`](https://pubs.opengroup.org/onlinepubs/9799919799/functions/open.html), directory-stream, metadata, unlink, and rename families to keep authorization relative to already-open directory descriptors, plus Linux, Android, and Apple atomic no-replace and exchange extensions. Android invokes the `renameat2` system call directly because the bionic symbol is newer than the minimum API level; an unsupported kernel makes the mutation fail closed, never fall back to a weaker path sequence. Stable `std` lacks descriptor-relative traversal, no-follow inspection, and atomic replacement, and path prechecks would reintroduce symlink and rename races.

**Safety invariants:** Every name is one validated component with no NUL, separator, absolute form, `.`, or `..`. Parent descriptors stay live for each call. Creation mode is supplied exactly when creation flags require it. Each successful descriptor has one Rust owner. `fdopendir` receives an owned duplicate, the stream stays exclusive, and each `readdir` name is copied before the next call; errno distinguishes end of stream from failure on Linux, Android, and Apple, and other Unix targets report directory streams as unsupported. Output is treated as initialized only after a successful call. After an exchange, the installed entry must match the prepared file's identity and revision, and the displaced entry the original's, before it supplies permissions or becomes a rollback candidate.

**Accepted residual:** Authorization verifies the working-folder identity, but the file service later reopens the stored root path without carrying the original handle or revalidating it, so a same-user replacement of the root directory can cross that interval (traversal below the root remains descriptor-relative and no-follow). Rollback and cleanup are path-based after the verifying handle closes; a concurrent same-user writer can cause a recovery error with preserved artifacts, or, in a narrow window, have a substituted object deleted while cleanup reports success. Failure paths preserve identity-uncertain artifacts rather than unlinking them. Closing these gaps needs an opened-root capability and an exact-object deletion primitive or a serialized directory owner.

**Replacement trigger:** Stable Rust or an approved safe dependency provides the same descriptor-relative, no-follow, metadata, and atomic replacement guarantees on supported Unix targets.

**Required validation:** End-of-directory and error results, repeated open and drop, non-UTF-8 names, symlinks, special files, parent replacement races, stale revisions, permission preservation, atomic rollback, and each platform's rename variant on its owning target. Android validation uses the declared minimum API level and covers an unsupported-kernel result.

### Windows filesystem identity and atomic replacement

**Owned modules:** `crates/ganbaru-chat/src/workspace.rs`, `crates/ganbaru-chat/src/workspace/files/platform/windows.rs`, and `apps/client/src-tauri/app/src/chat/execution_environment.rs`.

**Contract:** [`GetFileInformationByHandle`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfileinformationbyhandle) provides directory identity, [`GetFileInformationByHandleEx`](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex) full file identity, [`ReplaceFileW`](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew) atomic replacement, and [`MoveFileExW`](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw) without replacement flags fail-closed moves. Backup and recovery names use [`BCryptGenRandom`](https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcryptgenrandom). Safe by-handle metadata is still [unstable](https://github.com/rust-lang/rust/issues/63010) and no safe operation preserves the replacement contract.

**Safety invariants:** The owning `File` keeps every borrowed handle live, and invalid handles or failed output initialization are rejected. Persisted identity is the volume serial number and 64-bit file index from [`BY_HANDLE_FILE_INFORMATION`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/ns-fileapi-by_handle_file_information), separated by authorization domain. Replacement verifies the full 128-bit [`FILE_ID_INFO`](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info) identity and content revision of each file through the same handle, and succeeds only when the installed target is the prepared object and the backup is the expected original. Reparse points are rejected where a directory must be physical. UTF-16 buffers reject interior NUL, are terminated, and stay live for each call. Backup names carry an independent 256-bit random token, and RNG failure is checked before any destructive call. All files stay inside the validated parent, and every documented partial-success result is handled without reporting an uncertain state as committed.

**Accepted residual:** Microsoft does not guarantee the 64-bit file index is unique on ReFS. Switching persisted identity to `FILE_ID_INFO` would invalidate stored working-folder and Git storage identities, so ReFS working folders must not be described as fully supported until a versioned migration or explicit rebind contract exists. Descendant opens are path-based after authorization, and `ReplaceFileW` is not a compare-and-swap, so the same-user race classes described for Unix also apply; failure paths preserve uncertain artifacts.

**Replacement trigger:** Stable by-handle metadata and a safe atomic replacement API with the required identity width, reparse-point behavior, and failure semantics.

**Required validation:** NTFS identity stability, distinct authorization domains, reparse and interior-NUL rejection, replacement success, content verification, partial failures, rollback, RNG failure, generated-name collisions, and no-replace moves onto an existing destination. A future `FILE_ID_INFO` migration must cover ReFS and rebind behavior.

### Provider process trees and native process probes

**Owned modules:** `crates/ganbaru-chat-providers/src/process.rs`, `crates/ganbaru-chat-providers/src/opencode/tests.rs`, `apps/client/src-tauri/app/src/chat/tests/process.rs`, and `apps/client/src-tauri/app/src/chat/tests/process_windows.rs`.

**Contract:** Unix providers are stopped with POSIX [`kill`](https://pubs.opengroup.org/onlinepubs/9799919799/functions/kill.html) on a negative process-group ID. Windows providers use [job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), suspended creation, and Tool Help thread enumeration. Test probes use native liveness checks to verify descendants, not only the leader. Unix process-group methods are [unstable](https://github.com/rust-lang/rust/issues/141975), and stable Windows `Child` does not expose the [main-thread handle](https://github.com/rust-lang/rust/issues/96723).

**Safety invariants:** A process-group ID is greater than one and representable before negation, and only a fixed signal allowlist reaches `kill`, so zero and `-1` can never be targeted. `ESRCH` means already stopped. A numeric group ID is not a kernel capability, so a narrow ID-reuse race after reaping remains. On Windows the child stays suspended until it is assigned to an owned job that kills descendants on close. Handles are checked and transferred to one RAII owner. The thread handle is revalidated against the child before `ResumeThread`, which must report the expected suspend count. Test probes distinguish timeout, exit, failed waits, and unexpected statuses, and are not production capabilities.

**Replacement trigger:** Stable standard-library process-group signaling, tree termination, and main-thread ownership with the same descendant and cleanup guarantees.

**Required validation:** A Unix fixture starts a real descendant, stops the group, and polls to `ESRCH`. Windows validation proves job closure stops descendants and covers Tool Help and resume errors. Probes cover permission-denied and failed waits.

### Windows desktop integration

**Owned modules:** `apps/client/src-tauri/app/src/benchmark/memory.rs`, `apps/client/src-tauri/app/src/distractions/foreground/windows.rs`, `apps/client/src-tauri/app/src/music/media_controls/windows.rs`, `apps/client/src-tauri/app/src/pomodoro/idle.rs`, and the Windows portion of `apps/client/src-tauri/app/src/pomodoro/overlay/enforcement.rs`.

**Contract:** Win32 process enumeration and memory queries, foreground window and process inspection, close messages, low-level keyboard hooks, execution-state and idle APIs, and system media transport controls interop. Primary contracts include [Tool Help snapshots](https://learn.microsoft.com/en-us/windows/win32/api/tlhelp32/nf-tlhelp32-createtoolhelp32snapshot), [`LowLevelKeyboardProc`](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc), [`SetWindowsHookExW`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw), [`GetLastInputInfo`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getlastinputinfo), and [SMTC `GetForWindow`](https://learn.microsoft.com/en-us/windows/win32/api/systemmediatransportcontrolsinterop/nf-systemmediatransportcontrolsinterop-isystemmediatransportcontrolsinterop-getforwindow). Tauri and stable Rust do not expose these capabilities with the same semantics.

**Safety invariants:** Handles and output structures are initialized, validated, and RAII-owned, buffer lengths match allocations, and return values gate which outputs are read. A foreground `HWND` and process identity are revalidated immediately before a close request, still treated as non-atomic. The keyboard hook runs on its installing thread, accepts only documented layouts, never unwinds, returns promptly, and always delegates unhandled events; installation surfaces rejection and the hook thread cannot block forever after stop. Overlay creation, reconciliation, replacement, close, and shutdown share one lifecycle barrier, main-thread waits run off the UI thread, and process exit and restart wait for overlay cleanup. SMTC callbacks run inside an unwind barrier. Power requests and media objects have balanced lifetimes. An idle-query failure is an unavailable reading, not proof of activity.

**Replacement trigger:** A safe Tauri, standard-library, or `windows` wrapper with equivalent ownership, callback, and failure behavior, per operation.

**Required validation:** On Windows: structure initialization and failed calls, process sampling cleanup, foreground identity changes, keyboard modifier transitions and blocked chords, hook shutdown, idle failure, balanced execution state, SMTC creation for valid and stale windows, callback panics, overlapping lifecycle operations, reconciliation during close, and process exit and restart with an active overlay.

### Native webview and window bridges

**Owned modules:** `apps/client/src-tauri/app/src/chat/preview/capture.rs` and the macOS window portion of `apps/client/src-tauri/app/src/pomodoro/overlay/enforcement.rs`.

**Contract:** Tauri's [`with_webview`](https://docs.rs/tauri/latest/tauri/webview/struct.Webview.html#method.with_webview) supplies a platform handle on the main thread, but WebView2 [`CapturePreview`](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2?view=webview2-1.0.1462.37) and WebKit [`takeSnapshot`](https://developer.apple.com/documentation/webkit/wkwebview/takesnapshot%28with%3Acompletionhandler%3A%29) cross COM or Objective-C interfaces. Linux capture uses WebKitGTK's safe method, but its completion closure still runs through a foreign non-unwind trampoline and follows the same containment policy. Tauri and Wry have no safe portable capture API or typed native-window operation for these uses.

**Safety invariants:** Native objects are non-null, of the expected type, live for each synchronous use, and retained for asynchronous callbacks. Calls stay on the main thread. COM streams are owned, bounded before allocation, rewound before reading, and never report more bytes than the destination holds. Every request resolves exactly once, and all completion bodies run inside an unwind barrier. Objective-C image and error pointers are checked before dereference. A Tauri window pointer is borrowed only while its window is live. macOS presentation options use overlapping leases: only the final guard restores the original state.

**Replacement trigger:** Tauri, Wry, WebView2 bindings, or `objc2` provide safe typed operations preserving asynchronous ownership and main-thread behavior.

**Required validation:** On Linux, Windows, and macOS: capture success, immediate and callback failure, panic containment, webview teardown, and exactly-once completion. On Windows, oversized and short streams. On macOS, window setup and teardown including rapid replacement with overlapping guards.

### macOS IOKit power assertions

**Owned module:** The macOS power portion of `apps/client/src-tauri/app/src/pomodoro/overlay/enforcement.rs`.

**Contract:** Pomodoro display and system wake locks use [`IOPMAssertionCreateWithName`](https://developer.apple.com/documentation/iokit/1557134-iopmassertioncreatewithname) and its release API. Stable Rust and Tauri provide no equivalent assertion ownership.

**Safety invariants:** Declarations match the C ABI. Assertion-type macros are represented by live Core Foundation strings, not nonexistent symbols. An assertion ID is read only after successful creation, owned by one guard, and released once. If the system assertion fails after the display assertion succeeds, the guard keeps display-only behavior until stop. Release failure is reported without reusing the ID.

**Replacement trigger:** A maintained safe API with distinct display and system assertions, explicit creation errors, and RAII release.

**Required validation:** On macOS: both assertion types, display-only fallback, repeated start and stop, release failure, and process cleanup.

### Unix hostname

**Owned module:** `apps/client/src-tauri/app/src/chat/terminal.rs`.

**Contract:** Terminal labels call POSIX [`gethostname`](https://pubs.opengroup.org/onlinepubs/9799919799/functions/gethostname.html) through libc while the standard-library hostname API is [unstable](https://github.com/rust-lang/rust/issues/135142).

**Safety invariants:** The buffer is writable for exactly the supplied length, and the return code is checked before reading. Output with a NUL terminator or a full buffer is accepted, converted with bounded lossy UTF-8, and rejected if empty or containing control characters.

**Replacement trigger:** A stable standard-library hostname API.

**Required validation:** NUL-terminated, full-length, invalid UTF-8, empty, control-character, and failed-call results through a testable parser.

### Test-only SQLite statement tracing

**Owned module:** `apps/client/src-tauri/app/src/first_use_contracts.rs`, compiled only for tests.

**Contract:** First-use contract tests install [`sqlite3_trace_v2`](https://www.sqlite.org/c3ref/trace_v2.html) because SQLx exposes no safe statement-trace hook. This must never enter production behavior.

**Safety invariants:** SQLx holds the connection lock while the hook changes. The in-memory fixture caps its pool at one connection and installs and removes the hook through it; this is not a general pool-safe abstraction. On normal completion the context outlives unregistration and the raw `Arc` reference is balanced once. Cancellation deliberately leaks that reference because SQLite may still hold the pointer. The callback checks every pointer before borrowing, copies SQL before the native lifetime ends, synchronizes shared state, ignores a poisoned counter instead of unwinding, and contains every panic.

**Replacement trigger:** SQLx exposes a safe trace callback with explicit registration lifetime and cleanup.

**Required validation:** Statement classification, install and remove cycles, callback error isolation, pool teardown, and cancellation safety.

## Platform validation gaps

Linux-host tests and Android ARM64 checks cover much of the inventory, but Windows and macOS boundaries still require compilation and native tests on correctly provisioned hosts. Linux-host tests do not replace runtime capture, keyboard-hook, system-media, power, window-lifecycle, or process-exit acceptance on the owning platforms. Neither `pnpm -w run validate` nor `validate:full` replaces manual unsafe review.

## Change discipline

A change that introduces or materially alters unsafe Rust must include:

1. The safe alternatives considered and the specific missing guarantee that requires the native boundary.
2. A minimal wrapper with concrete local `// SAFETY:` proofs for every operation.
3. Primary contract references and a review of initialization, pointers, sizes, ownership transfer, cleanup, callbacks, threading, and failure sentinels.
4. Focused tests for success, failure, cleanup, and race-sensitive behavior, plus validation on every affected native target.
5. An update to this inventory and its replacement trigger. A new dependency also requires the review in [Supply chain](supply-chain.md).

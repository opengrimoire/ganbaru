use super::*;

pub(super) async fn capture_recording_frames(
    app: tauri::AppHandle,
    tab: RuntimeBrowserTab,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) -> ChatResult<Vec<Vec<u8>>> {
    let started = Instant::now();
    let mut frames = Vec::new();
    let mut total_bytes = 0_usize;
    loop {
        let frame = capture_browser_png(&app, &tab).await?;
        total_bytes = total_bytes.saturating_add(frame.len());
        if total_bytes > MAX_RECORDING_BYTES {
            return Err(ChatError::new(
                ChatErrorCode::Protocol,
                "Browser recording exceeds the supported limit",
                true,
            ));
        }
        frames.push(frame);
        if started.elapsed() >= MAX_RECORDING_DURATION {
            break;
        }
        tokio::select! {
            _ = &mut stop => break,
            _ = tokio::time::sleep(RECORDING_FRAME_INTERVAL) => {}
        }
    }
    Ok(frames)
}

pub(super) fn recording_archive(frames: &[Vec<u8>], duration: Duration) -> ChatResult<Vec<u8>> {
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;
    use zip::write::{SimpleFileOptions, ZipWriter};

    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let manifest = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 1,
        "mimeType": "image/png",
        "frameIntervalMilliseconds": RECORDING_FRAME_INTERVAL.as_millis(),
        "durationMilliseconds": duration.as_millis(),
        "frameCount": frames.len()
    }))
    .map_err(|_| browser_unavailable_error())?;
    writer
        .start_file("manifest.json", options)
        .and_then(|_| writer.write_all(&manifest).map_err(Into::into))
        .map_err(|_| browser_unavailable_error())?;
    for (index, frame) in frames.iter().enumerate() {
        writer
            .start_file(format!("frames/{index:04}.png"), options)
            .and_then(|_| writer.write_all(frame).map_err(Into::into))
            .map_err(|_| browser_unavailable_error())?;
    }
    writer
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|_| browser_unavailable_error())
}

pub(super) async fn capture_browser_png(
    app: &tauri::AppHandle,
    tab: &RuntimeBrowserTab,
) -> ChatResult<Vec<u8>> {
    let webview = app
        .get_webview(&tab.webview_label)
        .ok_or_else(browser_unavailable_error)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    capture_platform_png(&webview, sender)?;
    let bytes = tokio::time::timeout(BROWSER_CAPTURE_TIMEOUT, receiver)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Timeout, "Browser capture timed out", true))?
        .map_err(|_| browser_unavailable_error())??;
    if bytes.is_empty() || bytes.len() > MAX_CAPTURE_BYTES {
        return Err(ChatError::new(
            ChatErrorCode::Protocol,
            "Browser screenshot exceeds the supported limit",
            true,
        ));
    }
    Ok(bytes)
}

#[cfg(any(target_os = "linux", windows, target_os = "macos", test))]
fn run_native_capture_callback(callback: impl FnOnce()) -> bool {
    // WebKit, WebView2, and Objective-C invoke these closures through foreign
    // callback ABIs that cannot accept a Rust unwind. A panic payload may also
    // panic when dropped, so leak that exceptional payload after catching it.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)) {
        Ok(()) => true,
        Err(payload) => {
            std::mem::forget(payload);
            false
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn capture_platform_png(
    webview: &tauri::Webview,
    sender: tokio::sync::oneshot::Sender<ChatResult<Vec<u8>>>,
) -> ChatResult<()> {
    use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};
    let sender = Arc::new(Mutex::new(Some(sender)));
    webview
        .with_webview(move |platform| {
            let sender = Arc::clone(&sender);
            platform.inner().snapshot(
                SnapshotRegion::Visible,
                SnapshotOptions::NONE,
                None::<&webkit2gtk::gio::Cancellable>,
                move |result| {
                    let completed = run_native_capture_callback(|| {
                        let encoded =
                            result
                                .map_err(|_| browser_unavailable_error())
                                .and_then(|surface| {
                                    let mut bytes = Vec::new();
                                    surface
                                        .write_to_png(&mut bytes)
                                        .map_err(|_| browser_unavailable_error())?;
                                    Ok(bytes)
                                });
                        if let Ok(mut sender) = sender.lock() {
                            if let Some(sender) = sender.take() {
                                let _ = sender.send(encoded);
                            }
                        }
                    });
                    if !completed {
                        let _ = run_native_capture_callback(|| {
                            if let Ok(mut sender) = sender.lock() {
                                if let Some(sender) = sender.take() {
                                    let _ = sender.send(Err(browser_unavailable_error()));
                                }
                            }
                        });
                    }
                },
            );
        })
        .map_err(|_| browser_unavailable_error())
}

#[cfg(windows)]
pub(super) fn capture_platform_png(
    webview: &tauri::Webview,
    sender: tokio::sync::oneshot::Sender<ChatResult<Vec<u8>>>,
) -> ChatResult<()> {
    use webview2_com::{CapturePreviewCompletedHandler, Microsoft::Web::WebView2::Win32::*};
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Com::IStream;
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;

    let sender = Arc::new(Mutex::new(Some(sender)));
    webview
        .with_webview(move |platform| {
            // SAFETY: Tauri runs this callback on the WebView thread where COM is
            // initialized. A null HGLOBAL requests a new movable allocation, and
            // `true` transfers its cleanup to the returned IStream.
            let stream = match unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) } {
                Ok(stream) => stream,
                Err(_) => {
                    if let Ok(mut sender) = sender.lock() {
                        if let Some(sender) = sender.take() {
                            let _ = sender.send(Err(browser_unavailable_error()));
                        }
                    }
                    return;
                }
            };
            let callback_stream: IStream = stream.clone();
            let callback_sender = Arc::clone(&sender);
            let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
                let completed = run_native_capture_callback(|| {
                    let captured = result
                        .map_err(|_| browser_unavailable_error())
                        .and_then(|_| read_windows_stream(&callback_stream));
                    if let Ok(mut sender) = callback_sender.lock() {
                        if let Some(sender) = sender.take() {
                            let _ = sender.send(captured);
                        }
                    }
                });
                if !completed {
                    let _ = run_native_capture_callback(|| {
                        if let Ok(mut sender) = callback_sender.lock() {
                            if let Some(sender) = sender.take() {
                                let _ = sender.send(Err(browser_unavailable_error()));
                            }
                        }
                    });
                }
                Ok(())
            }));
            // SAFETY: `controller()` returns a live typed COM controller owned by
            // the platform webview. The getter initializes and returns its typed
            // CoreWebView2 interface without retaining Rust pointers.
            let core: ICoreWebView2 = match unsafe { platform.controller().CoreWebView2() } {
                Ok(core) => core,
                Err(_) => {
                    if let Ok(mut sender) = sender.lock() {
                        if let Some(sender) = sender.take() {
                            let _ = sender.send(Err(browser_unavailable_error()));
                        }
                    }
                    return;
                }
            };
            // SAFETY: `core`, `stream`, and `handler` are live typed COM
            // interfaces in the current apartment. WebView2 retains the stream
            // and handler for the asynchronous operation; both own their state.
            if unsafe {
                core.CapturePreview(
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                    &stream,
                    &handler,
                )
            }
            .is_err()
            {
                if let Ok(mut sender) = sender.lock() {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(Err(browser_unavailable_error()));
                    }
                }
            }
        })
        .map_err(|_| browser_unavailable_error())
}

#[cfg(any(windows, test))]
fn capture_stream_capacity(reported_length: u64) -> ChatResult<usize> {
    let length = usize::try_from(reported_length).map_err(|_| browser_unavailable_error())?;
    if length == 0 || length > MAX_CAPTURE_BYTES {
        return Err(browser_unavailable_error());
    }
    let _ = u32::try_from(length).map_err(|_| browser_unavailable_error())?;
    Ok(length)
}

#[cfg(any(windows, test))]
fn capture_stream_read_progress(
    reported_read: u32,
    offset: usize,
    length: usize,
) -> ChatResult<usize> {
    let remaining = length
        .checked_sub(offset)
        .ok_or_else(browser_unavailable_error)?;
    let read = usize::try_from(reported_read).map_err(|_| browser_unavailable_error())?;
    if read == 0 || read > remaining {
        return Err(browser_unavailable_error());
    }
    offset
        .checked_add(read)
        .ok_or_else(browser_unavailable_error)
}

#[cfg(windows)]
pub(super) fn read_windows_stream(
    stream: &windows::Win32::System::Com::IStream,
) -> ChatResult<Vec<u8>> {
    use windows::Win32::System::Com::{STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};
    let mut stat = STATSTG::default();
    // SAFETY: `stream` is a live typed COM interface, and `stat` is initialized
    // writable storage of the exact type required. STATFLAG_NONAME prevents COM
    // from allocating a name that the caller would need to free.
    unsafe { stream.Stat(&mut stat, STATFLAG_NONAME) }.map_err(|_| browser_unavailable_error())?;
    let length = capture_stream_capacity(stat.cbSize)?;
    // SAFETY: `stream` remains live, the seek origin and zero offset are valid,
    // and no output-position pointer is supplied.
    unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }.map_err(|_| browser_unavailable_error())?;
    let mut bytes = vec![0_u8; length];
    let mut offset = 0;
    while offset < length {
        let remaining = &mut bytes[offset..];
        let read_capacity =
            u32::try_from(remaining.len()).map_err(|_| browser_unavailable_error())?;
        let mut reported_read = 0_u32;
        // SAFETY: `remaining` is writable for exactly `read_capacity` bytes,
        // which was checked to fit COM's u32 count. `reported_read` is valid
        // writable storage, and neither pointer is retained after this call.
        unsafe {
            stream.Read(
                remaining.as_mut_ptr().cast(),
                read_capacity,
                Some(&mut reported_read),
            )
        }
        .ok()
        .map_err(|_| browser_unavailable_error())?;
        offset = capture_stream_read_progress(reported_read, offset, length)?;
    }
    Ok(bytes)
}

#[cfg(target_os = "macos")]
pub(super) fn capture_platform_png(
    webview: &tauri::Webview,
    sender: tokio::sync::oneshot::Sender<ChatResult<Vec<u8>>>,
) -> ChatResult<()> {
    use block2::RcBlock;
    use objc2_app_kit::{NSBitmapImageRep, NSImage, NSPNGFileType};
    use objc2_foundation::{NSDictionary, NSError};
    use objc2_web_kit::WKWebView;
    use std::ptr::NonNull;

    let sender = Arc::new(Mutex::new(Some(sender)));
    webview
        .with_webview(move |platform| {
            let Some(view) = NonNull::new(platform.inner().cast::<WKWebView>()) else {
                if let Ok(mut sender) = sender.lock() {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(Err(browser_unavailable_error()));
                    }
                }
                return;
            };
            // SAFETY: Tauri documents this non-null platform pointer as the live
            // WKWebView for the duration of `with_webview`. It is borrowed only
            // within this callback and is never released by this code.
            let view = unsafe { view.as_ref() };
            let callback_sender = Arc::clone(&sender);
            let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                let completed = run_native_capture_callback(|| {
                    let result = if !error.is_null() {
                        Err(browser_unavailable_error())
                    } else if let Some(image) = NonNull::new(image) {
                        // SAFETY: WebKit supplied a non-null NSImage pointer with no
                        // NSError. The callback contract keeps it alive for this
                        // invocation, so the temporary shared reference cannot escape.
                        let image = unsafe { image.as_ref() };
                        image
                            .TIFFRepresentation()
                            .and_then(|data| NSBitmapImageRep::imageRepWithData(&data))
                            .and_then(|representation| {
                                let properties = NSDictionary::new();
                                representation
                                    .representationUsingType_properties(NSPNGFileType, &properties)
                            })
                            .map(|data| data.to_vec())
                            .ok_or_else(browser_unavailable_error)
                    } else {
                        Err(browser_unavailable_error())
                    };
                    if let Ok(mut sender) = callback_sender.lock() {
                        if let Some(sender) = sender.take() {
                            let _ = sender.send(result);
                        }
                    }
                });
                if !completed {
                    let _ = run_native_capture_callback(|| {
                        if let Ok(mut sender) = callback_sender.lock() {
                            if let Some(sender) = sender.take() {
                                let _ = sender.send(Err(browser_unavailable_error()));
                            }
                        }
                    });
                }
            });
            // SAFETY: `view` is the live Tauri WKWebView established above. The
            // Objective-C block owns its captured sender and remains valid for
            // the asynchronous completion callback.
            unsafe { view.takeSnapshotWithConfiguration_completionHandler(None, &block) };
        })
        .map_err(|_| browser_unavailable_error())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_CAPTURE_BYTES, capture_stream_capacity, capture_stream_read_progress,
        run_native_capture_callback,
    };

    #[test]
    fn capture_stream_lengths_are_nonzero_bounded_and_representable() {
        assert!(capture_stream_capacity(0).is_err());
        assert_eq!(
            capture_stream_capacity(MAX_CAPTURE_BYTES as u64).unwrap(),
            MAX_CAPTURE_BYTES
        );
        assert!(capture_stream_capacity(MAX_CAPTURE_BYTES as u64 + 1).is_err());
        assert!(capture_stream_capacity(u64::MAX).is_err());
    }

    #[test]
    fn capture_stream_fills_short_reads_and_rejects_stalls_or_overruns() {
        let offset = capture_stream_read_progress(3, 0, 8).unwrap();
        assert_eq!(offset, 3);
        assert_eq!(capture_stream_read_progress(5, offset, 8).unwrap(), 8);
        assert!(capture_stream_read_progress(0, offset, 8).is_err());
        assert!(capture_stream_read_progress(6, offset, 8).is_err());
        assert!(capture_stream_read_progress(1, 9, 8).is_err());
    }

    #[test]
    fn native_capture_callback_barrier_forgets_panicking_payloads() {
        struct PanicOnDrop(std::sync::Arc<std::sync::atomic::AtomicBool>);

        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
                panic!("panic payload drop fixture");
            }
        }

        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let payload = PanicOnDrop(std::sync::Arc::clone(&dropped));
        assert!(!run_native_capture_callback(|| {
            std::panic::panic_any(payload)
        }));
        assert!(!dropped.load(std::sync::atomic::Ordering::SeqCst));
        assert!(run_native_capture_callback(|| {}));
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub(super) fn capture_platform_png(
    _webview: &tauri::Webview,
    sender: tokio::sync::oneshot::Sender<ChatResult<Vec<u8>>>,
) -> ChatResult<()> {
    let _ = sender.send(Err(ChatError::unsupported(
        "Browser capture is unavailable on this platform",
    )));
    Ok(())
}

use super::*;
use std::sync::{Arc, Mutex};

#[test]
fn media_kind_uses_extension_groups() {
    assert_eq!(media_kind_from_extension("flac"), MediaKind::Audio);
    assert_eq!(media_kind_from_extension("mkv"), MediaKind::Video);
    assert_eq!(media_kind_from_extension("txt"), MediaKind::Unknown);
}

#[test]
fn clamp_helpers_bound_user_values() {
    assert_eq!(clamp_volume(-1.0), 0.0);
    assert_eq!(clamp_volume(0.75), 0.75);
    assert_eq!(clamp_volume(1.25), 1.0);
    assert_eq!(clamp_volume(2.0), 1.0);
    assert_eq!(clamp_rate(0.1), 0.25);
    assert_eq!(clamp_rate(4.0), 2.0);
}

#[test]
fn local_file_validation_rejects_relative_paths() {
    let error = validate_local_file_path(Path::new("song.mp3")).unwrap_err();
    assert_eq!(error.code, "invalidSource");
}

#[test]
fn media_title_omits_file_extension() {
    assert_eq!(
        media_title_from_path(Path::new("/music/01 - Made in Abyss.mp3")),
        "01 - Made in Abyss"
    );
}

#[test]
fn mp4_duration_conversion_rounds_to_milliseconds() {
    assert_eq!(duration_units_to_ms(62_561, 1_000), 62_561);
    assert_eq!(duration_units_to_ms(1, 3), 333);
}

#[test]
fn mp4_probe_reads_leading_empty_edit_start() {
    let path = std::env::temp_dir().join(format!("ganbaru-media-probe-{}.mp4", std::process::id()));
    std::fs::write(&path, minimal_mp4_with_leading_empty_edit(62_561)).unwrap();

    let start_ms = probe_mp4_leading_empty_edit_ms(&path);

    let _ = std::fs::remove_file(&path);
    assert_eq!(start_ms, Some(62_561));
}

#[test]
fn mp4_probe_reads_only_the_edit_entries_inside_the_elst_box() {
    let mut elst = vec![0, 0, 0, 0];
    elst.extend_from_slice(&u32::MAX.to_be_bytes());
    for duration in [500_u32, 700] {
        elst.extend_from_slice(&duration.to_be_bytes());
        elst.extend_from_slice(&(-1_i32).to_be_bytes());
        elst.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    }
    let mut trailing = Vec::new();
    for _ in 0..64 {
        trailing.extend_from_slice(&9_000_u32.to_be_bytes());
        trailing.extend_from_slice(&(-1_i32).to_be_bytes());
        trailing.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    }
    let mut mvhd = vec![0, 0, 0, 0];
    mvhd.extend_from_slice(&0_u32.to_be_bytes());
    mvhd.extend_from_slice(&0_u32.to_be_bytes());
    mvhd.extend_from_slice(&1_000_u32.to_be_bytes());
    mvhd.extend_from_slice(&128_000_u32.to_be_bytes());
    let trak = mp4_box(*b"trak", mp4_box(*b"edts", mp4_box(*b"elst", elst)));
    let movie = [
        mp4_box(*b"moov", [mp4_box(*b"mvhd", mvhd), trak].concat()),
        mp4_box(*b"free", trailing),
    ]
    .concat();
    let path = std::env::temp_dir().join(format!(
        "ganbaru-media-probe-bounded-{}.mp4",
        std::process::id()
    ));
    std::fs::write(&path, movie).unwrap();

    let start_ms = probe_mp4_leading_empty_edit_ms(&path);

    let _ = std::fs::remove_file(&path);
    assert_eq!(start_ms, Some(1_200));
}

#[test]
fn mp4_probe_stops_after_the_sibling_box_cap() {
    let mut children = Vec::new();
    for _ in 0..MAX_MP4_SIBLING_BOXES {
        children.extend_from_slice(&mp4_box(*b"free", Vec::new()));
    }
    children.extend_from_slice(&minimal_mp4_with_leading_empty_edit(250));
    let path = std::env::temp_dir().join(format!(
        "ganbaru-media-probe-siblings-{}.mp4",
        std::process::id()
    ));
    std::fs::write(&path, children).unwrap();

    let start_ms = probe_mp4_leading_empty_edit_ms(&path);

    let _ = std::fs::remove_file(&path);
    assert_eq!(start_ms, None);
}

#[test]
fn player_core_loads_and_updates_snapshot() {
    let mut core = test_core();
    let snapshot = core
        .handle(BackendCommand::load(
            local_load_request(Some(1_500)),
            audio_probe(),
        ))
        .unwrap();

    assert_eq!(snapshot.status, PlayerStatus::Ready);
    assert_eq!(snapshot.position_ms, 1_500);
    assert_eq!(snapshot.volume, 0.75);
    assert!(!snapshot.muted);
    assert_eq!(snapshot.rate, 1.5);
    assert_eq!(snapshot.title.as_deref(), Some("Song title"));
    assert!(!snapshot.has_video);
    assert_eq!(snapshot.backend_kind, BackendKind::Rodio);
    assert_eq!(snapshot.playable_start_ms, None);
    assert_eq!(snapshot.duration_ms, Some(120_000));
}

#[test]
fn player_core_pause_is_state_only_and_clears_errors() {
    let mut core = loaded_core();
    let playing = core.handle(BackendCommand::Play).unwrap();
    assert_eq!(playing.status, PlayerStatus::Playing);

    let snapshot = core.handle(BackendCommand::Pause).unwrap();
    assert_eq!(snapshot.status, PlayerStatus::Paused);
    assert_eq!(snapshot.error, None);
}

#[test]
fn player_core_stop_releases_source_state_and_keeps_settings() {
    let mut core = loaded_core();
    core.handle(BackendCommand::SetMuted(true)).unwrap();
    let snapshot = core.handle(BackendCommand::Stop).unwrap();

    assert_eq!(snapshot.status, PlayerStatus::Idle);
    assert_eq!(snapshot.source_identity, None);
    assert_eq!(snapshot.position_ms, 0);
    assert_eq!(snapshot.volume, 0.75);
    assert!(snapshot.muted);
    assert_eq!(snapshot.rate, 1.5);
}

#[test]
fn player_core_reloading_stops_previous_audio_backend() {
    let loaded_backends = Arc::new(Mutex::new(Vec::new()));
    let mut core = PlayerCore::with_audio_factory(Box::new(RecordingAudioFactory {
        loaded_backends: Arc::clone(&loaded_backends),
    }));
    core.handle(BackendCommand::load(
        local_load_request(None),
        audio_probe(),
    ))
    .unwrap();
    core.handle(BackendCommand::load(
        local_load_request(Some(5_000)),
        audio_probe(),
    ))
    .unwrap();

    let backends = loaded_backends.lock().unwrap();
    assert_eq!(backends.len(), 2);
    assert!(backends[0].lock().unwrap().stopped);
    assert!(!backends[1].lock().unwrap().stopped);
}

#[test]
fn player_core_uses_webview_for_video() {
    let mut core = test_core();
    let loaded = core
        .handle(BackendCommand::load(
            local_load_request(None),
            video_probe(),
        ))
        .unwrap();
    assert_eq!(loaded.status, PlayerStatus::Ready);
    assert!(loaded.has_video);
    assert_eq!(loaded.playable_start_ms, Some(62_561));
    assert_eq!(loaded.backend_kind, BackendKind::Webview);
    assert_eq!(loaded.error, None);
}

#[test]
fn player_core_muting_does_not_change_volume() {
    let mut core = loaded_core();
    let muted = core.handle(BackendCommand::SetMuted(true)).unwrap();
    assert!(muted.muted);
    assert_eq!(muted.volume, 0.75);

    let unmuted = core.handle(BackendCommand::SetMuted(false)).unwrap();
    assert!(!unmuted.muted);
    assert_eq!(unmuted.volume, 0.75);
}

#[test]
fn playback_controller_dispatches_on_worker_thread() {
    let controller = PlaybackController::with_core(test_core());
    let loaded = controller
        .dispatch(BackendCommand::load(
            local_load_request(None),
            audio_probe(),
        ))
        .unwrap();
    assert_eq!(loaded.status, PlayerStatus::Ready);

    let seeked = controller.dispatch(BackendCommand::Seek(42_000)).unwrap();
    assert_eq!(seeked.position_ms, 42_000);

    let snapshot = controller.dispatch(BackendCommand::Snapshot).unwrap();
    assert_eq!(snapshot.position_ms, 42_000);
}

#[test]
fn revoked_delivery_cannot_start_after_blocking_decode() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    let directory = TestMediaDirectory::new();
    let path = directory.path().join("song.mp3");
    std::fs::write(&path, b"fake decoder input").unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let (started, admission) = mpsc::sync_channel(1);
    let (release, blocked) = mpsc::sync_channel(1);
    let mut core = PlayerCore::with_audio_factory(Box::new(RecordingAudioFactory {
        loaded_backends: recorded.clone(),
    }));
    core.handle(BackendCommand::load(
        local_load_request(None),
        audio_probe(),
    ))
    .unwrap();
    core.handle(BackendCommand::Play).unwrap();
    core.audio_factory = Box::new(BlockingAudioFactory {
        started,
        blocked,
        recorded: recorded.clone(),
    });
    let controller = Arc::new(PlaybackController::with_core(core));
    let current = Arc::new(AtomicBool::new(true));
    let guard = current.clone();
    let task_controller = controller.clone();
    let effect = native_load_effect(&path);
    let task = std::thread::spawn(move || {
        task_controller.dispatch_authorized(
            BackendCommand::Session(Box::new(effect)),
            Arc::new(move || guard.load(Ordering::Acquire)),
        )
    });
    admission.recv_timeout(Duration::from_secs(2)).unwrap();
    let prior = recorded.lock().unwrap()[0].clone();
    assert!(prior.lock().unwrap().stopped);
    current.store(false, Ordering::Release);
    release.send(()).unwrap();
    assert_eq!(task.join().unwrap().unwrap_err().code, "deliveryRevoked");
    let backend = recorded.lock().unwrap()[1].clone();
    let backend = backend.lock().unwrap();
    assert_eq!(backend.play_count, 0);
    assert!(backend.stopped);
    drop(backend);
    assert_eq!(
        controller
            .dispatch(BackendCommand::Snapshot)
            .unwrap()
            .status,
        PlayerStatus::Idle
    );
}

#[test]
fn decoder_timeout_retains_admission_until_actual_completion() {
    use std::sync::mpsc;
    let directory = TestMediaDirectory::new();
    let path = directory.path().join("song.mp3");
    std::fs::write(&path, b"fake decoder input").unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let (started, admission) = mpsc::sync_channel(1);
    let (release, blocked) = mpsc::sync_channel(1);
    let controller = Arc::new(PlaybackController::with_core(
        PlayerCore::with_audio_factory(Box::new(BlockingAudioFactory {
            started,
            blocked,
            recorded: recorded.clone(),
        })),
    ));
    let task_controller = controller.clone();
    let effect = native_load_effect(&path);
    let task = std::thread::spawn(move || {
        task_controller.dispatch_with_timeout(
            BackendCommand::Session(Box::new(effect)),
            Arc::new(|| true),
            Duration::from_millis(100),
        )
    });
    admission.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(task.join().unwrap().unwrap_err().code, "backendTimeout");
    for _ in 0..100 {
        assert_eq!(
            controller.dispatch(BackendCommand::Stop).unwrap_err().code,
            "backendBusy"
        );
    }
    assert!(recorded.lock().unwrap().is_empty());
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match controller.dispatch(BackendCommand::Snapshot) {
            Ok(snapshot) => {
                assert_eq!(snapshot.status, PlayerStatus::Idle);
                break;
            }
            Err(error) if error.code == "backendBusy" && std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1));
            }
            result => panic!("Decoder did not finish cancellation: {result:?}"),
        }
    }
    let backend = recorded.lock().unwrap()[0].clone();
    let backend = backend.lock().unwrap();
    assert_eq!(backend.play_count, 0);
    assert!(backend.stopped);
    drop(backend);
    assert_eq!(
        controller.dispatch(BackendCommand::Stop).unwrap().status,
        PlayerStatus::Idle
    );
}

#[test]
fn dropping_controller_does_not_wait_for_a_stalled_source() {
    use std::sync::mpsc;
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let (started, admission) = mpsc::sync_channel(1);
    let (release, blocked) = mpsc::sync_channel(1);
    let controller = Arc::new(PlaybackController::with_core(
        PlayerCore::with_audio_factory(Box::new(BlockingAudioFactory {
            started,
            blocked,
            recorded: recorded.clone(),
        })),
    ));
    let task_controller = controller.clone();
    let task = std::thread::spawn(move || {
        task_controller.dispatch_with_timeout(
            BackendCommand::load(local_load_request(None), audio_probe()),
            Arc::new(|| true),
            Duration::from_millis(100),
        )
    });
    admission.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(task.join().unwrap().unwrap_err().code, "backendTimeout");
    let (dropped, completion) = mpsc::sync_channel(1);
    let task = std::thread::spawn(move || {
        drop(controller);
        dropped.send(()).unwrap();
    });
    let result = completion.recv_timeout(Duration::from_secs(2));
    release.send(()).unwrap();
    result.unwrap();
    task.join().unwrap();
}

#[test]
fn committed_settings_apply_together_without_starting_a_paused_decoder() {
    let mut core = loaded_core();
    let result = core
        .handle(BackendCommand::Session(Box::new(SessionEffect::Settings {
            generation: 1,
            volume: 0.5,
            muted: true,
            rate: 0.75,
        })))
        .unwrap();
    assert_eq!(result.status, PlayerStatus::Ready);
    assert_eq!(result.volume, 0.5);
    assert!(result.muted);
    assert_eq!(result.rate, 0.75);
}

fn native_load_effect(path: &Path) -> SessionEffect {
    use ganbaru_music_library::session::models::{SessionBackend, SessionSource, SourceKind};
    SessionEffect::Load {
        session_id: "session".into(),
        generation: 1,
        source: Box::new(SessionSource {
            kind: SourceKind::LocalFile,
            identity: "local:song".into(),
            original_input: "song".into(),
            title: "Song".into(),
            artist: None,
            album: None,
            path: Some(path.to_string_lossy().into_owned()),
            artwork_path: None,
            video_id: None,
            playlist_id: None,
            start_ms: None,
            end_ms: None,
        }),
        backend: SessionBackend::NativeAudio,
        position_ms: 0,
        autoplay: true,
        volume: 0.75,
        muted: false,
        rate: 1.0,
    }
}

struct BlockingAudioFactory {
    started: std::sync::mpsc::SyncSender<()>,
    blocked: std::sync::mpsc::Receiver<()>,
    recorded: Arc<Mutex<Vec<Arc<Mutex<FakeAudioState>>>>>,
}

struct TestMediaDirectory(PathBuf);

impl TestMediaDirectory {
    fn new() -> Self {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ganbaru-music-worker-{}-{timestamp}-{sequence}",
            std::process::id(),
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestMediaDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("Remove Music test directory {}: {error}", self.0.display());
        }
    }
}

impl LocalAudioFactory for BlockingAudioFactory {
    fn load(
        &mut self,
        request: &LoadRequest,
        volume: f64,
        muted: bool,
        rate: f64,
    ) -> Result<Box<dyn LocalAudioBackend>, MediaPlayerError> {
        self.started
            .send(())
            .map_err(|_| MediaPlayerError::backend_thread())?;
        self.blocked
            .recv()
            .map_err(|_| MediaPlayerError::backend_thread())?;
        RecordingAudioFactory {
            loaded_backends: self.recorded.clone(),
        }
        .load(request, volume, muted, rate)
    }
}

fn test_core() -> PlayerCore {
    PlayerCore::with_audio_factory(Box::new(FakeAudioFactory))
}

fn loaded_core() -> PlayerCore {
    let mut core = test_core();
    core.handle(BackendCommand::load(
        local_load_request(None),
        audio_probe(),
    ))
    .unwrap();
    core
}

fn local_load_request(start_ms: Option<u64>) -> LoadRequest {
    LoadRequest {
        source: LocalMediaSource {
            kind: "local-file".to_string(),
            path: "/music/song.mp3".to_string(),
            identity: "local:/music/song.mp3".to_string(),
            title: Some("Song title".to_string()),
        },
        start_ms,
        volume: Some(0.75),
        rate: Some(1.5),
    }
}

fn audio_probe() -> MediaProbe {
    MediaProbe {
        path: "/music/song.mp3".to_string(),
        title: "song".to_string(),
        file_size_bytes: 1024,
        extension: Some("mp3".to_string()),
        media_kind: MediaKind::Audio,
        playable_start_ms: None,
    }
}

fn video_probe() -> MediaProbe {
    MediaProbe {
        path: "/music/video.mp4".to_string(),
        title: "video".to_string(),
        file_size_bytes: 1024,
        extension: Some("mp4".to_string()),
        media_kind: MediaKind::Video,
        playable_start_ms: Some(62_561),
    }
}

fn minimal_mp4_with_leading_empty_edit(empty_duration: u32) -> Vec<u8> {
    let mut mvhd = vec![0, 0, 0, 0];
    mvhd.extend_from_slice(&0_u32.to_be_bytes());
    mvhd.extend_from_slice(&0_u32.to_be_bytes());
    mvhd.extend_from_slice(&1_000_u32.to_be_bytes());
    mvhd.extend_from_slice(&128_000_u32.to_be_bytes());

    let mut elst = vec![0, 0, 0, 0];
    elst.extend_from_slice(&2_u32.to_be_bytes());
    elst.extend_from_slice(&empty_duration.to_be_bytes());
    elst.extend_from_slice(&(-1_i32).to_be_bytes());
    elst.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    elst.extend_from_slice(&65_000_u32.to_be_bytes());
    elst.extend_from_slice(&0_i32.to_be_bytes());
    elst.extend_from_slice(&0x0001_0000_u32.to_be_bytes());

    let elst = mp4_box(*b"elst", elst);
    let edts = mp4_box(*b"edts", elst);
    let trak = mp4_box(*b"trak", edts);
    mp4_box(*b"moov", [mp4_box(*b"mvhd", mvhd), trak].concat())
}

fn mp4_box(name: [u8; 4], payload: Vec<u8>) -> Vec<u8> {
    let size = u32::try_from(payload.len() + 8).unwrap();
    let mut bytes = Vec::with_capacity(size as usize);
    bytes.extend_from_slice(&size.to_be_bytes());
    bytes.extend_from_slice(&name);
    bytes.extend_from_slice(&payload);
    bytes
}

struct FakeAudioFactory;

impl LocalAudioFactory for FakeAudioFactory {
    fn load(
        &mut self,
        request: &LoadRequest,
        volume: f64,
        muted: bool,
        rate: f64,
    ) -> Result<Box<dyn LocalAudioBackend>, MediaPlayerError> {
        Ok(Box::new(FakeAudioBackend {
            state: Arc::new(Mutex::new(FakeAudioState {
                position_ms: request.start_ms.unwrap_or(0),
                duration_ms: Some(120_000),
                volume,
                muted,
                rate,
                playing: false,
                play_count: 0,
                stopped: false,
                empty: false,
            })),
        }))
    }
}

struct RecordingAudioFactory {
    loaded_backends: Arc<Mutex<Vec<Arc<Mutex<FakeAudioState>>>>>,
}

impl LocalAudioFactory for RecordingAudioFactory {
    fn load(
        &mut self,
        request: &LoadRequest,
        volume: f64,
        muted: bool,
        rate: f64,
    ) -> Result<Box<dyn LocalAudioBackend>, MediaPlayerError> {
        let state = Arc::new(Mutex::new(FakeAudioState {
            position_ms: request.start_ms.unwrap_or(0),
            duration_ms: Some(120_000),
            volume,
            muted,
            rate,
            playing: false,
            play_count: 0,
            stopped: false,
            empty: false,
        }));
        self.loaded_backends
            .lock()
            .unwrap()
            .push(Arc::clone(&state));
        Ok(Box::new(FakeAudioBackend { state }))
    }
}

struct FakeAudioBackend {
    state: Arc<Mutex<FakeAudioState>>,
}

struct FakeAudioState {
    position_ms: u64,
    duration_ms: Option<u64>,
    volume: f64,
    muted: bool,
    rate: f64,
    playing: bool,
    play_count: usize,
    stopped: bool,
    empty: bool,
}

impl LocalAudioBackend for FakeAudioBackend {
    fn play(&mut self) {
        let mut state = self.state.lock().unwrap();
        state.playing = true;
        state.play_count += 1;
    }

    fn pause(&mut self) {
        let mut state = self.state.lock().unwrap();
        state.playing = false;
    }

    fn stop(&mut self) {
        let mut state = self.state.lock().unwrap();
        state.stopped = true;
        state.playing = false;
    }

    fn seek(&mut self, position_ms: u64) -> Result<(), MediaPlayerError> {
        let mut state = self.state.lock().unwrap();
        state.position_ms = position_ms;
        Ok(())
    }

    fn set_volume(&mut self, volume: f64, muted: bool) {
        let mut state = self.state.lock().unwrap();
        state.volume = volume;
        state.muted = muted;
    }

    fn set_rate(&mut self, rate: f64) {
        let mut state = self.state.lock().unwrap();
        state.rate = rate;
    }

    fn position_ms(&self) -> u64 {
        self.state.lock().unwrap().position_ms
    }

    fn duration_ms(&self) -> Option<u64> {
        self.state.lock().unwrap().duration_ms
    }

    fn is_empty(&self) -> bool {
        self.state.lock().unwrap().empty
    }
}

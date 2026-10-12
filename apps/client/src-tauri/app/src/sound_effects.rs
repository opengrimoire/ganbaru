//! Bundled app sound effects played on a dedicated audio worker.

use rodio::{
    ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source,
};
use std::io::Cursor;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TrySendError},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

const APP_SOUND_COMMAND_QUEUE_LIMIT: usize = 32;
const APP_SOUND_PLAYER_QUEUE_LIMIT: usize = 8;
const APP_SOUND_OUTPUT_SAMPLE_RATE_HZ: u32 = 48_000;
const APP_SOUND_OUTPUT_CHANNELS: u16 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppSound {
    EventNotification,
    IdleAlert,
    FocusSessionFailedLongIdle,
    FocusEndingWarning,
    BreakStart,
    BreakFinished,
    EventFinished,
    PomodoroDayComplete,
    PomodoroWorkweekComplete,
}

impl AppSound {
    fn id(self) -> &'static str {
        match self {
            Self::EventNotification => "event-notification",
            Self::IdleAlert => "idle-alert",
            Self::FocusSessionFailedLongIdle => "focus-session-failed-long-idle",
            Self::FocusEndingWarning => "focus-ending-warning",
            Self::BreakStart => "break-start",
            Self::BreakFinished => "break-finished",
            Self::EventFinished => "event-finished",
            Self::PomodoroDayComplete => "pomodoro-day-complete",
            Self::PomodoroWorkweekComplete => "pomodoro-workweek-complete",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::EventNotification => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../static/sfx/event-notification.wav"
            )),
            Self::IdleAlert => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../static/sfx/idle-alert.wav"
            )),
            Self::FocusSessionFailedLongIdle => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../static/sfx/focus-session-failed-long-idle.wav"
            )),
            Self::FocusEndingWarning => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../static/sfx/focus-ending-warning.wav"
            )),
            Self::BreakStart => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../static/sfx/break-start.wav"
            )),
            Self::BreakFinished => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../static/sfx/break-finished.wav"
            )),
            Self::EventFinished => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../static/sfx/event-finished.wav"
            )),
            Self::PomodoroDayComplete => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../static/sfx/pomodoro-day-complete.wav"
            )),
            Self::PomodoroWorkweekComplete => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../static/sfx/pomodoro-workweek-complete.wav"
            )),
        }
    }
}

pub(crate) struct AppSoundState {
    controller: Mutex<Option<AppSoundController>>,
}

impl Default for AppSoundState {
    fn default() -> Self {
        Self {
            controller: Mutex::new(None),
        }
    }
}

impl AppSoundState {
    pub(crate) fn play(&self, sound: AppSound) {
        self.handle().play(sound);
    }

    /// Reports actual source completion without sleeping on the audio worker or guessing asset lengths.
    pub(crate) fn play_focus_completion(
        &self,
        sound: AppSound,
        app: tauri::AppHandle,
        scope: (u64, i64),
        expires_at: Instant,
    ) -> Result<oneshot::Receiver<Result<(), String>>, String> {
        let (reply, response) = oneshot::channel();
        self.handle()
            .sender
            .try_send(AppSoundMessage::FocusCompletion {
                sound,
                app,
                scope,
                expires_at,
                reply,
            })
            .map_err(|error| format!("Enqueue Focus completion audio: {error}"))?;
        Ok(response)
    }

    fn handle(&self) -> AppSoundHandle {
        let mut controller = self
            .controller
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if controller.is_none() {
            *controller = Some(AppSoundController::new());
        }
        controller
            .as_ref()
            .expect("app sound controller should exist")
            .handle()
    }
}

impl Drop for AppSoundState {
    fn drop(&mut self) {
        if let Ok(mut controller) = self.controller.lock() {
            controller.take();
        }
    }
}

#[derive(Clone)]
struct AppSoundHandle {
    sender: SyncSender<AppSoundMessage>,
}

impl AppSoundHandle {
    fn play(&self, sound: AppSound) {
        match self.sender.try_send(AppSoundMessage::Play(sound)) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                eprintln!("Dropped app sound {} because the queue is full", sound.id());
            }
            Err(TrySendError::Disconnected(_)) => {
                eprintln!(
                    "Dropped app sound {} because the audio worker stopped",
                    sound.id()
                );
            }
        }
    }
}

struct AppSoundController {
    sender: Option<SyncSender<AppSoundMessage>>,
    worker: Option<JoinHandle<()>>,
}

impl AppSoundController {
    fn new() -> Self {
        let (sender, receiver) = mpsc::sync_channel(APP_SOUND_COMMAND_QUEUE_LIMIT);
        let worker = std::thread::Builder::new()
            .name("ganbaru-ai-app-sounds".to_string())
            .spawn(move || app_sound_worker(receiver))
            .expect("failed to start app sound worker thread");
        Self {
            sender: Some(sender),
            worker: Some(worker),
        }
    }

    fn handle(&self) -> AppSoundHandle {
        AppSoundHandle {
            sender: self
                .sender
                .as_ref()
                .expect("app sound controller sender should exist")
                .clone(),
        }
    }
}

impl Drop for AppSoundController {
    fn drop(&mut self) {
        let shutdown_sent = self
            .sender
            .take()
            .map(|sender| {
                let sent = match sender.try_send(AppSoundMessage::Shutdown) {
                    Ok(()) => true,
                    Err(TrySendError::Full(_)) => false,
                    Err(TrySendError::Disconnected(_)) => true,
                };
                drop(sender);
                sent
            })
            .unwrap_or(true);

        if shutdown_sent {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
}

enum AppSoundMessage {
    Play(AppSound),
    FocusCompletion {
        sound: AppSound,
        app: tauri::AppHandle,
        scope: (u64, i64),
        expires_at: Instant,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Shutdown,
}

/// The mixer drops this source on completion, interruption, or output replacement.
struct CompletionSource<S: Source> {
    source: S,
    reply: Option<oneshot::Sender<Result<(), String>>>,
}

impl<S: Source> Iterator for CompletionSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.source.next();
        if sample.is_none()
            && let Some(reply) = self.reply.take()
        {
            let _ = reply.send(Ok(()));
        }
        sample
    }
}

impl<S: Source> Source for CompletionSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }
    fn channels(&self) -> ChannelCount {
        self.source.channels()
    }
    fn sample_rate(&self) -> SampleRate {
        self.source.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
}

impl<S: Source> Drop for CompletionSource<S> {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            let result = if self.source.is_exhausted() {
                Ok(())
            } else {
                Err("Focus completion audio was interrupted".into())
            };
            let _ = reply.send(result);
        }
    }
}

struct AppSoundOutput {
    _sink: MixerDeviceSink,
    player: Player,
    stream_failed: Arc<AtomicBool>,
}

impl AppSoundOutput {
    fn open() -> Result<Self, String> {
        let stream_failed = Arc::new(AtomicBool::new(false));
        let stream_failed_for_callback = stream_failed.clone();
        let mut sink = DeviceSinkBuilder::from_default_device()
            .map(|builder| {
                builder
                    .with_sample_rate(nonzero_sample_rate(APP_SOUND_OUTPUT_SAMPLE_RATE_HZ))
                    .with_channels(nonzero_channels(APP_SOUND_OUTPUT_CHANNELS))
                    .with_error_callback(move |e| {
                        stream_failed_for_callback.store(true, Ordering::Release);
                        eprintln!("App sound audio output error: {e}");
                    })
            })
            .and_then(|builder| builder.open_sink_or_fallback())
            .map_err(|e| format!("open audio output for app sounds: {e}"))?;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        Ok(Self {
            _sink: sink,
            player,
            stream_failed,
        })
    }

    fn should_reopen(&self) -> bool {
        self.stream_failed.load(Ordering::Acquire)
    }
}

fn app_sound_worker(receiver: Receiver<AppSoundMessage>) {
    let mut output: Option<AppSoundOutput> = None;
    while let Ok(message) = receiver.recv() {
        match message {
            AppSoundMessage::Play(sound) => {
                if let Err(e) = queue_app_sound(&mut output, sound) {
                    output = None;
                    eprintln!("Failed to play app sound: {e}");
                }
            }
            AppSoundMessage::FocusCompletion {
                sound,
                app,
                scope,
                expires_at,
                reply,
            } => {
                let prepared = (|| {
                    if Instant::now() >= expires_at
                        || !crate::pomodoro::native_runtime::presentation_is_current(
                            &app, scope.0, scope.1,
                        )
                    {
                        return Err("Focus completion audio was superseded or expired".into());
                    }
                    if output.as_ref().is_some_and(AppSoundOutput::should_reopen) {
                        output = None;
                    }
                    if output.is_none() {
                        output = Some(AppSoundOutput::open()?);
                    }
                    let decoder = Decoder::try_from(Cursor::new(sound.bytes()))
                        .map_err(|error| format!("Decode Focus completion audio: {error}"))?;
                    Ok::<_, String>(decoder)
                })();
                match prepared {
                    Ok(source) => {
                        let player = &output.as_ref().expect("prepared audio output").player;
                        // Completion replaces queued reminders instead of waiting behind them.
                        player.clear();
                        player.append(CompletionSource {
                            source,
                            reply: Some(reply),
                        });
                    }
                    Err(error) => {
                        eprintln!("Focus completion audio: {error}");
                        let _ = reply.send(Err(error));
                    }
                }
            }
            AppSoundMessage::Shutdown => break,
        }
    }
}

fn queue_app_sound(output: &mut Option<AppSoundOutput>, sound: AppSound) -> Result<(), String> {
    if output.as_ref().is_some_and(AppSoundOutput::should_reopen) {
        *output = None;
    }

    if output.is_none() {
        *output = Some(AppSoundOutput::open()?);
    }

    let output = output
        .as_ref()
        .expect("app sound output should exist after opening");
    if output.player.len() >= APP_SOUND_PLAYER_QUEUE_LIMIT {
        eprintln!(
            "Dropped app sound {} because too many sounds are queued",
            sound.id()
        );
        return Ok(());
    }

    let decoder = Decoder::try_from(Cursor::new(sound.bytes()))
        .map_err(|e| format!("decode app sound {}: {e}", sound.id()))?;
    output.player.append(decoder);
    Ok(())
}

fn nonzero_sample_rate(value: u32) -> SampleRate {
    SampleRate::new(value).expect("sample rate must be greater than zero")
}

fn nonzero_channels(value: u16) -> ChannelCount {
    ChannelCount::new(value).expect("channel count must be greater than zero")
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    fn source() -> (
        CompletionSource<rodio::buffer::SamplesBuffer>,
        oneshot::Receiver<Result<(), String>>,
    ) {
        let (reply, response) = oneshot::channel();
        (
            CompletionSource {
                source: rodio::buffer::SamplesBuffer::new(
                    nonzero_channels(1),
                    nonzero_sample_rate(2),
                    vec![0.0, 0.5],
                ),
                reply: Some(reply),
            },
            response,
        )
    }

    #[test]
    fn completion_audio_signals_actual_exhaustion_once_and_preserves_source_metadata() {
        let (mut source, mut response) = source();
        assert_eq!(source.total_duration(), Some(Duration::from_secs(1)));
        assert_eq!(source.channels().get(), 1);
        assert_eq!(source.sample_rate().get(), 2);
        assert_eq!(source.next(), Some(0.0));
        assert!(matches!(
            response.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(source.next(), Some(0.5));
        assert_eq!(source.next(), None);
        assert_eq!(response.try_recv(), Ok(Ok(())));
        assert_eq!(source.next(), None);
    }

    #[test]
    fn output_replacement_reports_interruption_instead_of_leaving_music_silent() {
        let (mut source, mut response) = source();
        source.next();
        drop(source);
        assert!(
            response
                .try_recv()
                .unwrap()
                .unwrap_err()
                .contains("interrupted")
        );
    }
}

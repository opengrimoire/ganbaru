use std::{
    fs::File,
    io::BufReader,
    path::Path,
    sync::{Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use ganbaru_music_library::soundscapes::MAX_SOUNDSCAPE_LAYERS;
use rodio::{
    ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source,
    mixer::Mixer,
};
use serde::{Deserialize, Serialize};
use tauri::Manager;

const OUTPUT_SAMPLE_RATE: u32 = 48_000;
const OUTPUT_CHANNELS: u16 = 2;
const RAMP_MILLIS: u64 = 40;
const RAMP_STEPS: u64 = 8;
const NOISE_AMPLITUDE: f32 = 0.22;
/// How long a freshly started soundscape takes to rise from silence, so choosing one does not start abruptly.
/// Layer changes in a playing mix glide over the same time, so linear fades in and out keep the sum steady.
const START_FADE: Duration = Duration::from_millis(2_500);
/// How often the engine worker moves gliding layer volumes.
const GLIDE_TICK: Duration = Duration::from_millis(20);
/// Longest a command waits for the engine worker before reporting the engine unavailable.
const ENGINE_REPLY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum GeneratedNoiseKind {
    White,
    Pink,
    Brown,
}

/// One preset to play: its first sound, the remaining sounds, and the preset level.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SoundscapeStartRequest {
    pub source_id: String,
    pub generated_kind: Option<GeneratedNoiseKind>,
    pub local_path: Option<String>,
    /// Share of the preset level this sound plays at, from 0 to 1.
    pub level: f64,
    #[serde(default)]
    pub extra_sources: Vec<SoundscapeLayer>,
    /// Multiplier of the Music volume setting, from 0 to 2.
    pub preset_level: f64,
    /// Changes a playing mix in place: layers that keep playing stay audible, new ones fade in, and dropped ones
    /// fade out. Without output to change, the request starts from silence as usual.
    #[serde(default)]
    pub keep_playing_layers: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SoundscapeLayer {
    pub source_id: String,
    pub generated_kind: Option<GeneratedNoiseKind>,
    pub local_path: Option<String>,
    /// Share of the preset level this sound plays at, from 0 to 1.
    pub level: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SoundscapeStatus {
    Idle,
    Playing,
    Paused,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SoundscapeSnapshot {
    pub status: SoundscapeStatus,
    pub source_id: Option<String>,
    pub error_code: Option<String>,
}

impl Default for SoundscapeSnapshot {
    fn default() -> Self {
        Self {
            status: SoundscapeStatus::Idle,
            source_id: None,
            error_code: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SoundscapeError {
    code: String,
    message: String,
}

impl SoundscapeError {
    fn validation(message: impl Into<String>) -> Self {
        Self {
            code: "validation".into(),
            message: message.into(),
        }
    }

    fn audio_device() -> Self {
        Self {
            code: "audio-device".into(),
            message: "The soundscape output is unavailable.".into(),
        }
    }

    fn local_file() -> Self {
        Self {
            code: "local-file".into(),
            message: "The selected background audio file is unavailable or unsupported.".into(),
        }
    }

    fn backend() -> Self {
        Self {
            code: "backend".into(),
            message: "The soundscape engine is unavailable.".into(),
        }
    }
}

struct NoiseSource {
    kind: GeneratedNoiseKind,
    random_state: u64,
    channel: u16,
    frame: u64,
    current_sample: f32,
    pink_rows: [f32; 7],
    pink_counter: u32,
    brown_value: f32,
    brown_mean: f32,
}

impl NoiseSource {
    fn new(kind: GeneratedNoiseKind, seed: u64) -> Self {
        Self {
            kind,
            random_state: seed.max(1),
            channel: 0,
            frame: 0,
            current_sample: 0.0,
            pink_rows: [0.0; 7],
            pink_counter: 0,
            brown_value: 0.0,
            brown_mean: 0.0,
        }
    }

    fn white(&mut self) -> f32 {
        self.random_state = self
            .random_state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let unit = (self.random_state >> 40) as f32 / ((1_u32 << 24) - 1) as f32;
        unit.mul_add(2.0, -1.0)
    }

    fn next_mono(&mut self) -> f32 {
        let white = self.white();
        let shaped = match self.kind {
            GeneratedNoiseKind::White => white,
            GeneratedNoiseKind::Pink => {
                self.pink_counter = self.pink_counter.wrapping_add(1);
                let row = self.pink_counter.trailing_zeros().min(6) as usize;
                self.pink_rows[row] = white;
                (self.pink_rows.iter().sum::<f32>() + white) / 8.0
            }
            GeneratedNoiseKind::Brown => {
                self.brown_value = (self.brown_value * 0.995 + white * 0.055).clamp(-1.0, 1.0);
                self.brown_mean = self.brown_mean * 0.9995 + self.brown_value * 0.0005;
                (self.brown_value - self.brown_mean) * 0.8
            }
        };
        let ramp_frames = u64::from(OUTPUT_SAMPLE_RATE) * RAMP_MILLIS / 1_000;
        let ramp = (self.frame as f32 / ramp_frames.max(1) as f32).clamp(0.0, 1.0);
        (shaped * NOISE_AMPLITUDE * ramp).clamp(-NOISE_AMPLITUDE, NOISE_AMPLITUDE)
    }
}

impl Iterator for NoiseSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.channel == 0 {
            self.current_sample = self.next_mono();
        }
        let sample = self.current_sample;
        self.channel += 1;
        if self.channel == OUTPUT_CHANNELS {
            self.channel = 0;
            self.frame = self.frame.saturating_add(1);
        }
        Some(sample)
    }
}

impl Source for NoiseSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        nonzero_channels(OUTPUT_CHANNELS)
    }
    fn sample_rate(&self) -> SampleRate {
        nonzero_sample_rate(OUTPUT_SAMPLE_RATE)
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct StreamingLoopSource {
    path: String,
    decoder: Decoder<BufReader<File>>,
    channels: ChannelCount,
    sample_rate: SampleRate,
}

impl StreamingLoopSource {
    fn open(path: &str) -> Result<Self, SoundscapeError> {
        let decoder = open_decoder(path)?;
        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();
        Ok(Self {
            path: path.to_string(),
            decoder,
            channels,
            sample_rate,
        })
    }
}

impl Iterator for StreamingLoopSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(sample) = self.decoder.next() {
            return Some(sample);
        }
        self.decoder = open_decoder(&self.path).ok()?;
        self.decoder.next()
    }
}

impl Source for StreamingLoopSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        self.channels
    }
    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

fn open_decoder(path: &str) -> Result<Decoder<BufReader<File>>, SoundscapeError> {
    let path = Path::new(path);
    // `stat` first so a FIFO or device never blocks the worker in `open`.
    crate::music::path_authority::path_key(path).map_err(|_| SoundscapeError::local_file())?;
    crate::music::path_authority::require_kind(
        path,
        crate::music::path_authority::MusicPathKind::File,
    )
    .map_err(|_| SoundscapeError::local_file())?;
    let file = File::open(path).map_err(|_| SoundscapeError::local_file())?;
    Decoder::try_from(file).map_err(|_| SoundscapeError::local_file())
}

struct ActiveOutput {
    sink: MixerDeviceSink,
    players: Vec<Player>,
    /// Source id of each player, in the same order.
    ids: Vec<String>,
}

impl ActiveOutput {
    fn open(
        request: &SoundscapeStartRequest,
        guard: Option<&OutputGuard>,
    ) -> Result<Self, SoundscapeError> {
        check_output_guard(guard)?;
        let sample_rate = if let Some(path) = request.local_path.as_deref() {
            StreamingLoopSource::open(path)?.sample_rate()
        } else {
            nonzero_sample_rate(OUTPUT_SAMPLE_RATE)
        };
        let mut sink = DeviceSinkBuilder::from_default_device()
            .map(|builder| builder.with_sample_rate(sample_rate))
            .and_then(|builder| builder.open_sink_or_fallback())
            .map_err(|_| SoundscapeError::audio_device())?;
        sink.log_on_drop(false);
        let layers = request_layers(request);
        let players = layers
            .iter()
            .map(|layer| layer_player(sink.mixer(), layer))
            .collect::<Result<Vec<_>, _>>()?;
        check_output_guard(guard)?;
        Ok(Self {
            sink,
            players,
            ids: layers.into_iter().map(|layer| layer.source_id).collect(),
        })
    }

    /// Moves to the request's layers in place. Layers that keep playing are untouched, new layers start at their
    /// target and rise through their source fade, and the players of dropped layers are returned so they can fade
    /// out. Nothing changes when a new layer cannot open.
    fn change_layers(
        &mut self,
        request: &SoundscapeStartRequest,
        targets: &[f32],
    ) -> Result<Vec<Player>, SoundscapeError> {
        let layers = request_layers(request);
        let ids = layers
            .iter()
            .map(|layer| layer.source_id.clone())
            .collect::<Vec<_>>();
        let plan = reuse_plan(&self.ids, &ids);
        let mut fresh = Vec::new();
        for ((layer, reuse), target) in layers.iter().zip(&plan).zip(targets) {
            if reuse.is_none() {
                let player = layer_player(self.sink.mixer(), layer)?;
                player.set_volume(*target);
                fresh.push(player);
            }
        }
        let mut fresh = fresh.into_iter();
        let mut previous = std::mem::take(&mut self.players)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>();
        for reuse in &plan {
            let player = match reuse {
                Some(index) => previous[*index].take(),
                None => fresh.next(),
            };
            self.players.extend(player);
        }
        self.ids = ids;
        Ok(previous.into_iter().flatten().collect())
    }

    fn stop(self) {
        ramp_players(&self.players, &vec![0.0; self.players.len()]);
        for player in &self.players {
            player.stop();
        }
    }
}

/// Layer volumes moving to their targets after the playing mix changed, advanced by the engine worker.
struct VolumeGlide {
    started: Instant,
    /// Volume of each playing layer when the glide began, in player order.
    from: Vec<f32>,
    /// Players of dropped layers with the volume they fade out from; they stop when the glide ends.
    retiring: Vec<(Player, f32)>,
}

#[derive(Default)]
struct EngineCore {
    output: Option<ActiveOutput>,
    last_request: Option<SoundscapeStartRequest>,
    snapshot: SoundscapeSnapshot,
    /// Music volume setting, or zero while muted; silent until the Music session reports it.
    master: f64,
    glide: Option<VolumeGlide>,
    /// Whether a Music pause paused the soundscape, so resuming Music resumes it too. Any other
    /// playback choice clears it.
    paused_by_music: bool,
}

impl EngineCore {
    fn handle(&mut self, command: EngineCommand) -> Result<SoundscapeSnapshot, SoundscapeError> {
        match command {
            EngineCommand::Automatic(request, guard) => {
                check_output_guard(Some(&guard))?;
                self.paused_by_music = false;
                match request {
                    Some(request) => self.start(request, Some(&guard)),
                    None => Ok(self.pause()),
                }
            }
            EngineCommand::ResumeGuarded(guard) => {
                check_output_guard(Some(&guard))?;
                self.paused_by_music = false;
                self.resume(Some(&guard))
            }
            EngineCommand::RecoverGuarded(guard) => {
                // Output paused with Music waits for Music instead of restarting after an interruption.
                if self.paused_by_music {
                    return Ok(self.snapshot.clone());
                }
                check_output_guard(Some(&guard))?;
                self.recover(Some(&guard))
            }
            EngineCommand::PauseWithMusic => Ok(self.pause_with_music()),
            EngineCommand::ResumeWithMusic(guard) => self.resume_with_music(guard),
            EngineCommand::Pause => {
                self.paused_by_music = false;
                Ok(self.pause())
            }
            EngineCommand::Stop => {
                self.paused_by_music = false;
                Ok(self.stop())
            }
            EngineCommand::SetPresetLevel(level) => self.set_preset_level(level),
            EngineCommand::SetMaster(master) => Ok(self.set_master(master)),
            EngineCommand::SetLevels(levels) => self.set_levels(levels),
            EngineCommand::Snapshot => Ok(self.snapshot.clone()),
        }
    }

    fn start(
        &mut self,
        request: SoundscapeStartRequest,
        guard: Option<&OutputGuard>,
    ) -> Result<SoundscapeSnapshot, SoundscapeError> {
        check_output_guard(guard)?;
        validate_request(&request)?;
        for path in std::iter::once(request.local_path.as_deref())
            .chain(
                request
                    .extra_sources
                    .iter()
                    .map(|layer| layer.local_path.as_deref()),
            )
            .flatten()
        {
            StreamingLoopSource::open(path)?;
        }
        check_output_guard(guard)?;
        if request.keep_playing_layers && self.snapshot.status == SoundscapeStatus::Playing {
            return self.change_layers(request);
        }
        match ActiveOutput::open(&request, guard) {
            Ok(output) => {
                check_output_guard(guard)?;
                self.stop_output();
                if let Err(error) = check_output_guard(guard) {
                    self.snapshot.status = SoundscapeStatus::Idle;
                    return Err(error);
                }
                ramp_players(&output.players, &layer_targets(&request, self.master));
                self.output = Some(output);
                self.last_request = Some(request.clone());
                self.snapshot = SoundscapeSnapshot {
                    status: SoundscapeStatus::Playing,
                    source_id: Some(request.source_id),
                    error_code: None,
                };
                Ok(self.snapshot.clone())
            }
            Err(error) => {
                self.snapshot.status = SoundscapeStatus::Error;
                self.snapshot.source_id = Some(request.source_id);
                self.snapshot.error_code = Some(error.code.clone());
                Err(error)
            }
        }
    }

    /// Changes the playing mix to the request's layers without restarting the layers it keeps.
    fn change_layers(
        &mut self,
        request: SoundscapeStartRequest,
    ) -> Result<SoundscapeSnapshot, SoundscapeError> {
        let Some(output) = self.output.as_mut() else {
            return Err(SoundscapeError::backend());
        };
        let targets = layer_targets(&request, self.master);
        let dropped = match output.change_layers(&request, &targets) {
            Ok(dropped) => dropped,
            Err(error) => {
                self.snapshot.error_code = Some(error.code.clone());
                return Err(error);
            }
        };
        // Layers still fading out from an earlier change continue from where they are now.
        let retiring = self
            .glide
            .take()
            .into_iter()
            .flat_map(|glide| glide.retiring.into_iter().map(|(player, _)| player))
            .chain(dropped)
            .map(|player| {
                let volume = player.volume();
                (player, volume)
            })
            .collect();
        self.glide = Some(VolumeGlide {
            started: Instant::now(),
            from: output.players.iter().map(Player::volume).collect(),
            retiring,
        });
        self.snapshot.source_id = Some(request.source_id.clone());
        self.snapshot.error_code = None;
        self.last_request = Some(request);
        Ok(self.snapshot.clone())
    }

    /// Moves gliding layers toward their current targets, and stops dropped layers once the glide ends.
    fn advance_glide(&mut self, now: Instant) {
        let Some(glide) = self.glide.as_ref() else {
            return;
        };
        let progress = glide_progress(now.saturating_duration_since(glide.started));
        if let (Some(output), Some(request)) = (self.output.as_ref(), self.last_request.as_ref()) {
            let targets = layer_targets(request, self.master);
            for ((player, from), target) in output.players.iter().zip(&glide.from).zip(targets) {
                player.set_volume(ramp_between(*from, target, progress));
            }
        }
        for (player, from) in &glide.retiring {
            player.set_volume(ramp_between(*from, 0.0, progress));
        }
        if progress >= 1.0 {
            self.finish_glide();
        }
    }

    /// Ends a glide early, stopping the layers that were fading out.
    fn finish_glide(&mut self) {
        if let Some(glide) = self.glide.take() {
            for (player, _) in glide.retiring {
                player.stop();
            }
        }
    }

    /// How long the worker may wait for a command before moving the glide again.
    fn glide_tick(&self) -> Option<Duration> {
        self.glide.as_ref().map(|_| GLIDE_TICK)
    }

    fn pause(&mut self) -> SoundscapeSnapshot {
        self.finish_glide();
        if let Some(output) = self.output.as_ref() {
            ramp_players(&output.players, &vec![0.0; output.players.len()]);
            for player in &output.players {
                player.pause();
            }
            self.snapshot.status = SoundscapeStatus::Paused;
        }
        self.snapshot.clone()
    }

    fn resume(
        &mut self,
        guard: Option<&OutputGuard>,
    ) -> Result<SoundscapeSnapshot, SoundscapeError> {
        check_output_guard(guard)?;
        if let Some(output) = self.output.as_ref() {
            for player in &output.players {
                player.play();
            }
            if let Some(request) = self.last_request.as_ref() {
                ramp_players(&output.players, &layer_targets(request, self.master));
            }
            self.snapshot.status = SoundscapeStatus::Playing;
            self.snapshot.error_code = None;
            return Ok(self.snapshot.clone());
        }
        self.recover(guard)
    }

    /// Pauses playing output because Music paused; output that is already paused stays as it is.
    fn pause_with_music(&mut self) -> SoundscapeSnapshot {
        if self.snapshot.status != SoundscapeStatus::Playing {
            return self.snapshot.clone();
        }
        let snapshot = self.pause();
        self.paused_by_music = snapshot.status == SoundscapeStatus::Paused;
        snapshot
    }

    /// Resumes output only when a Music pause paused it, never one the listener paused.
    fn resume_with_music(
        &mut self,
        guard: OutputGuard,
    ) -> Result<SoundscapeSnapshot, SoundscapeError> {
        if !self.paused_by_music || self.snapshot.status != SoundscapeStatus::Paused {
            return Ok(self.snapshot.clone());
        }
        check_output_guard(Some(&guard))?;
        self.paused_by_music = false;
        self.resume(Some(&guard))
    }

    fn stop(&mut self) -> SoundscapeSnapshot {
        self.stop_output();
        self.last_request = None;
        self.snapshot = SoundscapeSnapshot::default();
        self.snapshot.clone()
    }

    fn set_preset_level(&mut self, level: f64) -> Result<SoundscapeSnapshot, SoundscapeError> {
        if !valid_preset_level(level) {
            return Err(SoundscapeError::validation(
                "The preset level must be between zero and two.",
            ));
        }
        if let Some(request) = self.last_request.as_mut() {
            request.preset_level = level;
        }
        self.apply_levels();
        Ok(self.snapshot.clone())
    }

    /// Follows the Music volume setting; invalid values are ignored because nobody awaits this command.
    fn set_master(&mut self, master: f64) -> SoundscapeSnapshot {
        if master.is_finite() {
            self.master = master.clamp(0.0, 1.0);
            self.apply_levels();
        }
        self.snapshot.clone()
    }

    fn set_levels(&mut self, levels: Vec<f64>) -> Result<SoundscapeSnapshot, SoundscapeError> {
        if levels.is_empty()
            || levels.len() > MAX_SOUNDSCAPE_LAYERS
            || levels.iter().any(|level| !valid_sound_level(*level))
        {
            return Err(SoundscapeError::validation("Invalid soundscape levels."));
        }
        if let Some(request) = self.last_request.as_mut() {
            if levels.len() != 1 + request.extra_sources.len() {
                return Err(SoundscapeError::validation("Soundscape selection changed."));
            }
            request.level = levels[0];
            for (layer, level) in request.extra_sources.iter_mut().zip(levels.iter().skip(1)) {
                layer.level = *level;
            }
        }
        self.apply_levels();
        Ok(self.snapshot.clone())
    }

    /// Moves playing layers to their current targets without a ramp, for continuous slider changes. A running glide
    /// keeps going toward the new targets instead.
    fn apply_levels(&mut self) {
        if self.snapshot.status != SoundscapeStatus::Playing {
            return;
        }
        if self.glide.is_some() {
            self.advance_glide(Instant::now());
            return;
        }
        if let (Some(output), Some(request)) = (self.output.as_ref(), self.last_request.as_ref()) {
            set_player_levels(&output.players, &layer_targets(request, self.master));
        }
    }

    fn recover(
        &mut self,
        guard: Option<&OutputGuard>,
    ) -> Result<SoundscapeSnapshot, SoundscapeError> {
        let mut request = self
            .last_request
            .clone()
            .ok_or_else(SoundscapeError::backend)?;
        request.keep_playing_layers = false;
        self.start(request, guard)
    }

    fn stop_output(&mut self) {
        self.finish_glide();
        if let Some(output) = self.output.take() {
            output.stop();
        }
    }
}

enum EngineCommand {
    Automatic(Option<SoundscapeStartRequest>, OutputGuard),
    Pause,
    ResumeGuarded(OutputGuard),
    Stop,
    SetPresetLevel(f64),
    /// Music volume setting pushed by the Music session; no caller waits for the reply.
    SetMaster(f64),
    SetLevels(Vec<f64>),
    RecoverGuarded(OutputGuard),
    /// Music paused; pushed by the Music session, which never waits for the reply.
    PauseWithMusic,
    /// Music resumed; pushed by the Music session, which never waits for the reply.
    ResumeWithMusic(OutputGuard),
    Snapshot,
}

enum EngineMessage {
    Command(
        EngineCommand,
        mpsc::Sender<Result<SoundscapeSnapshot, SoundscapeError>>,
    ),
    Shutdown,
}

struct EngineController {
    sender: mpsc::Sender<EngineMessage>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl EngineController {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("ganbaru-soundscape".into())
            .spawn(move || {
                let mut core = EngineCore::default();
                loop {
                    // A running glide wakes the worker on every tick; otherwise it sleeps until a command arrives.
                    let message = match core.glide_tick() {
                        Some(tick) => match receiver.recv_timeout(tick) {
                            Ok(message) => Some(message),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        },
                        None => match receiver.recv() {
                            Ok(message) => Some(message),
                            Err(_) => break,
                        },
                    };
                    match message {
                        Some(EngineMessage::Command(command, reply)) => {
                            let _ = reply.send(core.handle(command));
                        }
                        Some(EngineMessage::Shutdown) => {
                            core.stop_output();
                            break;
                        }
                        None => {}
                    }
                    core.advance_glide(Instant::now());
                }
            })
            .expect("failed to start soundscape worker");
        Self {
            sender,
            worker: Mutex::new(Some(worker)),
        }
    }

    fn dispatch(&self, command: EngineCommand) -> Result<SoundscapeSnapshot, SoundscapeError> {
        let (reply, receiver) = mpsc::channel();
        self.sender
            .send(EngineMessage::Command(command, reply))
            .map_err(|_| SoundscapeError::backend())?;
        await_reply(&receiver, ENGINE_REPLY_TIMEOUT)
    }

    /// Queues a command without waiting, for callers that must never block on audio work.
    fn notify(&self, command: EngineCommand) {
        let (reply, _receiver) = mpsc::channel();
        let _ = self.sender.send(EngineMessage::Command(command, reply));
    }
}

/// Waits a bounded time for the worker, which may be stuck on a slow file or audio device.
fn await_reply(
    receiver: &mpsc::Receiver<Result<SoundscapeSnapshot, SoundscapeError>>,
    timeout: Duration,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    receiver
        .recv_timeout(timeout)
        .map_err(|_| SoundscapeError::backend())?
}

impl Default for EngineController {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EngineController {
    fn drop(&mut self) {
        let _ = self.sender.send(EngineMessage::Shutdown);
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct SoundscapeEngineState {
    controller: EngineController,
}

type OutputGuard = Box<dyn Fn() -> bool + Send>;

fn check_output_guard(guard: Option<&OutputGuard>) -> Result<(), SoundscapeError> {
    if guard.is_some_and(|guard| !guard()) {
        return Err(SoundscapeError::validation(
            "The accepted soundscape was superseded.",
        ));
    }
    Ok(())
}

/// Refuses a local loop that no grant or root binding on this device covers, so a webview
/// request or a vault-sourced location cannot open a path the user never chose here.
fn authorize_request(
    app: &tauri::AppHandle,
    request: &SoundscapeStartRequest,
) -> Result<(), SoundscapeError> {
    let paths = std::iter::once(request.local_path.as_deref())
        .chain(
            request
                .extra_sources
                .iter()
                .map(|layer| layer.local_path.as_deref()),
        )
        .flatten();
    for path in paths {
        if !crate::music::is_soundscape_audio_path(Path::new(path)) {
            return Err(SoundscapeError::local_file());
        }
        crate::music::path_authority::authorize(
            app,
            path,
            crate::music::path_authority::MusicPathKind::File,
        )
        .map_err(|_| SoundscapeError::local_file())?;
    }
    Ok(())
}

/// Runs engine work off the main thread; the worker may open files and audio devices.
async fn run_engine<T, F>(app: tauri::AppHandle, work: F) -> Result<T, SoundscapeError>
where
    T: Send + 'static,
    F: FnOnce(&tauri::AppHandle, &EngineController) -> Result<T, SoundscapeError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<SoundscapeEngineState>();
        work(&app, &state.controller)
    })
    .await
    .map_err(|_| SoundscapeError::backend())?
}

/// Execute a persisted automatic effect, checking its authority before audible output.
pub(crate) fn apply_automatic(
    app: &tauri::AppHandle,
    request: Option<SoundscapeStartRequest>,
    guard: OutputGuard,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    if let Some(request) = &request {
        authorize_request(app, request)?;
    }
    app.state::<SoundscapeEngineState>()
        .controller
        .dispatch(EngineCommand::Automatic(request, guard))
}

/// Stops background output from a blocking context.
pub(crate) fn stop_blocking(app: &tauri::AppHandle) -> Result<SoundscapeSnapshot, SoundscapeError> {
    app.state::<SoundscapeEngineState>()
        .controller
        .dispatch(EngineCommand::Stop)
}

/// Makes the soundscape follow the Music volume setting, where `master` is zero while Music is muted.
///
/// The command is queued without waiting so a Music session publish never blocks on the audio worker.
pub(crate) fn set_master(app: &tauri::AppHandle, master: f64) {
    if let Some(state) = app.try_state::<SoundscapeEngineState>() {
        state.controller.notify(EngineCommand::SetMaster(master));
    }
}

/// Pauses a playing soundscape because Music paused, and remembers that Music paused it.
pub(crate) fn pause_with_music(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<SoundscapeEngineState>() {
        state.controller.notify(EngineCommand::PauseWithMusic);
    }
}

/// Resumes the soundscape because Music resumed, only when a Music pause paused it.
pub(crate) fn resume_with_music(app: &tauri::AppHandle, guard: OutputGuard) {
    if let Some(state) = app.try_state::<SoundscapeEngineState>() {
        state
            .controller
            .notify(EngineCommand::ResumeWithMusic(guard));
    }
}

#[tauri::command]
pub(crate) async fn music_soundscape_start(
    app: tauri::AppHandle,
    request: SoundscapeStartRequest,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    run_engine(app, move |app, controller| {
        authorize_request(app, &request)?;
        let guard = crate::music::session::runtime::background_output_guard(app)
            .map_err(SoundscapeError::validation)?;
        controller.dispatch(EngineCommand::Automatic(Some(request), guard))
    })
    .await
}

#[tauri::command]
pub(crate) async fn music_soundscape_pause(
    app: tauri::AppHandle,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    run_engine(app, |_, controller| {
        controller.dispatch(EngineCommand::Pause)
    })
    .await
}

#[tauri::command]
pub(crate) async fn music_soundscape_resume(
    app: tauri::AppHandle,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    run_engine(app, |app, controller| {
        let guard = crate::music::session::runtime::background_output_guard(app)
            .map_err(SoundscapeError::validation)?;
        controller.dispatch(EngineCommand::ResumeGuarded(guard))
    })
    .await
}

#[tauri::command]
pub(crate) async fn music_soundscape_stop(
    app: tauri::AppHandle,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    run_engine(app, |_, controller| {
        controller.dispatch(EngineCommand::Stop)
    })
    .await
}

#[tauri::command]
pub(crate) async fn music_soundscape_set_preset_level(
    app: tauri::AppHandle,
    level: f64,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    run_engine(app, move |_, controller| {
        controller.dispatch(EngineCommand::SetPresetLevel(level))
    })
    .await
}

#[tauri::command]
pub(crate) async fn music_soundscape_set_levels(
    app: tauri::AppHandle,
    levels: Vec<f64>,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    run_engine(app, move |_, controller| {
        controller.dispatch(EngineCommand::SetLevels(levels))
    })
    .await
}

#[tauri::command]
pub(crate) async fn music_soundscape_recover(
    app: tauri::AppHandle,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    run_engine(app, |app, controller| {
        let guard = crate::music::session::runtime::background_output_guard(app)
            .map_err(SoundscapeError::validation)?;
        controller.dispatch(EngineCommand::RecoverGuarded(guard))
    })
    .await
}

#[tauri::command]
pub(crate) async fn music_soundscape_snapshot(
    app: tauri::AppHandle,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    run_engine(app, |_, controller| {
        controller.dispatch(EngineCommand::Snapshot)
    })
    .await
}

fn validate_request(request: &SoundscapeStartRequest) -> Result<(), SoundscapeError> {
    if request.source_id.trim().is_empty() {
        return Err(SoundscapeError::validation(
            "A stable source id is required.",
        ));
    }
    if !valid_preset_level(request.preset_level) {
        return Err(SoundscapeError::validation(
            "The preset level must be between zero and two.",
        ));
    }
    if request.extra_sources.len() >= MAX_SOUNDSCAPE_LAYERS {
        return Err(SoundscapeError::validation("Too many soundscape sounds."));
    }
    let mut ids = std::collections::HashSet::new();
    for layer in request_layers(request) {
        if layer.source_id.trim().is_empty() || !ids.insert(layer.source_id) {
            return Err(SoundscapeError::validation(
                "Soundscape sounds need distinct source ids.",
            ));
        }
        if !valid_sound_level(layer.level) {
            return Err(SoundscapeError::validation(
                "Sound levels must be between zero and one.",
            ));
        }
        match (layer.generated_kind, layer.local_path.as_deref()) {
            (Some(_), None) => {}
            (None, Some(path)) if Path::new(path).is_absolute() => {}
            _ => {
                return Err(SoundscapeError::validation(
                    "Choose a generated noise or local loop source.",
                ));
            }
        }
    }
    Ok(())
}

fn valid_preset_level(level: f64) -> bool {
    level.is_finite() && (0.0..=2.0).contains(&level)
}

fn valid_sound_level(level: f64) -> bool {
    level.is_finite() && (0.0..=1.0).contains(&level)
}

/// The preset output, Music volume times preset level clamped to full scale, shared evenly by the layers.
fn layer_volume(master: f64, preset_level: f64, count: usize) -> f64 {
    (master * preset_level).clamp(0.0, 1.0) / count.max(1) as f64
}

fn layer_targets(request: &SoundscapeStartRequest, master: f64) -> Vec<f32> {
    let base = layer_volume(
        master,
        request.preset_level,
        1 + request.extra_sources.len(),
    );
    std::iter::once(request.level)
        .chain(request.extra_sources.iter().map(|layer| layer.level))
        .map(|level| (base * level).clamp(0.0, 1.0) as f32)
        .collect()
}

/// Every layer of a request in play order: the first sound, then the extra sources.
fn request_layers(request: &SoundscapeStartRequest) -> Vec<SoundscapeLayer> {
    std::iter::once(SoundscapeLayer {
        source_id: request.source_id.clone(),
        generated_kind: request.generated_kind,
        local_path: request.local_path.clone(),
        level: request.level,
    })
    .chain(request.extra_sources.iter().cloned())
    .collect()
}

/// A silent player on the mixer whose source rises from silence over the start fade.
fn layer_player(mixer: &Mixer, layer: &SoundscapeLayer) -> Result<Player, SoundscapeError> {
    let player = Player::connect_new(mixer);
    player.set_volume(0.0);
    match (layer.generated_kind, layer.local_path.as_deref()) {
        (Some(kind), None) => player
            .append(NoiseSource::new(kind, seed_from_id(&layer.source_id)).fade_in(START_FADE)),
        (None, Some(path)) => player.append(StreamingLoopSource::open(path)?.fade_in(START_FADE)),
        _ => {
            return Err(SoundscapeError::validation(
                "Choose a generated noise or local loop source.",
            ));
        }
    }
    Ok(player)
}

/// For each next layer, the index of the playing layer that continues as it, or `None` when it starts fresh.
fn reuse_plan(playing: &[String], next: &[String]) -> Vec<Option<usize>> {
    next.iter()
        .map(|id| playing.iter().position(|entry| entry == id))
        .collect()
}

/// Share of a layer glide completed after this much time, from 0 to 1.
fn glide_progress(elapsed: Duration) -> f32 {
    (elapsed.as_secs_f32() / START_FADE.as_secs_f32()).min(1.0)
}

fn ramp_between(from: f32, to: f32, progress: f32) -> f32 {
    from + (to - from) * progress
}

fn set_player_levels(players: &[Player], levels: &[f32]) {
    for (player, level) in players.iter().zip(levels) {
        player.set_volume(*level);
    }
}

fn ramp_players(players: &[Player], targets: &[f32]) {
    let starts: Vec<f32> = players.iter().map(Player::volume).collect();
    for step in 1..=RAMP_STEPS {
        for ((player, start), target) in players.iter().zip(&starts).zip(targets) {
            player.set_volume(ramp_value(*start, *target, step));
        }
        thread::sleep(Duration::from_millis(RAMP_MILLIS / RAMP_STEPS));
    }
}

fn ramp_value(from: f32, to: f32, step: u64) -> f32 {
    ramp_between(from, to, step as f32 / RAMP_STEPS as f32)
}

fn seed_from_id(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    })
}

fn nonzero_sample_rate(value: u32) -> SampleRate {
    SampleRate::new(value).expect("sample rate must be greater than zero")
}

fn nonzero_channels(value: u16) -> ChannelCount {
    ChannelCount::new(value).expect("channel count must be greater than zero")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn local_loops_reject_fifos_and_folders_before_opening_them() {
        let root = std::env::temp_dir().join(format!("soundscape-kind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let fifo = root.join("rain.ogg");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        // Opening the FIFO would block this thread forever; a refusal proves `stat` ran first.
        for path in [
            fifo.as_path(),
            root.as_path(),
            Path::new("relative/rain.ogg"),
        ] {
            assert_eq!(
                open_decoder(path.to_str().unwrap()).err(),
                Some(SoundscapeError::local_file())
            );
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dispatch_reports_the_engine_unavailable_when_the_worker_does_not_reply() {
        let (_reply, receiver) = mpsc::channel();
        assert_eq!(
            await_reply(&receiver, Duration::from_millis(20)),
            Err(SoundscapeError::backend())
        );
    }

    #[test]
    fn native_music_background_rejects_revoked_automatic_start_before_opening_an_audio_device() {
        let mut core = EngineCore::default();
        let request = SoundscapeStartRequest {
            source_id: "generated-white".into(),
            generated_kind: Some(GeneratedNoiseKind::White),
            local_path: None,
            level: 1.0,
            extra_sources: Vec::new(),
            preset_level: 0.5,
            keep_playing_layers: false,
        };
        assert!(
            core.handle(EngineCommand::Automatic(Some(request), Box::new(|| false)))
                .is_err()
        );
        assert_eq!(core.snapshot.status, SoundscapeStatus::Idle);
        assert!(core.output.is_none());
    }

    fn paused_by_music() -> EngineCore {
        let mut core = EngineCore::default();
        core.snapshot.status = SoundscapeStatus::Paused;
        core.paused_by_music = true;
        core
    }

    #[test]
    fn output_paused_with_music_waits_for_music_instead_of_recovering() {
        let mut core = paused_by_music();
        let snapshot = core
            .handle(EngineCommand::RecoverGuarded(Box::new(|| true)))
            .unwrap();
        assert_eq!(snapshot.status, SoundscapeStatus::Paused);
        assert!(core.paused_by_music);
    }

    #[test]
    fn resuming_music_leaves_output_the_listener_paused() {
        let mut core = paused_by_music();
        core.paused_by_music = false;
        let snapshot = core
            .handle(EngineCommand::ResumeWithMusic(Box::new(|| true)))
            .unwrap();
        assert_eq!(snapshot.status, SoundscapeStatus::Paused);

        let mut idle = EngineCore::default();
        assert_eq!(
            idle.handle(EngineCommand::PauseWithMusic).unwrap().status,
            SoundscapeStatus::Idle
        );
        assert!(!idle.paused_by_music);
    }

    #[test]
    fn resuming_music_spends_the_music_pause_once() {
        let mut core = paused_by_music();
        // Without output the engine restarts the last request, and there is none here.
        assert!(
            core.handle(EngineCommand::ResumeWithMusic(Box::new(|| true)))
                .is_err()
        );
        assert!(!core.paused_by_music);
    }

    #[test]
    fn listener_choices_clear_the_music_pause_but_rejected_automation_does_not() {
        let mut core = paused_by_music();
        assert!(
            core.handle(EngineCommand::Automatic(None, Box::new(|| false)))
                .is_err()
        );
        assert!(core.paused_by_music);
        assert!(
            core.handle(EngineCommand::ResumeWithMusic(Box::new(|| false)))
                .is_err()
        );
        assert!(core.paused_by_music);

        for command in [EngineCommand::Pause, EngineCommand::Stop] {
            let mut core = paused_by_music();
            core.handle(command).unwrap();
            assert!(!core.paused_by_music);
        }
        let mut core = paused_by_music();
        core.handle(EngineCommand::Automatic(None, Box::new(|| true)))
            .unwrap();
        assert!(!core.paused_by_music);
    }

    fn mono_samples(kind: GeneratedNoiseKind, count: usize) -> Vec<f32> {
        NoiseSource::new(kind, 42)
            .step_by(2)
            .skip(2_000)
            .take(count)
            .collect()
    }

    fn roughness(samples: &[f32]) -> f32 {
        samples
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .sum::<f32>()
            / (samples.len() - 1) as f32
    }

    #[test]
    fn generated_noise_is_deterministic_bounded_and_ramped() {
        let first: Vec<_> = NoiseSource::new(GeneratedNoiseKind::White, 7)
            .take(2_000)
            .collect();
        let second: Vec<_> = NoiseSource::new(GeneratedNoiseKind::White, 7)
            .take(2_000)
            .collect();
        assert_eq!(first, second);
        assert!(first.iter().all(|sample| sample.abs() <= NOISE_AMPLITUDE));
        assert!(first[0].abs() < 0.001);
        assert!(first[1_900].abs() > first[0].abs());
    }

    #[test]
    fn generated_noise_spectra_have_distinct_roughness() {
        let white = roughness(&mono_samples(GeneratedNoiseKind::White, 20_000));
        let pink = roughness(&mono_samples(GeneratedNoiseKind::Pink, 20_000));
        let brown = roughness(&mono_samples(GeneratedNoiseKind::Brown, 20_000));
        assert!(white > pink * 1.5, "white={white}, pink={pink}");
        assert!(pink > brown * 1.5, "pink={pink}, brown={brown}");
    }

    #[test]
    fn generated_noise_stays_near_zero_mean() {
        for kind in [
            GeneratedNoiseKind::White,
            GeneratedNoiseKind::Pink,
            GeneratedNoiseKind::Brown,
        ] {
            let samples = mono_samples(kind, 100_000);
            let mean = samples.iter().sum::<f32>() / samples.len() as f32;
            assert!(mean.abs() < 0.02, "{kind:?} mean={mean}");
        }
    }

    #[test]
    fn source_request_requires_exactly_one_source() {
        let request = SoundscapeStartRequest {
            source_id: "rain".into(),
            generated_kind: Some(GeneratedNoiseKind::White),
            local_path: Some("/tmp/rain.wav".into()),
            level: 1.0,
            extra_sources: Vec::new(),
            preset_level: 0.5,
            keep_playing_layers: false,
        };
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn layered_requests_require_distinct_sources_and_share_the_preset_output() {
        let request = SoundscapeStartRequest {
            source_id: "brown".into(),
            generated_kind: Some(GeneratedNoiseKind::Brown),
            local_path: None,
            level: 1.0,
            extra_sources: vec![SoundscapeLayer {
                source_id: "pink".into(),
                generated_kind: Some(GeneratedNoiseKind::Pink),
                local_path: None,
                level: 0.5,
            }],
            preset_level: 0.5,
            keep_playing_layers: false,
        };
        assert!(validate_request(&request).is_ok());
        assert!((layer_volume(0.4, 0.5, 2) - 0.1).abs() < f64::EPSILON);
        assert_eq!(layer_targets(&request, 0.4), vec![0.1, 0.05]);
        for invalid in [
            SoundscapeStartRequest {
                level: 1.1,
                ..request.clone()
            },
            SoundscapeStartRequest {
                preset_level: 2.1,
                ..request.clone()
            },
            SoundscapeStartRequest {
                preset_level: f64::NAN,
                ..request.clone()
            },
        ] {
            assert!(validate_request(&invalid).is_err());
        }
        let duplicate = SoundscapeStartRequest {
            extra_sources: vec![SoundscapeLayer {
                source_id: "brown".into(),
                generated_kind: Some(GeneratedNoiseKind::Pink),
                local_path: None,
                level: 1.0,
            }],
            ..request
        };
        assert!(validate_request(&duplicate).is_err());
    }

    #[test]
    fn preset_output_follows_music_volume_and_clamps_at_full_scale() {
        assert_eq!(layer_volume(0.0, 2.0, 1), 0.0);
        assert_eq!(layer_volume(0.6, 1.0, 1), 0.6);
        assert_eq!(layer_volume(0.8, 2.0, 1), 1.0);
        assert_eq!(layer_volume(0.8, 2.0, 4), 0.25);
        let mut core = EngineCore::default();
        assert_eq!(core.master, 0.0);
        core.set_master(0.7);
        assert_eq!(core.master, 0.7);
        core.set_master(f64::NAN);
        assert_eq!(core.master, 0.7);
        core.set_master(1.5);
        assert_eq!(core.master, 1.0);
        assert!(core.set_preset_level(2.5).is_err());
        assert!(core.set_levels(vec![1.2]).is_err());
    }

    #[test]
    fn volume_ramps_end_at_target_without_an_abrupt_step() {
        let down = (1..=RAMP_STEPS)
            .map(|step| ramp_value(0.8, 0.0, step))
            .collect::<Vec<_>>();
        assert_eq!(down.last().copied(), Some(0.0));
        assert!(down.windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(
            down.windows(2)
                .all(|pair| (pair[1] - pair[0]).abs() <= 0.11)
        );
        let up = (1..=RAMP_STEPS)
            .map(|step| ramp_value(0.0, 0.8, step))
            .collect::<Vec<_>>();
        assert_eq!(up.last().copied(), Some(0.8));
        assert!(up.windows(2).all(|pair| pair[1] >= pair[0]));
    }

    #[test]
    fn changing_layers_keeps_the_ones_still_chosen_and_starts_only_new_ones() {
        let ids = |values: &[&str]| {
            values
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            reuse_plan(&ids(&["brown", "rain"]), &ids(&["brown", "rain", "pink"])),
            vec![Some(0), Some(1), None]
        );
        assert_eq!(
            reuse_plan(&ids(&["brown", "rain", "pink"]), &ids(&["pink", "brown"])),
            vec![Some(2), Some(0)]
        );
        assert_eq!(reuse_plan(&ids(&["brown"]), &ids(&["white"])), vec![None]);
    }

    #[test]
    fn layer_glides_last_as_long_as_the_start_fade_and_keep_the_sum_steady() {
        assert_eq!(glide_progress(Duration::ZERO), 0.0);
        assert_eq!(glide_progress(START_FADE / 2), 0.5);
        assert_eq!(glide_progress(START_FADE * 2), 1.0);
        // A kept layer gliding from full to half while a new layer's fade rises to half sums to full throughout.
        for step in 0..=10 {
            let progress = step as f32 / 10.0;
            let kept = ramp_between(0.4, 0.2, progress);
            let added = ramp_between(0.0, 0.2, progress);
            assert!((kept + added - 0.4).abs() < 1e-6);
        }
    }
}

use std::{
    fs::File,
    io::BufReader,
    path::Path,
    sync::{Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

use ganbaru_music_library::soundscapes::MAX_SOUNDSCAPE_LAYERS;
use rodio::{
    ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source,
};
use serde::{Deserialize, Serialize};
use tauri::State;

const OUTPUT_SAMPLE_RATE: u32 = 48_000;
const OUTPUT_CHANNELS: u16 = 2;
const RAMP_MILLIS: u64 = 40;
const RAMP_STEPS: u64 = 8;
const NOISE_AMPLITUDE: f32 = 0.22;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum GeneratedNoiseKind {
    White,
    Pink,
    Brown,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SoundscapeStartRequest {
    pub source_id: String,
    pub generated_kind: Option<GeneratedNoiseKind>,
    pub local_path: Option<String>,
    pub level: f64,
    #[serde(default)]
    pub extra_sources: Vec<SoundscapeLayer>,
    pub volume: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SoundscapeLayer {
    pub source_id: String,
    pub generated_kind: Option<GeneratedNoiseKind>,
    pub local_path: Option<String>,
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
    pub volume: f64,
    pub error_code: Option<String>,
}

impl Default for SoundscapeSnapshot {
    fn default() -> Self {
        Self {
            status: SoundscapeStatus::Idle,
            source_id: None,
            volume: 0.1,
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
            message: "The background sound output is unavailable.".into(),
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
            message: "The background sound engine is unavailable.".into(),
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
    if !path.is_absolute() {
        return Err(SoundscapeError::local_file());
    }
    let file = File::open(path).map_err(|_| SoundscapeError::local_file())?;
    Decoder::try_from(file).map_err(|_| SoundscapeError::local_file())
}

struct ActiveOutput {
    _sink: MixerDeviceSink,
    players: Vec<Player>,
    request: SoundscapeStartRequest,
}

impl ActiveOutput {
    fn open(
        request: SoundscapeStartRequest,
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
        let mut players = Vec::with_capacity(1 + request.extra_sources.len());
        for layer in std::iter::once(SoundscapeLayer {
            source_id: request.source_id.clone(),
            generated_kind: request.generated_kind,
            local_path: request.local_path.clone(),
            level: request.level,
        })
        .chain(request.extra_sources.iter().cloned())
        {
            let player = Player::connect_new(sink.mixer());
            player.set_volume(0.0);
            match (layer.generated_kind, layer.local_path.as_deref()) {
                (Some(kind), None) => {
                    player.append(NoiseSource::new(kind, seed_from_id(&layer.source_id)))
                }
                (None, Some(path)) => player.append(StreamingLoopSource::open(path)?),
                _ => {
                    return Err(SoundscapeError::validation(
                        "Choose a generated noise or local loop source.",
                    ));
                }
            }
            players.push(player);
        }
        check_output_guard(guard)?;
        Ok(Self {
            _sink: sink,
            players,
            request,
        })
    }

    fn stop(mut self) {
        ramp_players(&self.players, &vec![0.0; self.players.len()]);
        for player in &self.players {
            player.stop();
        }
        self.request.volume = 0.0;
    }
}

#[derive(Default)]
struct EngineCore {
    output: Option<ActiveOutput>,
    last_request: Option<SoundscapeStartRequest>,
    snapshot: SoundscapeSnapshot,
}

impl EngineCore {
    fn handle(&mut self, command: EngineCommand) -> Result<SoundscapeSnapshot, SoundscapeError> {
        match command {
            EngineCommand::Automatic(request, guard) => {
                check_output_guard(Some(&guard))?;
                match request {
                    Some(request) => self.start(request, Some(&guard)),
                    None => Ok(self.pause()),
                }
            }
            EngineCommand::ResumeGuarded(guard) => {
                check_output_guard(Some(&guard))?;
                self.resume(Some(&guard))
            }
            EngineCommand::RecoverGuarded(guard) => {
                check_output_guard(Some(&guard))?;
                self.recover(Some(&guard))
            }
            EngineCommand::Pause => Ok(self.pause()),
            EngineCommand::Stop => Ok(self.stop()),
            EngineCommand::SetVolume(volume) => self.set_volume(volume),
            EngineCommand::SetLevels(levels) => self.set_levels(levels),
            EngineCommand::Snapshot => Ok(self.snapshot.clone()),
        }
    }

    fn start(
        &mut self,
        mut request: SoundscapeStartRequest,
        guard: Option<&OutputGuard>,
    ) -> Result<SoundscapeSnapshot, SoundscapeError> {
        check_output_guard(guard)?;
        validate_request(&request)?;
        request.volume = clamp_volume(request.volume);
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
        match ActiveOutput::open(request.clone(), guard) {
            Ok(output) => {
                check_output_guard(guard)?;
                self.stop_output();
                if let Err(error) = check_output_guard(guard) {
                    self.snapshot.status = SoundscapeStatus::Idle;
                    return Err(error);
                }
                ramp_players(&output.players, &layer_targets(&request));
                self.output = Some(output);
                self.last_request = Some(request.clone());
                self.snapshot = SoundscapeSnapshot {
                    status: SoundscapeStatus::Playing,
                    source_id: Some(request.source_id),
                    volume: request.volume,
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

    fn pause(&mut self) -> SoundscapeSnapshot {
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
                ramp_players(&output.players, &layer_targets(request));
            }
            self.snapshot.status = SoundscapeStatus::Playing;
            self.snapshot.error_code = None;
            return Ok(self.snapshot.clone());
        }
        self.recover(guard)
    }

    fn stop(&mut self) -> SoundscapeSnapshot {
        self.stop_output();
        self.last_request = None;
        let volume = self.snapshot.volume;
        self.snapshot = SoundscapeSnapshot {
            volume,
            ..SoundscapeSnapshot::default()
        };
        self.snapshot.clone()
    }

    fn set_volume(&mut self, volume: f64) -> Result<SoundscapeSnapshot, SoundscapeError> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(SoundscapeError::validation(
                "Volume must be between zero and one.",
            ));
        }
        self.snapshot.volume = volume;
        if let Some(request) = self.last_request.as_mut() {
            request.volume = volume;
        }
        if self.snapshot.status == SoundscapeStatus::Playing {
            if let Some(output) = self.output.as_ref() {
                if let Some(request) = self.last_request.as_ref() {
                    set_player_levels(&output.players, &layer_targets(request));
                }
            }
        }
        Ok(self.snapshot.clone())
    }

    fn set_levels(&mut self, levels: Vec<f64>) -> Result<SoundscapeSnapshot, SoundscapeError> {
        if levels.is_empty()
            || levels.len() > MAX_SOUNDSCAPE_LAYERS
            || levels
                .iter()
                .any(|level| !level.is_finite() || !(0.0..=2.0).contains(level))
        {
            return Err(SoundscapeError::validation(
                "Invalid background sound levels.",
            ));
        }
        if let Some(request) = self.last_request.as_mut() {
            if levels.len() != 1 + request.extra_sources.len() {
                return Err(SoundscapeError::validation(
                    "Background sound selection changed.",
                ));
            }
            request.level = levels[0];
            for (layer, level) in request.extra_sources.iter_mut().zip(levels.iter().skip(1)) {
                layer.level = *level;
            }
            if self.snapshot.status == SoundscapeStatus::Playing {
                if let Some(output) = self.output.as_ref() {
                    set_player_levels(&output.players, &layer_targets(request));
                }
            }
        }
        Ok(self.snapshot.clone())
    }

    fn recover(
        &mut self,
        guard: Option<&OutputGuard>,
    ) -> Result<SoundscapeSnapshot, SoundscapeError> {
        let request = self
            .last_request
            .clone()
            .ok_or_else(SoundscapeError::backend)?;
        self.start(request, guard)
    }

    fn stop_output(&mut self) {
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
    SetVolume(f64),
    SetLevels(Vec<f64>),
    RecoverGuarded(OutputGuard),
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
            .name("ganbaru-ai-soundscape".into())
            .spawn(move || {
                let mut core = EngineCore::default();
                while let Ok(message) = receiver.recv() {
                    match message {
                        EngineMessage::Command(command, reply) => {
                            let _ = reply.send(core.handle(command));
                        }
                        EngineMessage::Shutdown => {
                            core.stop_output();
                            break;
                        }
                    }
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
        receiver.recv().map_err(|_| SoundscapeError::backend())?
    }
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
            "The accepted background sound was superseded.",
        ));
    }
    Ok(())
}

/// Execute a persisted automatic effect, checking its authority before audible output.
pub(crate) fn apply_automatic(
    state: State<'_, SoundscapeEngineState>,
    request: Option<SoundscapeStartRequest>,
    guard: OutputGuard,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    state
        .controller
        .dispatch(EngineCommand::Automatic(request, guard))
}

#[tauri::command]
pub(crate) fn music_soundscape_start(
    app: tauri::AppHandle,
    state: State<'_, SoundscapeEngineState>,
    request: SoundscapeStartRequest,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    let guard = crate::music::session::runtime::background_output_guard(&app)
        .map_err(SoundscapeError::validation)?;
    state
        .controller
        .dispatch(EngineCommand::Automatic(Some(request), guard))
}

#[tauri::command]
pub(crate) fn music_soundscape_pause(
    state: State<'_, SoundscapeEngineState>,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    state.controller.dispatch(EngineCommand::Pause)
}

#[tauri::command]
pub(crate) fn music_soundscape_resume(
    app: tauri::AppHandle,
    state: State<'_, SoundscapeEngineState>,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    let guard = crate::music::session::runtime::background_output_guard(&app)
        .map_err(SoundscapeError::validation)?;
    state
        .controller
        .dispatch(EngineCommand::ResumeGuarded(guard))
}

#[tauri::command]
pub(crate) fn music_soundscape_stop(
    state: State<'_, SoundscapeEngineState>,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    state.controller.dispatch(EngineCommand::Stop)
}

#[tauri::command]
pub(crate) fn music_soundscape_set_volume(
    state: State<'_, SoundscapeEngineState>,
    volume: f64,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    state.controller.dispatch(EngineCommand::SetVolume(volume))
}

#[tauri::command]
pub(crate) fn music_soundscape_set_levels(
    state: State<'_, SoundscapeEngineState>,
    levels: Vec<f64>,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    state.controller.dispatch(EngineCommand::SetLevels(levels))
}

#[tauri::command]
pub(crate) fn music_soundscape_recover(
    app: tauri::AppHandle,
    state: State<'_, SoundscapeEngineState>,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    let guard = crate::music::session::runtime::background_output_guard(&app)
        .map_err(SoundscapeError::validation)?;
    state
        .controller
        .dispatch(EngineCommand::RecoverGuarded(guard))
}

#[tauri::command]
pub(crate) fn music_soundscape_snapshot(
    state: State<'_, SoundscapeEngineState>,
) -> Result<SoundscapeSnapshot, SoundscapeError> {
    state.controller.dispatch(EngineCommand::Snapshot)
}

fn validate_request(request: &SoundscapeStartRequest) -> Result<(), SoundscapeError> {
    if request.source_id.trim().is_empty() {
        return Err(SoundscapeError::validation(
            "A stable source id is required.",
        ));
    }
    if !request.volume.is_finite() || !(0.0..=1.0).contains(&request.volume) {
        return Err(SoundscapeError::validation(
            "Volume must be between zero and one.",
        ));
    }
    if request.extra_sources.len() >= MAX_SOUNDSCAPE_LAYERS {
        return Err(SoundscapeError::validation("Too many background sounds."));
    }
    let mut ids = std::collections::HashSet::new();
    for layer in std::iter::once(SoundscapeLayer {
        source_id: request.source_id.clone(),
        generated_kind: request.generated_kind,
        local_path: request.local_path.clone(),
        level: request.level,
    })
    .chain(request.extra_sources.iter().cloned())
    {
        if layer.source_id.trim().is_empty() || !ids.insert(layer.source_id) {
            return Err(SoundscapeError::validation(
                "Background sounds need distinct source ids.",
            ));
        }
        if !layer.level.is_finite() || !(0.0..=2.0).contains(&layer.level) {
            return Err(SoundscapeError::validation(
                "Sound levels must be between zero and two.",
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

fn clamp_volume(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.1
    }
}

fn layer_volume(volume: f64, count: usize) -> f64 {
    volume / count.max(1) as f64
}

fn layer_targets(request: &SoundscapeStartRequest) -> Vec<f32> {
    let base = layer_volume(request.volume, 1 + request.extra_sources.len());
    std::iter::once(request.level)
        .chain(request.extra_sources.iter().map(|layer| layer.level))
        .map(|level| (base * level).clamp(0.0, 1.0) as f32)
        .collect()
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
    let progress = step as f32 / RAMP_STEPS as f32;
    from + (to - from) * progress
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

    #[test]
    fn native_music_background_rejects_revoked_automatic_start_before_opening_an_audio_device() {
        let mut core = EngineCore::default();
        let request = SoundscapeStartRequest {
            source_id: "generated-white".into(),
            generated_kind: Some(GeneratedNoiseKind::White),
            local_path: None,
            level: 1.0,
            extra_sources: Vec::new(),
            volume: 0.1,
        };
        assert!(
            core.handle(EngineCommand::Automatic(Some(request), Box::new(|| false)))
                .is_err()
        );
        assert_eq!(core.snapshot.status, SoundscapeStatus::Idle);
        assert!(core.output.is_none());
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
            volume: 0.4,
        };
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn layered_requests_require_distinct_sources_and_reduce_each_layer_level() {
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
            volume: 0.1,
        };
        assert!(validate_request(&request).is_ok());
        assert!((layer_volume(0.1, 2) - 0.05).abs() < f64::EPSILON);
        assert_eq!(layer_targets(&request), vec![0.05, 0.025]);
        let excessive = SoundscapeStartRequest {
            level: 2.1,
            ..request.clone()
        };
        assert!(validate_request(&excessive).is_err());
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
}

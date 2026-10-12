//! One admitted decoder operation retains its worker until blocking work finishes.

use super::{BackendCommand, MediaPlayerError, PlayerCore, PlayerSnapshot};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(8);

pub(super) struct DeliveryAuthority {
    current: Arc<dyn Fn() -> bool + Send + Sync>,
    canceled: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    deadline: Instant,
}

impl DeliveryAuthority {
    #[cfg(test)]
    pub(super) fn unrestricted() -> Self {
        Self {
            current: Arc::new(|| true),
            canceled: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + DELIVERY_TIMEOUT,
        }
    }

    /// Recheck cancellation after the predicate because authority can change during it.
    pub(super) fn require_current(&self) -> Result<(), MediaPlayerError> {
        let available = || {
            !self.canceled.load(Ordering::Acquire)
                && !self.shutdown.load(Ordering::Acquire)
                && Instant::now() < self.deadline
        };
        if available() && (self.current)() && available() {
            Ok(())
        } else {
            Err(MediaPlayerError::delivery_revoked())
        }
    }
}

struct Flight(Arc<AtomicBool>);

impl Drop for Flight {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct Request {
    command: BackendCommand,
    authority: DeliveryAuthority,
    reply: mpsc::SyncSender<Result<PlayerSnapshot, MediaPlayerError>>,
    flight: Flight,
}

enum Message {
    Command(Box<Request>),
    Shutdown,
}

pub(super) struct PlaybackController {
    sender: mpsc::SyncSender<Message>,
    active: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for PlaybackController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaybackController").finish_non_exhaustive()
    }
}

impl Default for PlaybackController {
    fn default() -> Self {
        Self::with_core(PlayerCore::default())
    }
}

impl PlaybackController {
    pub(super) fn with_core(core: PlayerCore) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = shutdown.clone();
        let worker = match thread::Builder::new()
            .name("ganbaru-ai-media-player".into())
            .spawn(move || run(receiver, core, worker_shutdown))
        {
            Ok(worker) => Some(worker),
            Err(error) => {
                eprintln!("Start native Music decoder worker: {error}");
                None
            }
        };
        Self {
            sender,
            active: Arc::new(AtomicBool::new(false)),
            shutdown,
            worker,
        }
    }

    pub(super) fn dispatch(
        &self,
        command: BackendCommand,
    ) -> Result<PlayerSnapshot, MediaPlayerError> {
        self.dispatch_authorized(command, Arc::new(|| true))
    }

    pub(super) fn dispatch_authorized(
        &self,
        command: BackendCommand,
        current: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<PlayerSnapshot, MediaPlayerError> {
        self.dispatch_with_timeout(command, current, DELIVERY_TIMEOUT)
    }

    pub(super) fn dispatch_with_timeout(
        &self,
        command: BackendCommand,
        current: Arc<dyn Fn() -> bool + Send + Sync>,
        timeout: Duration,
    ) -> Result<PlayerSnapshot, MediaPlayerError> {
        self.active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| MediaPlayerError {
                code: "backendBusy".into(),
                message: "Native Music decoder work is still pending.".into(),
            })?;
        let flight = Flight(self.active.clone());
        let (reply, receiver) = mpsc::sync_channel(1);
        let canceled = Arc::new(AtomicBool::new(false));
        let authority = DeliveryAuthority {
            current,
            canceled: canceled.clone(),
            shutdown: self.shutdown.clone(),
            deadline: Instant::now() + timeout,
        };
        self.sender
            .try_send(Message::Command(Box::new(Request {
                command,
                authority,
                reply,
                flight,
            })))
            .map_err(|_| MediaPlayerError::backend_thread())?;
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(error) => {
                canceled.store(true, Ordering::Release);
                match error {
                    mpsc::RecvTimeoutError::Timeout => Err(MediaPlayerError {
                        code: "backendTimeout".into(),
                        message: "Native Music decoder execution timed out; its worker is retained until completion.".into(),
                    }),
                    mpsc::RecvTimeoutError::Disconnected => Err(MediaPlayerError::backend_thread()),
                }
            }
        }
    }
}

impl Drop for PlaybackController {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        // A stalled OS read cannot be joined safely. Its existing worker retains
        // the core and stops it when that read returns; no replacement is spawned.
        if let Err(mpsc::TrySendError::Full(_)) = self.sender.try_send(Message::Shutdown) {
            // The queued request observes shutdown before touching the decoder.
        }
        if let Some(worker) = self.worker.take()
            && worker.is_finished()
            && worker.join().is_err()
        {
            eprintln!("Native Music decoder worker terminated with a panic");
        }
    }
}

fn run(receiver: mpsc::Receiver<Message>, mut core: PlayerCore, shutdown: Arc<AtomicBool>) {
    while let Ok(message) = receiver.recv() {
        let Message::Command(request) = message else {
            break;
        };
        let Request {
            command,
            authority,
            reply,
            flight,
        } = *request;
        let mut result = authority
            .require_current()
            .and_then(|()| core.handle_authorized(command, &authority));
        if let Err(error) = authority.require_current() {
            result = Err(error);
        }
        if result.is_err() {
            core.stop();
        }
        drop(flight);
        // A canceled waiter may have gone away. Its completed result must not
        // hold the worker or prevent the retained Stop from being admitted.
        let _ = reply.try_send(result);
        if shutdown.load(Ordering::Acquire) {
            break;
        }
    }
    core.stop();
}

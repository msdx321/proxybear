use std::sync::{
    Mutex, MutexGuard,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use iced::futures::channel::mpsc;

const STATS_CHANNEL_SIZE: usize = 32;

#[derive(Debug, Clone)]
pub enum StatsEvent {
    Changed,
}

#[derive(Default)]
pub struct ProxyStats {
    ssh_connected: AtomicBool,
    bytes_up: AtomicU64,
    bytes_down: AtomicU64,
    status: Mutex<String>,
    last_error: Mutex<Option<String>>,
}

#[derive(Clone, Debug)]
pub struct StatsSnapshot {
    pub ssh_connected: bool,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub status: String,
    pub last_error: Option<String>,
}

static STATS_TX: Mutex<Option<mpsc::Sender<StatsEvent>>> = Mutex::new(None);

impl ProxyStats {
    pub fn set_status(&self, status: impl Into<String>) {
        let status = status.into();
        let mut current = lock(&self.status);
        if *current != status {
            *current = status;
            notify_changed();
        }
    }

    pub fn set_error(&self, error: impl Into<String>) {
        let error = Some(error.into());
        let mut current = lock(&self.last_error);
        if *current != error {
            *current = error;
            notify_changed();
        }
    }

    pub fn clear_error(&self) {
        let mut current = lock(&self.last_error);
        if current.take().is_some() {
            notify_changed();
        }
    }

    pub fn ssh_connected(&self) {
        if !self.ssh_connected.swap(true, Ordering::Relaxed) {
            notify_changed();
        }
    }

    pub fn ssh_disconnected(&self) {
        if self.ssh_connected.swap(false, Ordering::Relaxed) {
            notify_changed();
        }
    }

    pub fn add_up(&self, bytes: usize) {
        self.bytes_up.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn add_down(&self, bytes: usize) {
        self.bytes_down.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            ssh_connected: self.ssh_connected.load(Ordering::Relaxed),
            bytes_up: self.bytes_up.load(Ordering::Relaxed),
            bytes_down: self.bytes_down.load(Ordering::Relaxed),
            status: lock(&self.status).clone(),
            last_error: lock(&self.last_error).clone(),
        }
    }
}

#[derive(Hash)]
struct StatsSubId;

pub fn subscription() -> iced::Subscription<StatsEvent> {
    iced::Subscription::run_with(StatsSubId, |_: &StatsSubId| {
        let (tx, rx) = mpsc::channel::<StatsEvent>(STATS_CHANNEL_SIZE);
        *stats_sender() = Some(tx);
        rx
    })
}

fn notify_changed() {
    if let Some(tx) = stats_sender().as_mut() {
        let _ = tx.try_send(StatsEvent::Changed);
    }
}

fn stats_sender() -> MutexGuard<'static, Option<mpsc::Sender<StatsEvent>>> {
    STATS_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

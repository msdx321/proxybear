use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

use serde::Serialize;

#[derive(Default)]
pub struct ProxyStats {
    total_connections: AtomicU64,
    active_connections: AtomicU64,
    bytes_up: AtomicU64,
    bytes_down: AtomicU64,
    status: Mutex<String>,
    last_error: Mutex<Option<String>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StatsSnapshot {
    pub total_connections: u64,
    pub active_connections: u64,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub status: String,
    pub last_error: Option<String>,
}

impl ProxyStats {
    pub fn set_status(&self, status: impl Into<String>) {
        *self.status.lock().unwrap() = status.into();
    }

    pub fn set_error(&self, error: impl Into<String>) {
        *self.last_error.lock().unwrap() = Some(error.into());
    }

    pub fn clear_error(&self) {
        *self.last_error.lock().unwrap() = None;
    }

    pub fn connection_opened(&self) {
        self.total_connections.fetch_add(1, Ordering::Relaxed);
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn connection_closed(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn add_up(&self, bytes: usize) {
        self.bytes_up.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn add_down(&self, bytes: usize) {
        self.bytes_down.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            total_connections: self.total_connections.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            bytes_up: self.bytes_up.load(Ordering::Relaxed),
            bytes_down: self.bytes_down.load(Ordering::Relaxed),
            status: self.status.lock().unwrap().clone(),
            last_error: self.last_error.lock().unwrap().clone(),
        }
    }
}

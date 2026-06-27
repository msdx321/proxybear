use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
pub struct ProxyStats {
    ssh_current: AtomicU64,
    ssh_total: AtomicU64,
    bytes_up: AtomicU64,
    bytes_down: AtomicU64,
    status: Mutex<String>,
    last_error: Mutex<Option<String>>,
}

#[derive(Clone, Debug)]
pub struct StatsSnapshot {
    pub ssh_current: u64,
    pub ssh_total: u64,
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

    pub fn ssh_connected(&self) {
        self.ssh_current.fetch_add(1, Ordering::Relaxed);
        self.ssh_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn ssh_disconnected(&self) {
        self.ssh_current.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn add_up(&self, bytes: usize) {
        self.bytes_up.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn add_down(&self, bytes: usize) {
        self.bytes_down.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            ssh_current: self.ssh_current.load(Ordering::Relaxed),
            ssh_total: self.ssh_total.load(Ordering::Relaxed),
            bytes_up: self.bytes_up.load(Ordering::Relaxed),
            bytes_down: self.bytes_down.load(Ordering::Relaxed),
            status: self.status.lock().unwrap().clone(),
            last_error: self.last_error.lock().unwrap().clone(),
        }
    }
}

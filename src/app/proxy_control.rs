use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tokio::{
    runtime::{Builder, Runtime},
    sync::oneshot,
};

use crate::{
    config::{AppConfig, AppPaths},
    proxy,
};

use super::stats::ProxyStats;

pub struct ProxyController {
    runtime: Runtime,
    /// Present while the proxy runs and has not been asked to stop.
    shutdown: Option<oneshot::Sender<()>>,
    /// The proxy task has not finished yet; it may still be stopping.
    task_alive: bool,
    /// Start again once the stopping task finishes.
    start_pending: bool,
}

impl ProxyController {
    pub fn new() -> Result<Self> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("proxybear-proxy")
            .enable_all()
            .build()
            .context("tokio runtime")?;

        Ok(Self {
            runtime,
            shutdown: None,
            task_alive: false,
            start_pending: false,
        })
    }

    /// Whether the proxy runs and has not been asked to stop.
    pub fn is_running(&self) -> bool {
        self.shutdown.is_some()
    }

    /// Record that the proxy task ended. Returns whether a start was
    /// requested while it was stopping.
    pub fn finish(&mut self) -> bool {
        self.shutdown = None;
        self.task_alive = false;
        std::mem::take(&mut self.start_pending)
    }

    /// Start the proxy. Returns the task to watch, or `None` when it is
    /// already running or the start waits for the previous run to stop.
    pub fn start(
        &mut self,
        config: Arc<Mutex<AppConfig>>,
        paths: AppPaths,
        stats: Arc<ProxyStats>,
    ) -> Result<Option<tokio::task::JoinHandle<Result<()>>>> {
        if self.is_running() {
            return Ok(None);
        }

        config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .validate_ready()?;
        if self.task_alive {
            self.start_pending = true;
            return Ok(None);
        }
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        stats.set_status("Starting");
        stats.clear_error();
        let task = self
            .runtime
            .spawn(proxy::run_proxy(config, paths, stats, shutdown_rx));
        self.shutdown = Some(shutdown_tx);
        self.task_alive = true;
        Ok(Some(task))
    }

    pub fn stop(&mut self, stats: &ProxyStats) {
        self.start_pending = false;
        if let Some(shutdown) = self.shutdown.take() {
            stats.set_status("Stopping...");
            if shutdown.send(()).is_err() {
                stats.set_status("Stopped");
            }
        } else if !self.task_alive {
            stats.set_status("Stopped");
        }
    }
}

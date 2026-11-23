//! Controls the background proxy from the UI.

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::{
    runtime::{Builder, Runtime},
    sync::mpsc,
};

use crate::{config::AppConfig, proxy};

use super::stats::ProxyStats;

pub struct ProxyController {
    runtime: Runtime,
    /// Dropping the sender stops this run, even during a health check.
    health_checks: Option<mpsc::Sender<()>>,
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
            health_checks: None,
            task_alive: false,
            start_pending: false,
        })
    }

    /// Whether the proxy runs and has not been asked to stop.
    pub fn is_running(&self) -> bool {
        self.health_checks.is_some()
    }

    pub fn check_ssh_status(&self) {
        if let Some(requests) = &self.health_checks {
            let _ = requests.try_send(());
        }
    }

    /// Record that the proxy task ended. Returns whether a start was
    /// requested while it was stopping.
    pub fn finish(&mut self) -> bool {
        self.health_checks = None;
        self.task_alive = false;
        std::mem::take(&mut self.start_pending)
    }

    /// Start the proxy. Returns the task to watch, or `None` when it is
    /// already running or the start waits for the previous run to stop.
    pub fn start(
        &mut self,
        config: &AppConfig,
        stats: Arc<ProxyStats>,
    ) -> Result<Option<tokio::task::JoinHandle<Result<()>>>> {
        if self.is_running() {
            return Ok(None);
        }

        let runtime = config.runtime_config()?;
        if self.task_alive {
            self.start_pending = true;
            return Ok(None);
        }
        let (requests, receiver) = mpsc::channel(1);
        stats.set_status("Starting");
        stats.clear_error();
        let task = self
            .runtime
            .spawn(proxy::run_proxy(runtime, stats, receiver));
        self.health_checks = Some(requests);
        self.task_alive = true;
        Ok(Some(task))
    }

    pub fn stop(&mut self, stats: &ProxyStats) {
        self.start_pending = false;
        if let Some(requests) = self.health_checks.take() {
            stats.set_status(if requests.is_closed() {
                "Stopped"
            } else {
                "Stopping..."
            });
        } else if !self.task_alive {
            stats.set_status("Stopped");
        }
    }
}

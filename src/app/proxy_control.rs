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

#[derive(Debug, Clone)]
pub enum ProxyEvent {
    Done(Option<String>),
}

pub struct ProxyController {
    runtime: Runtime,
    handle: Option<ProxyHandle>,
}

struct ProxyHandle {
    shutdown: Option<oneshot::Sender<()>>,
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
            handle: None,
        })
    }

    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }

    pub fn finish(&mut self) {
        self.handle = None;
    }

    pub fn start(
        &mut self,
        config: Arc<Mutex<AppConfig>>,
        paths: AppPaths,
        stats: Arc<ProxyStats>,
    ) -> Result<iced::Task<ProxyEvent>> {
        if self.handle.is_some() {
            return Ok(iced::Task::none());
        }

        config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .validate_ready()?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        stats.set_status("Starting");
        stats.clear_error();
        let task = self
            .runtime
            .spawn(proxy::run_proxy(config, paths, stats, shutdown_rx));
        self.handle = Some(ProxyHandle {
            shutdown: Some(shutdown_tx),
        });
        Ok(iced::Task::perform(
            async move { task.await.context("proxy task failed")? },
            |result| ProxyEvent::Done(result.err().map(|error| error.to_string())),
        ))
    }

    pub fn stop(&mut self, stats: &ProxyStats) {
        if let Some(handle) = self.handle.as_mut() {
            if let Some(shutdown) = handle.shutdown.take() {
                stats.set_status("Stopping...");
                if shutdown.send(()).is_err() {
                    stats.set_status("Stopped");
                }
            }
        } else {
            stats.set_status("Stopped");
        }
    }
}

use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Context, Result};
use iced::futures::channel::mpsc;
use tokio::{
    runtime::{Builder, Runtime},
    sync::oneshot,
    task::JoinHandle,
};

use crate::{
    config::{AppConfig, AppPaths},
    proxy,
};

use super::stats::ProxyStats;
const PROXY_CHANNEL_SIZE: usize = 32;

#[derive(Debug, Clone)]
pub enum ProxyEvent {
    Done(Option<String>),
}

static PROXY_TX: Mutex<Option<mpsc::Sender<ProxyEvent>>> = Mutex::new(None);

pub struct ProxyController {
    runtime: Runtime,
    handle: Option<ProxyHandle>,
}

struct ProxyHandle {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
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

    pub fn reap_finished(&mut self) {
        if self
            .handle
            .as_ref()
            .is_some_and(|handle| handle.task.is_finished())
        {
            self.handle = None;
        }
    }

    pub fn start(
        &mut self,
        config: Arc<Mutex<AppConfig>>,
        paths: AppPaths,
        stats: Arc<ProxyStats>,
    ) -> Result<()> {
        if self.handle.is_some() {
            return Ok(());
        }

        config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .validate_ready()?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        stats.set_status("Starting");
        stats.clear_error();
        let task = self.runtime.spawn(async move {
            let result = proxy::run_proxy(config, paths, Arc::clone(&stats), shutdown_rx).await;
            stats.set_status("Stopped");
            if let Some(tx) = proxy_sender().as_mut() {
                let _ = tx.try_send(ProxyEvent::Done(
                    result.err().map(|error| error.to_string()),
                ));
            }
        });
        self.handle = Some(ProxyHandle {
            shutdown: Some(shutdown_tx),
            task,
        });
        Ok(())
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

#[derive(Hash)]
struct ProxySubId;

pub fn subscription() -> iced::Subscription<ProxyEvent> {
    iced::Subscription::run_with(ProxySubId, |_: &ProxySubId| {
        let (tx, rx) = mpsc::channel::<ProxyEvent>(PROXY_CHANNEL_SIZE);
        *proxy_sender() = Some(tx);
        rx
    })
}

fn proxy_sender() -> MutexGuard<'static, Option<mpsc::Sender<ProxyEvent>>> {
    PROXY_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex, MutexGuard},
};

use anyhow::Result;
use tokio::sync::watch;
use tracing_subscriber::{
    Registry, filter::LevelFilter, fmt, layer::SubscriberExt, reload, util::SubscriberInitExt,
};

use crate::config::LogLevel;

pub type LogFilter = reload::Handle<LevelFilter, Registry>;

const LOG_MAX_SIZE: u64 = 1024 * 1024;
static LOG_CHANGED: LazyLock<watch::Sender<()>> = LazyLock::new(|| watch::channel(()).0);

pub fn subscribe() -> watch::Receiver<()> {
    LOG_CHANGED.subscribe()
}

pub fn init(config_dir: &Path, level: LogLevel) -> Result<LogFilter> {
    fs::create_dir_all(config_dir)?;
    let log_writer = SharedWriter(Mutex::new(RotatingWriter::new(
        config_dir.join("proxybear.log"),
        LOG_MAX_SIZE,
    )?));
    let (filter, handle) = reload::Layer::new(level.filter());

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_ansi(false).with_writer(log_writer))
        .try_init()?;
    Ok(handle)
}

/// A file writer that rotates to `.old.log` when it exceeds `max_size` bytes,
/// keeping disk usage bounded.
struct RotatingWriter {
    file: fs::File,
    path: PathBuf,
    written: u64,
    max_size: u64,
}

struct SharedWriter(Mutex<RotatingWriter>);

impl<'a> fmt::MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriterGuard<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriterGuard(self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

struct SharedWriterGuard<'a>(MutexGuard<'a, RotatingWriter>);

impl Write for SharedWriterGuard<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.0.write(buf)?;
        let _ = LOG_CHANGED.send(());
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl RotatingWriter {
    fn new(path: PathBuf, max_size: u64) -> io::Result<Self> {
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let written = file.metadata()?.len();
        Ok(Self {
            file,
            path,
            written,
            max_size,
        })
    }
}

impl Write for RotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written + buf.len() as u64 > self.max_size {
            let old_path = self.path.with_extension("old.log");
            let _ = fs::remove_file(&old_path);
            if fs::rename(&self.path, &old_path).is_err() {
                // Rename failed. Truncate in place as fallback.
                self.file = fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&self.path)?;
            } else {
                self.file = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)?;
            }
            self.written = 0;
        }
        let n = self.file.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

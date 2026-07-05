use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

const LOG_MAX_SIZE: u64 = 1024 * 1024;
const DEFAULT_LOG_FILTER: &str = "warn,proxybear=info,iced=warn,wgpu=warn,naga=warn";

pub fn init(config_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(config_dir)?;
    let log_writer = RotatingWriter::new(config_dir.join("proxybear.log"), LOG_MAX_SIZE)?;
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(DEFAULT_LOG_FILTER))
        .target(env_logger::Target::Pipe(Box::new(log_writer)))
        .init();
    Ok(())
}

/// A file writer that rotates to `.old.log` when it exceeds `max_size` bytes,
/// keeping disk usage bounded.
struct RotatingWriter {
    file: fs::File,
    path: PathBuf,
    written: u64,
    max_size: u64,
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

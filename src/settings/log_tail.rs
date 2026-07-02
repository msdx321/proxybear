use std::{
    collections::VecDeque,
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MAX_LOG_LINES: usize = 400;

pub struct LogTail {
    path: PathBuf,
    path_label: String,
    offset: u64,
    lines: VecDeque<String>,
    status: String,
    error: Option<String>,
}

impl LogTail {
    pub fn new(path: PathBuf) -> Self {
        let path_label = path.display().to_string();
        Self {
            path,
            path_label,
            offset: 0,
            lines: VecDeque::new(),
            status: "Waiting for log file".into(),
            error: None,
        }
    }

    pub fn refresh(&mut self) -> usize {
        match self.read_new() {
            Ok(0) if self.lines.is_empty() => {
                self.error = None;
                self.status = "Log file is empty".into();
                0
            }
            Ok(0) => {
                self.error = None;
                self.status = format!("Live, {} lines", self.lines.len());
                0
            }
            Ok(added) => {
                self.error = None;
                self.status = format!("Live, {} lines, {added} new", self.lines.len());
                added
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.offset = 0;
                self.error = None;
                self.status = "No log file yet".into();
                0
            }
            Err(error) => {
                self.error = Some(format!("Could not read log: {error}"));
                self.status = "Log read error".into();
                0
            }
        }
    }

    pub fn clear(&mut self) -> io::Result<()> {
        fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        self.offset = 0;
        self.lines.clear();
        self.error = None;
        self.status = "Log cleared".into();
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn path_label(&self) -> &str {
        &self.path_label
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn lines(&self) -> &VecDeque<String> {
        &self.lines
    }

    fn read_new(&mut self) -> io::Result<usize> {
        let metadata = fs::metadata(&self.path)?;
        let len = metadata.len();
        if len < self.offset {
            self.offset = 0;
            self.lines.clear();
            self.status = "Log rotated or truncated".into();
        }

        let mut file = fs::File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.offset))?;

        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        self.offset = len;

        let mut added = 0;
        for line in String::from_utf8_lossy(&bytes).lines() {
            self.push_line(line);
            added += 1;
        }
        Ok(added)
    }

    fn push_line(&mut self, line: &str) {
        while self.lines.len() >= MAX_LOG_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line.to_string());
    }
}

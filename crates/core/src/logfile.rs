//! The app's log file. A GUI build has no visible stderr, so this is the only record of why something failed
//! on a user's machine. Captions are never logged.
use log::{Level, Log, Metadata, Record};
use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

const CURRENT: &str = "overlaysubs.log";
/// The launch before this one, kept because a crash is usually looked at after a restart.
const PREVIOUS: &str = "overlaysubs.old.log";
/// Stop writing past this, so a failure on every utterance cannot fill the disk. The start of the log is what is kept.
const MAX_BYTES: u64 = 8 << 20;
/// INFO lines of whisper.cpp / llama.cpp held back in memory; see [`LogFile`].
const RECENT: usize = 200;
/// Distinct warnings of other crates that are written once each; see [`LogFile`]. Past this they are only held back.
const WARNED: usize = 256;
/// How much of each file [`read`] returns.
const TAIL_BYTES: u64 = 512 << 10;

/// Writes the app's own messages at INFO and above, and every other crate's at WARN and above. whisper.cpp and
/// llama.cpp print dozens of INFO lines per utterance, too many to keep, but they hold the sizes and devices that
/// explain a failure. So the last [`RECENT`] of them stay in memory and are written just before the next ERROR.
/// A warning of theirs is written the first time only and held back like INFO after that: llama.cpp repeats one
/// on every translation (measured), which would fill the file within hours and leave no room for a later error.
pub struct LogFile {
    state: Mutex<State>,
    start: Instant,
    /// Written as `~`, so a log posted in a public issue does not carry the user's account name.
    home: Option<String>,
}

struct State {
    out: File,
    written: u64,
    recent: VecDeque<String>,
    warned: HashSet<String>,
}

impl LogFile {
    /// Starts a new log in `dir`, keeping the last launch's as the previous one.
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let _ = std::fs::rename(dir.join(CURRENT), dir.join(PREVIOUS));
        let out = File::create(dir.join(CURRENT))?;
        // A home of "/" (or none) would only garble every path.
        let home = std::env::home_dir().map(|h| h.to_string_lossy().into_owned()).filter(|h| h.len() > 1);
        Ok(Self { state: Mutex::new(State { out, written: 0, recent: VecDeque::new(), warned: HashSet::new() }), start: Instant::now(), home })
    }
}

impl State {
    fn write(&mut self, line: &str) {
        if self.written > MAX_BYTES {
            return;
        }
        self.written += line.len() as u64 + 1;
        let line = if self.written > MAX_BYTES { "log is full; later messages are dropped" } else { line };
        eprintln!("{line}");
        let _ = writeln!(self.out, "{line}");
    }
}

impl Log for LogFile {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= Level::Info
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        // Seconds since launch: enough to order events, and needs no date library.
        let line = format!("[{:9.3}] {:5} {}: {}", self.start.elapsed().as_secs_f64(), r.level(), r.target(), r.args());
        let line = match &self.home {
            Some(home) => line.replace(home.as_str(), "~"),
            None => line,
        };
        // Never panic here: llama.cpp calls this through a C callback.
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let app = r.target().starts_with("overlaysubs");
        let repeated = !app && r.level() == Level::Warn && (s.warned.len() >= WARNED || !s.warned.insert(format!("{} {}", r.target(), r.args())));
        if (r.level() <= Level::Warn && !repeated) || app {
            if r.level() == Level::Error {
                while let Some(l) = s.recent.pop_front() {
                    s.write(&l);
                }
            }
            s.write(&line);
        } else {
            if s.recent.len() == RECENT {
                s.recent.pop_front();
            }
            s.recent.push_back(line);
        }
    }

    fn flush(&self) {}
}

/// The end of the previous launch's log and of this one's, for the About page and for a bug report.
pub fn read(dir: &Path) -> String {
    let tail = |name: &str| {
        let mut f = File::open(dir.join(name)).ok()?;
        let len = f.metadata().ok()?.len();
        f.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES))).ok()?;
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    };
    format!("=== previous launch ===\n{}\n=== this launch ===\n{}", tail(PREVIOUS).unwrap_or_default(), tail(CURRENT).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(f: &LogFile, level: Level, target: &str, msg: &str) {
        f.log(&Record::builder().level(level).target(target).args(format_args!("{msg}")).build());
    }

    #[test]
    fn native_info_is_written_only_with_the_error_it_led_to() {
        let dir = std::env::temp_dir().join(format!("overlaysubs-logfile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let f = LogFile::open(&dir).unwrap();
        log(&f, Level::Info, "overlaysubs", "started");
        if let Some(home) = std::env::home_dir().filter(|h| h.as_os_str().len() > 1) {
            log(&f, Level::Info, "overlaysubs", &format!("model at {}", home.join("models/a.gguf").display()));
            let sep = std::path::MAIN_SEPARATOR;
            assert!(read(&dir).contains(&format!("model at ~{sep}models{sep}a.gguf")), "the home folder is written as ~");
        }
        log(&f, Level::Info, "llama.cpp", "quiet detail");
        log(&f, Level::Debug, "overlaysubs", "debug noise");
        assert!(read(&dir).contains("started") && !read(&dir).contains("quiet detail"));
        log(&f, Level::Warn, "llama.cpp", "a warning");
        assert!(read(&dir).contains("a warning") && !read(&dir).contains("quiet detail"), "a warning does not flush the held lines");
        log(&f, Level::Warn, "llama.cpp", "a warning");
        assert_eq!(read(&dir).matches("a warning").count(), 1, "a repeated warning is held back");
        log(&f, Level::Error, "llama.cpp", "failed to initialize the context");
        let text = read(&dir);
        assert!(text.find("quiet detail").unwrap() < text.find("failed to initialize the context").unwrap());
        assert_eq!(text.matches("a warning").count(), 2, "the held warning is written with the error");
        assert!(!text.contains("debug noise"));

        // The next launch keeps this one as the previous log.
        drop(f);
        let f = LogFile::open(&dir).unwrap();
        log(&f, Level::Info, "overlaysubs", "second launch");
        let text = read(&dir);
        assert!(text.find("failed to initialize the context").unwrap() < text.find("=== this launch ===").unwrap());
        assert!(text.find("second launch").unwrap() > text.find("=== this launch ===").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

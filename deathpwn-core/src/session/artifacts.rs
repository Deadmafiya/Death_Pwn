//! Per-session artifact directory and command-output persistence.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::clock::Clock;
use crate::error::Result;
use crate::exec::RunOutcome;

/// Per-session artifact directory. Each command's raw output is written to a
/// numbered file under `session_dir` for later review/export.
#[derive(Debug, Clone)]
pub struct Artifacts {
    root: PathBuf,
    session_dir: PathBuf,
}

impl Artifacts {
    /// Open (creating if needed) a session directory at `root/<now_ms>`.
    /// The timestamp comes from the injected clock so tests are deterministic.
    pub fn open(root: PathBuf, clock: &dyn Clock) -> Result<Artifacts> {
        let session_dir = root.join(clock.now_ms().to_string());
        fs::create_dir_all(&session_dir)?;
        Ok(Artifacts { root, session_dir })
    }

    /// Absolute path to this session's directory.
    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    /// Root artifacts directory shared across sessions.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Write one command's captured output to `session_dir/<index>.txt`,
    /// returning the path written. Includes exit code, cancellation flag, and
    /// both output streams.
    pub fn write_output(&self, index: usize, outcome: &RunOutcome) -> Result<PathBuf> {
        let path = self.session_dir.join(format!("{index}.txt"));
        let mut file = fs::File::create(&path)?;
        let exit = match outcome.exit {
            Some(code) => code.to_string(),
            None => "none".to_string(),
        };
        writeln!(file, "exit: {exit}")?;
        writeln!(file, "cancelled: {}", outcome.cancelled)?;
        writeln!(file, "--- stdout ---")?;
        file.write_all(outcome.stdout.as_bytes())?;
        writeln!(file, "\n--- stderr ---")?;
        file.write_all(outcome.stderr.as_bytes())?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;

    #[test]
    fn open_creates_session_dir_named_by_clock() {
        let tmp = tempfile::tempdir().unwrap();
        let clock = FakeClock::fixed(1_700_000_000_000);

        let artifacts = Artifacts::open(tmp.path().to_path_buf(), &clock).unwrap();

        let expected = tmp.path().join("1700000000000");
        assert_eq!(artifacts.session_dir(), expected.as_path());
        assert!(expected.is_dir());
    }

    #[test]
    fn write_output_persists_numbered_file_with_streams() {
        let tmp = tempfile::tempdir().unwrap();
        let clock = FakeClock::fixed(1_700_000_000_000);
        let artifacts = Artifacts::open(tmp.path().to_path_buf(), &clock).unwrap();

        let outcome = RunOutcome {
            exit: Some(0),
            stdout: "PORT 80 open\n".to_string(),
            stderr: "warning: slow\n".to_string(),
            cancelled: false,
        };

        let path = artifacts.write_output(1, &outcome).unwrap();

        assert_eq!(path, artifacts.session_dir().join("1.txt"));
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("exit: 0"));
        assert!(contents.contains("cancelled: false"));
        assert!(contents.contains("PORT 80 open"));
        assert!(contents.contains("warning: slow"));
    }
}

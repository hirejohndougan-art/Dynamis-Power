//! Read-only access to kernel interfaces under a configurable root.
//!
//! Every sensor reads through [`SysRoot`] instead of touching `/sys` or `/proc`
//! directly. In production the root is `/`; in tests it is a fixture directory
//! under `tests/fixtures/sysfs/<machine>/`. This is what lets the whole crate be
//! tested without real hardware.
//!
//! There is deliberately **no write method** here. Phase 1 is read-only, and
//! when writing arrives it will live in setting plugins that journal first
//! (see `docs/SAFETY.md`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Why a value could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The file or directory does not exist on this machine.
    Missing,
    /// It exists but this user may not read it (often needs root).
    PermissionDenied,
    /// It exists but its contents could not be parsed.
    Invalid(String),
    /// Any other I/O failure.
    Io(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Missing => write!(f, "not present"),
            ReadError::PermissionDenied => write!(f, "permission denied (try running as root)"),
            ReadError::Invalid(s) => write!(f, "unreadable value: {s}"),
            ReadError::Io(s) => write!(f, "I/O error: {s}"),
        }
    }
}

impl std::error::Error for ReadError {}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::NotFound => ReadError::Missing,
            io::ErrorKind::PermissionDenied => ReadError::PermissionDenied,
            _ => ReadError::Io(e.to_string()),
        }
    }
}

pub type ReadResult<T> = Result<T, ReadError>;

/// A root directory that kernel paths such as `/sys/class/power_supply` are
/// resolved against.
#[derive(Debug, Clone)]
pub struct SysRoot {
    root: PathBuf,
}

impl SysRoot {
    /// The real system: paths resolve as written.
    pub fn system() -> Self {
        Self {
            root: PathBuf::from("/"),
        }
    }

    /// A fixture or other alternative root.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve an absolute kernel path (for example `/sys/class/thermal`) under
    /// this root.
    pub fn path(&self, kernel_path: &str) -> PathBuf {
        self.root.join(kernel_path.trim_start_matches('/'))
    }

    pub fn exists(&self, kernel_path: &str) -> bool {
        self.path(kernel_path).exists()
    }

    /// Read a file as trimmed text.
    pub fn read_string(&self, kernel_path: &str) -> ReadResult<String> {
        let s = fs::read_to_string(self.path(kernel_path))?;
        Ok(s.trim().to_string())
    }

    /// Read a file holding one unsigned integer.
    pub fn read_u64(&self, kernel_path: &str) -> ReadResult<u64> {
        let s = self.read_string(kernel_path)?;
        s.parse::<u64>()
            .map_err(|_| ReadError::Invalid(format!("{kernel_path}: {s:?}")))
    }

    /// Read a file holding one signed integer.
    pub fn read_i64(&self, kernel_path: &str) -> ReadResult<i64> {
        let s = self.read_string(kernel_path)?;
        s.parse::<i64>()
            .map_err(|_| ReadError::Invalid(format!("{kernel_path}: {s:?}")))
    }

    /// Names of the entries in a directory, sorted. Missing directories give an
    /// empty list, since "no such devices" is a normal answer.
    pub fn list_dir(&self, kernel_path: &str) -> Vec<String> {
        let Ok(rd) = fs::read_dir(self.path(kernel_path)) else {
            return Vec::new();
        };
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The final component of a symlink's target, for example the driver name
    /// behind `/sys/class/net/wlan0/device/driver`.
    pub fn link_name(&self, kernel_path: &str) -> ReadResult<String> {
        let target = fs::read_link(self.path(kernel_path))?;
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| ReadError::Invalid(kernel_path.to_string()))
    }
}

/// Parse a sysfs list where the active choice is shown in brackets, such as
/// `[mq-deadline] none` or `lzo [zstd] lz4`. Returns (active, all choices).
pub fn parse_bracketed(s: &str) -> (Option<String>, Vec<String>) {
    let mut active = None;
    let mut all = Vec::new();
    for tok in s.split_whitespace() {
        if let Some(inner) = tok.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
            active = Some(inner.to_string());
            all.push(inner.to_string());
        } else {
            all.push(tok.to_string());
        }
    }
    (active, all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracketed_lists() {
        let (a, all) = parse_bracketed("[mq-deadline] kyber none");
        assert_eq!(a.as_deref(), Some("mq-deadline"));
        assert_eq!(all, vec!["mq-deadline", "kyber", "none"]);
        let (a, _) = parse_bracketed("lzo lz4");
        assert_eq!(a, None);
    }

    #[test]
    fn path_joins_under_root() {
        let r = SysRoot::at("/tmp/fixture");
        assert_eq!(
            r.path("/sys/class"),
            PathBuf::from("/tmp/fixture/sys/class")
        );
    }
}

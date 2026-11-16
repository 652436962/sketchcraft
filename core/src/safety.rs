//! Small guards for handing user paths to other programs.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A path that can't be read as an option: other programs (ffmpeg, open,
/// the DWG converters) treat an argument starting with `-` as a flag.
pub fn path_arg(p: impl AsRef<Path>) -> PathBuf {
    let p = p.as_ref();
    if p.to_string_lossy().starts_with('-') {
        Path::new(".").join(p)
    } else {
        p.to_path_buf()
    }
}

/// A per-user folder for caches and work in progress that must outlive one
/// call. On Unix a shared /tmp lets other users pre-create a fixed name, so
/// it lives under the user's own ~/.cache instead (private, 0700, never a
/// symlink); elsewhere the temp folder is already per-user.
pub fn cache_dir(name: &str) -> PathBuf {
    let base = {
        #[cfg(unix)]
        {
            std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"))).map(|d| d.join("sketchcraft"))
        }
        #[cfg(not(unix))]
        {
            None::<PathBuf>
        }
    };
    let Some(base) = base else { return std::env::temp_dir().join(format!("sketchcraft-{name}")) };
    let dir = base.join(name);
    // make it private; if something odd (a symlink) is in the way, use a fresh temp one
    let is_link = std::fs::symlink_metadata(&dir).map(|m| m.file_type().is_symlink()).unwrap_or(false);
    if is_link || std::fs::create_dir_all(&dir).is_err() {
        return std::env::temp_dir().join(format!("sketchcraft-{name}-{}", std::process::id()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok();
    }
    dir
}

static TMP_N: AtomicU64 = AtomicU64::new(0);

/// A unique, private scratch folder (random-ish name, created fresh, owner
/// only on Unix), removed when dropped. Replaces fixed names in a shared /tmp.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(prefix: &str) -> std::io::Result<Self> {
        let base = std::env::temp_dir();
        for _ in 0..64 {
            let n = TMP_N.fetch_add(1, Ordering::Relaxed);
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
            let p = base.join(format!("{prefix}-{}-{n}-{nanos:x}", std::process::id()));
            // create_new semantics: fails if anything is already there
            match std::fs::create_dir(&p) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).ok();
                    }
                    return Ok(TempDir(p));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::other("couldn't make a temporary folder"))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Write a file so it is either the old one or the whole new one, never half
/// of it: write beside it, flush to disk, then rename over it.
pub fn write_atomic(path: impl AsRef<Path>, bytes: &[u8]) -> std::io::Result<()> {
    let path = path.as_ref();
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let tmp = dir.join(format!(".{name}.tmp-{}-{}", std::process::id(), TMP_N.fetch_add(1, Ordering::Relaxed)));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        std::fs::remove_file(&tmp).ok();
    } else {
        #[cfg(unix)]
        if let Ok(d) = std::fs::File::open(dir) {
            d.sync_all().ok();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dashes_are_not_flags() {
        assert_eq!(path_arg("-i evil.mp4"), PathBuf::from("./-i evil.mp4"));
        assert_eq!(path_arg("/tmp/a.mp4"), PathBuf::from("/tmp/a.mp4"));
        assert_eq!(path_arg("a-b.mp4"), PathBuf::from("a-b.mp4"));
    }
}

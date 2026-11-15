// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/hook/src/unix.rs, translated to macOS from NotchBuddy/Sources/App/HookServer.swift @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
//! The macOS transport (Next Agent; Coucou's relay has Windows and Linux
//! only, and its Mac app has a relay script of its own). A Unix socket in the
//! app's private support folder, as HookServer.swift keeps `nb.sock` in its
//! support directory.
//!
//! The folder is ours and closed to everyone else (0700), so nobody else can
//! put a socket there or reach ours. Once connected we check that the socket
//! file belongs to us too. All of it with std and rustix: no `unsafe`.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::CONNECT_TIMEOUT;

/// `~/Library/Application Support/dev.nextagent.desktop/overlay/relay.sock`.
/// Next Agent: renamed from Coucou (owner request): was `…/coucou/coucou.sock`.
/// Must match `platform::relay_socket_path()` in the app exactly.
fn socket_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())?;
    let dir = home.join("Library/Application Support/dev.nextagent.desktop/overlay");
    is_private_dir(&dir).then(|| dir.join("relay.sock"))
}

fn my_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// A real directory (not a symlink), owned by us, no access for group or others.
fn is_private_dir(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(dir)
        .map(|m| m.file_type().is_dir() && m.uid() == my_uid() && m.mode() & 0o077 == 0)
        .unwrap_or(false)
}

/// A socket file of ours: not a symlink, owned by us.
fn is_our_socket(path: &Path) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_socket() && m.uid() == my_uid())
        .unwrap_or(false)
}

/// Opens the socket. Retries only while the app's backlog is full (macOS
/// answers ECONNREFUSED then too, so only until the short deadline): any other
/// error means there is nothing to talk to, and waiting would only delay
/// Claude Code.
pub fn connect() -> Option<UnixStream> {
    let path = socket_path()?;
    if !is_our_socket(&path) {
        return None;
    }
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match UnixStream::connect(&path) {
            Ok(stream) => return Some(stream),
            Err(err) => {
                let busy = matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::ConnectionRefused
                );
                if !busy || Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        }
    }
}

/// The process that ran the relay (Claude Code, or the shell it used).
pub struct Parent(u32);

impl Parent {
    /// True once it has exited: we were handed to launchd, so our parent is no
    /// longer the one we started with.
    pub fn gone(&self) -> bool {
        std::os::unix::process::parent_id() != self.0
    }
}

pub fn parent() -> Option<Parent> {
    Some(Parent(std::os::unix::process::parent_id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_path_fits_in_sun_path() {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
        let path =
            home.join("Library/Application Support/dev.nextagent.desktop/overlay/relay.sock");
        assert!(path.as_os_str().len() < 104);
    }

    #[test]
    fn a_folder_open_to_others_is_not_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("next-agent-hook-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!is_private_dir(&dir));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(is_private_dir(&dir));
        assert!(!is_our_socket(&dir.join("nothing.sock")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

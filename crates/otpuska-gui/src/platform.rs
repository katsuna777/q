//! Opening files and folders with the system's default programs (macOS, Windows, Linux).

use std::path::Path;
use std::process::Command;

/// Spawn without flashing a console window on Windows.
#[allow(unused_mut)] // `mut` is only needed for the Windows flags
fn spawn(mut cmd: Command) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn().map(|_| ())
}

pub fn open(path: &Path) -> std::io::Result<()> {
    let cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(path);
        c
    } else if cfg!(target_os = "windows") {
        // Opens with the default program; unlike `cmd /C start` it has no quoting pitfalls
        // with `&` or spaces in the path.
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler").arg(path);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(path);
        c
    };
    spawn(cmd)
}

/// Show a file in Finder / Explorer, or open a folder.
pub fn reveal(path: &Path) -> std::io::Result<()> {
    if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        if path.is_file() {
            c.arg("-R");
        }
        c.arg(path);
        spawn(c)
    } else if cfg!(target_os = "windows") {
        let mut c = Command::new("explorer");
        if path.is_file() {
            c.arg("/select,");
        }
        c.arg(path);
        spawn(c)
    } else {
        open(if path.is_file() { path.parent().unwrap_or(path) } else { path })
    }
}

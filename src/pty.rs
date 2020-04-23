// Copyright 2020 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Wrappers for POSIX psuedoterminals.
//!
//! Pseudoterminals are an older subsystem and refer to the pseudoterminal
//! pairs as "master" and "slave". This module refers to them as "parent pty"
//! and "child pty".

use std::fmt;
use std::fs::File;
use std::io;
use std::mem;
use std::os::raw::{c_char, c_ushort};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::process::Stdio;
use std::result;

/// Errors that can be encountered by a Pty.
#[remain::sorted]
#[derive(Debug)]
pub enum PtyError {
    GetPtyName(io::Error),
    GrantPt(io::Error),
    OpenPtyChild(io::Error),
    OpenPtyParent(io::Error),
    SetControllingTty(io::Error),
    SetDimensions(io::Error),
    UnlockPt(io::Error),
}

type Result<T> = result::Result<T, PtyError>;

impl fmt::Display for PtyError {
    #[remain::check]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::PtyError::*;

        #[remain::sorted]
        match self {
            GetPtyName(e) => write!(f, "failed to get pt name: {}", e),
            GrantPt(e) => write!(f, "failed to grant pt: {}", e),
            OpenPtyChild(e) => write!(f, "failed to open pty child: {}", e),
            OpenPtyParent(e) => write!(f, "failed to open pty parent: {}", e),
            SetControllingTty(e) => write!(f, "failed to set controlling tty: {}", e),
            SetDimensions(e) => write!(f, "failed to set pty dimensions: {}", e),
            UnlockPt(e) => write!(f, "failed to unlock pt: {}", e),
        }
    }
}

pub struct PtyParent {
    fd: RawFd,
}

impl PtyParent {
    pub fn new() -> Result<Self> {
        // Safe because the posix_openpt function modifies no memory and the
        // return value is checked.
        let fd = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(PtyError::OpenPtyParent(io::Error::last_os_error()));
        }

        // Put the pty fd into a File for now so it's closed on all exit paths.
        // Safe because we know fd is a valid pseudoterminal fd.
        let pty_parent = unsafe { File::from_raw_fd(fd) };

        // Safe because the grantpt function modifies no memory and the
        // return value is checked.
        let ret = unsafe { libc::grantpt(pty_parent.as_raw_fd()) };
        if ret < 0 {
            return Err(PtyError::GrantPt(io::Error::last_os_error()));
        }

        // Safe because the unlockpt function modifies no memory and the
        // return value is checked.
        let ret = unsafe { libc::unlockpt(pty_parent.as_raw_fd()) };
        if ret < 0 {
            return Err(PtyError::UnlockPt(io::Error::last_os_error()));
        }


        Ok(PtyParent {
            fd: pty_parent.into_raw_fd(),
        })
    }

    /// Sets the dimensions of the pseudoterminal window.
    pub fn set_dimensions(&mut self, rows: c_ushort, cols: c_ushort) -> Result<()> {
        let winsize = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // Safe because this ioctl modifies no memory and the return value is
        // checked.
        let ret = unsafe { libc::ioctl(self.fd, libc::TIOCSWINSZ, &winsize) };
        if ret < 0 {
            return Err(PtyError::SetDimensions(io::Error::last_os_error()));
        }

        Ok(())
    }

    /// Opens a child pseudoterminal connected to this parent.
    pub fn open_child(&mut self) -> Result<PtyChild> {
        let mut pt_name = [0u8; 64];

        // Safe because the ptsname_r function only modifies a slice that has
        // the current address and length, and the return value is checked.
        let ret = unsafe { libc::ptsname_r(self.fd, pt_name.as_mut_ptr() as *mut c_char, pt_name.len()) };
        if ret < 0 {
            return Err(PtyError::GetPtyName(io::Error::last_os_error()));
        }

        // Safe because the open function is given a valid path from ptsname_r,
        // modifies no memory, and the return value is checked.
        let fd = unsafe { libc::open(pt_name.as_ptr() as *const c_char, libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(PtyError::OpenPtyChild(io::Error::last_os_error()));
        }

        Ok(PtyChild{
            fd,
        })
    }
}

impl Drop for PtyParent {
    fn drop(&mut self) {
        // Safe because no memory is modified. Don't bother checking the return
        // value since nothing useful can be done about it in drop.
        unsafe { libc::close(self.fd) };
    }
}

impl AsRawFd for PtyParent {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

pub struct PtyChild {
    fd: RawFd,
}

impl PtyChild {
    /// Sets this pseudoterminal as the controlling tty for the current process.
    pub fn set_controlling_tty(&mut self) -> Result<()> {
        // Safe because this ioctl modifies no memory and the return value is
        // checked.
        let ret = unsafe { libc::ioctl(self.fd, libc::TIOCSCTTY) };
        if ret < 0 {
            return Err(PtyError::SetControllingTty(io::Error::last_os_error()));
        }

        Ok(())
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        // Safe because no memory is modified. Don't bother checking the return
        // value since nothing useful can be done about it in drop.
        unsafe { libc::close(self.fd) };
    }
}

impl AsRawFd for PtyChild {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl IntoRawFd for PtyChild {
    fn into_raw_fd(self) -> RawFd {
        let fd = self.fd;
        mem::forget(self);
        fd
    }
}

impl From<PtyChild> for Stdio {
    fn from(pty_child: PtyChild) -> Self {
        unsafe { Stdio::from_raw_fd(pty_child.into_raw_fd()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_pty_parent() {
        PtyParent::new().expect("create new PtyParent");
    }

    #[test]
    fn set_dimensions() {
        let mut pty_parent = PtyParent::new().expect("create new PtyParent");
        pty_parent.set_dimensions(80, 25).expect("set pty dimensions");
    }

    #[test]
    fn open_child() {
        let mut pty_parent = PtyParent::new().expect("create new PtyParent");
        let pty_child = pty_parent.open_child().expect("open pty child");
        let _stdio: Stdio = pty_child.into();
    }
}

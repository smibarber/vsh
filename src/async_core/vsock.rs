// Copyright 2020 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::convert::TryFrom;
use std::fmt::{self, Display};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::io::{AsyncRead, AsyncWrite, Error as IoError, ErrorKind as IoErrorKind};

use libchromeos::vsock;

use cros_async::fd_executor::{add_read_waker, add_write_waker};

/// Errors generated while polling for signals.
#[derive(Debug)]
pub enum Error {
    /// An error occurred while setting the vsock stream as nonblocking.
    SetNonblocking(std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Error::*;

        match self {
            SetNonblocking(e) => write!(
                f,
                "An error occurred while setting the vsock stream as nonblocking: {}.",
                e
            ),
        }
    }
}

pub struct VsockStream {
    inner: vsock::VsockStream,
}

impl TryFrom<vsock::VsockStream> for VsockStream {
    type Error = crate::async_core::vsock::Error;

    fn try_from(mut vsock_stream: vsock::VsockStream) -> Result<VsockStream> {
        vsock_stream.set_nonblocking(true).map_err(Error::SetNonblocking)?;
        Ok(VsockStream {
            inner: vsock_stream,
        })
    }
}

impl AsyncRead for VsockStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8]) -> Poll<std::result::Result<usize, IoError>> {
        let res = self.inner.read(buf);

        match res {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                match add_read_waker(self.inner.as_raw_fd(), cx.waker().clone()) {
                    Ok(_) => Poll::Pending,
                    Err(_) => {
                        // TODO(smbarber): convert fd_executor::Error here.
                        Poll::Ready(Err(IoError::new(IoErrorKind::Other, "failed to add read waker")))
                    },
                }
            },
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl AsyncWrite for VsockStream {
    fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[u8],
            ) -> Poll<std::result::Result<usize, IoError>> {
        let res = self.inner.write(buf);

        match res {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                match add_write_waker(self.inner.as_raw_fd(), cx.waker().clone()) {
                    Ok(_) => Poll::Pending,
                    Err(_) => {
                        // TODO(smbarber): convert fd_executor::Error here.
                        Poll::Ready(Err(IoError::new(IoErrorKind::Other, "failed to add write waker")))
                    },
                }
            },
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<std::result::Result<(), IoError>> {
        // Sockets don't need to flush writes.
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::result::Result<(), IoError>> {
        // We don't expect closing to block. Closing is performed when the socket is dropped.
        self.poll_flush(cx)
    }
}

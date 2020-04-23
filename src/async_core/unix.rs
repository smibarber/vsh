// Copyright 2020 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::convert::TryFrom;
use std::fmt::{self, Display};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::net;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::io::{AsyncRead, AsyncWrite, Error as IoError, ErrorKind as IoErrorKind};

use cros_async::fd_executor::{add_read_waker, add_write_waker};

/// Errors generated while polling for signals.
#[derive(Debug)]
pub enum Error {
    /// An error occurred while setting the unix stream as nonblocking.
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
                "An error occurred while setting the unix stream as nonblocking: {}.",
                e
            ),
        }
    }
}

pub struct UnixStream {
    inner: net::UnixStream,
}

impl TryFrom<net::UnixStream> for UnixStream {
    type Error = crate::async_core::unix::Error;

    fn try_from(unix_stream: net::UnixStream) -> Result<UnixStream> {
        unix_stream.set_nonblocking(true).map_err(Error::SetNonblocking)?;
        Ok(UnixStream {
            inner: unix_stream,
        })
    }
}

impl AsyncRead for UnixStream {
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

impl AsyncWrite for UnixStream {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryInto;
    use cros_async::complete2;
    use futures::pin_mut;
    use futures::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn async_rw() {
        let (s1, s2) = net::UnixStream::pair().unwrap();
        let stream1: UnixStream = s1.try_into().unwrap();
        let stream2: UnixStream = s2.try_into().unwrap();

        async fn read_buf(mut stream: UnixStream) -> String {
            let mut buf = vec![0u8; 3];

            let res = stream.read_exact(&mut buf).await;

            match res {
                Ok(_) => String::from_utf8(buf).unwrap(),
                Err(e) => e.to_string(),
            }
        }

        async fn write_buf(mut stream: UnixStream) -> std::result::Result<(), IoError> {
            let foo = "foo";

            stream.write_all(foo.as_bytes()).await
        }

        let r = read_buf(stream1);
        pin_mut!(r);

        let w = write_buf(stream2);
        pin_mut!(w);

        if let (s, Ok(_)) = complete2(r, w).unwrap()
        {
            assert_eq!(s, "foo");
        } else {
            panic!("wrong futures returned from complete2");
        }
    }
}

// Copyright 2020 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::ffi::CStr;
use std::fmt;
use std::result;

use libchromeos::syslog;
//use libchromeos::vsock::{VsockListener, VMADDR_PORT_ANY};
//use log::{error, warn};
//use protobuf::{self, Message as ProtoMessage, ProtobufError};
use sys_util::{self, block_signal};

// Program name.
const IDENT: &[u8] = b"vshd\0";

#[remain::sorted]
#[derive(Debug)]
enum Error {
    BlockSigpipe(sys_util::signal::Error),
    Syslog(log::SetLoggerError),
}

type Result<T> = result::Result<T, Error>;

impl fmt::Display for Error {
    #[remain::check]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Error::*;

        #[remain::sorted]
        match self {
            BlockSigpipe(e) => write!(f, "failed to block SIGPIPE: {}", e),
            Syslog(e) => write!(f, "failed to initialize syslog: {}", e),
        }
    }
}

fn main() -> Result<()> {
    // Safe because this string is defined above in this file and it contains exactly
    // one nul byte, which appears at the end.
    let ident = CStr::from_bytes_with_nul(IDENT).unwrap();
    syslog::init(ident).map_err(Error::Syslog)?;

    // Block SIGPIPE so the process doesn't exit when writing to a socket that's been shutdown.
    block_signal(libc::SIGPIPE).map_err(Error::BlockSigpipe)?;
    Ok(())
}

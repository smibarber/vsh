// Copyright 2020 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::env;
use std::ffi::CStr;
use std::fmt;
use std::process;
use std::result;

use getopts::Options;
use libchromeos::syslog;
use log::warn;
use sys_util::{self, block_signal};

// Program name.
const IDENT: &[u8] = b"vsh\0";

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

fn print_usage(program: &str, opts: &Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.reqopt("l", "local", "local socket to forward", "SOCKADDR");
    opts.reqopt("r", "remote", "remote socket to forward to", "SOCKADDR");
    opts.optopt("t", "type", "type of traffic to forward", "stream|datagram");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => {
            warn!("failed to parse arg: {}", e);
            print_usage(&program, &opts);
            process::exit(1);
        }
    };
    if matches.opt_present("h") {
        print_usage(&program, &opts);
        return Ok(());
    }

    // Safe because this string is defined above in this file and it contains exactly
    // one nul byte, which appears at the end.
    let ident = CStr::from_bytes_with_nul(IDENT).unwrap();
    syslog::init(ident).map_err(Error::Syslog)?;

    // Block SIGPIPE so the process doesn't exit when writing to a socket that's been shutdown.
    block_signal(libc::SIGPIPE).map_err(Error::BlockSigpipe)?;

    Ok(())
}

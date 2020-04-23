// Copyright 2020 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fmt;
use std::io::{self, Read, Write};
use std::result;

/// Errors that can be encountered by a ForwarderSession.
#[remain::sorted]
#[derive(Debug)]
pub enum ForwarderError {
    /// An io::Error was encountered while reading from a stream.
    ReadFromStream(io::Error),
    /// An io::Error was encountered while shutting down writes on a stream.
    ShutDownStream(io::Error),
    /// An io::Error was encountered while writing to a stream.
    WriteToStream(io::Error),
}

type Result<T> = result::Result<T, ForwarderError>;

impl fmt::Display for ForwarderError {
    #[remain::check]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::ForwarderError::*;

        #[remain::sorted]
        match self {
            ReadFromStream(e) => write!(f, "failed to read from stream: {}", e),
            ShutDownStream(e) => write!(f, "failed to shut down stream: {}", e),
            WriteToStream(e) => write!(f, "failed to write to stream: {}", e),
        }
    }
}

pub struct Command {
    args: Vector<CString>,
    env: BTreeMap<CString, Option<CString>>,
    arg0: Option<CString>,
}

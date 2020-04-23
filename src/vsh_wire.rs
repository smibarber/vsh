// Copyright 2020 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Implements the vsh wire protocol on top of a stream-based socket.

use std::convert::TryFrom;
use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::result;

use futures::prelude::*;

use protobuf::{ProtobufError, Message};

const VSH_BUF_SIZE: usize = 4096;

#[remain::sorted]
#[derive(Debug)]
pub enum VshWireError {
    DeserializeProto(ProtobufError),
    MessageTooBig(usize),
    ReceiveMessage(io::Error),
    SendMessage(io::Error),
    SerializeProto(ProtobufError),
}

type Result<T> = result::Result<T, VshWireError>;

impl fmt::Display for VshWireError {
    #[remain::check]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::VshWireError::*;

        #[remain::sorted]
        match self {
            DeserializeProto(e) => write!(f, "failed to deserialize protobuf: {}", e),
            MessageTooBig(s) => write!(f, "protobuf is too big to send: {}", s),
            ReceiveMessage(e) => write!(f, "failed to receive message: {}", e),
            SendMessage(e) => write!(f, "failed to send message: {}", e),
            SerializeProto(e) => write!(f, "failed to serialize protobuf: {}", e),
        }
    }
}

pub struct VshWire<T: Read + Write + AsRawFd> {
    sock: T,
    rx_buf: Vec<u8>,
    tx_buf: Vec<u8>,
}

impl<T: Read + Write + AsRawFd> VshWire<T> {
    pub fn new(sock: T) -> Self {
        VshWire {
            sock,
            rx_buf: vec![0u8; VSH_BUF_SIZE],
            tx_buf: Vec::with_capacity(VSH_BUF_SIZE),
        }
    }

    /// Receives a full frame from the socket.
    fn receive_frame(&mut self) -> io::Result<usize> {
        let mut frame_len_bytes = [0u8; 4];
        self.sock.read_exact(&mut frame_len_bytes[..])?;

        // This will always succeed on 32 or 64 bit architectures.
        let frame_len = usize::try_from(u32::from_le_bytes(frame_len_bytes)).unwrap();

        if frame_len > VSH_BUF_SIZE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid vsh frame size"));
        }

        self.sock.read_exact(&mut self.rx_buf[..frame_len])?;

        Ok(frame_len)
    }

    fn send_frame(&mut self, frame_len: u32) -> io::Result<()> {
        let frame_len_bytes = frame_len.to_le_bytes();
        self.sock.write_all(&frame_len_bytes[..])?;

        // This will always succeed on 32 or 64 bit architectures.
        let frame_len = usize::try_from(u32::from_le_bytes(frame_len_bytes)).unwrap();

        self.sock.write_all(&self.tx_buf[..frame_len])?;

        Ok(())
    }

    pub fn receive_message<M: Message>(&mut self, msg: &mut M) -> Result<()> {
        let frame_len = self.receive_frame().map_err(VshWireError::ReceiveMessage)?;

        msg.merge_from_bytes(&self.rx_buf[..frame_len]).map_err(VshWireError::DeserializeProto)
    }

    pub fn send_message<M: Message>(&mut self, msg: &M) -> Result<()> {
        self.tx_buf.truncate(0);
        msg.write_to_vec(&mut self.tx_buf).map_err(VshWireError::SerializeProto)?;

        let frame_len = self.tx_buf.len();
        if frame_len > VSH_BUF_SIZE {
            return Err(VshWireError::MessageTooBig(frame_len));
        }

        // Cast is safe since we've verified frame_len is <= VSH_FRAME_SIZE < u32::max.
        self.send_frame(frame_len as u32).map_err(VshWireError::SendMessage)
    }
}

impl<T: Read + Write + AsRawFd> AsRawFd for VshWire<T> {
    fn as_raw_fd(&self) -> RawFd {
        self.sock.as_raw_fd()
    }
}

pub struct VshAsyncRead<T: AsyncRead + Unpin> {
    sock: T,
    rx_buf: Vec<u8>,
}

impl<T: AsyncRead + Unpin> VshAsyncRead<T> {
    pub fn new(sock: T) -> Self {
        VshAsyncRead {
            sock,
            rx_buf: vec![0u8; VSH_BUF_SIZE],
        }
    }

    /// Receives a full frame from the socket.
    async fn receive_frame(&mut self) -> io::Result<usize> {
        let mut frame_len_bytes = [0u8; 4];
        self.sock.read_exact(&mut frame_len_bytes[..]).await?;

        // This will always succeed on 32 or 64 bit architectures.
        let frame_len = usize::try_from(u32::from_le_bytes(frame_len_bytes)).unwrap();

        if frame_len > VSH_BUF_SIZE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid vsh frame size"));
        }

        self.sock.read_exact(&mut self.rx_buf[..frame_len]).await?;

        Ok(frame_len)
    }

    pub async fn receive_message<M: Message>(&mut self, msg: &mut M) -> Result<()> {
        let frame_len = self.receive_frame().await.map_err(VshWireError::ReceiveMessage)?;

        msg.merge_from_bytes(&self.rx_buf[..frame_len]).map_err(VshWireError::DeserializeProto)
    }
}

pub struct VshAsyncWrite<T: AsyncWrite + Unpin> {
    sock: T,
    tx_buf: Vec<u8>,
}

impl<T: AsyncWrite + Unpin> VshAsyncWrite<T> {
    pub fn new(sock: T) -> Self {
        VshAsyncWrite {
            sock,
            tx_buf: vec![0u8; VSH_BUF_SIZE],
        }
    }

    async fn send_frame(&mut self, frame_len: u32) -> io::Result<()> {
        let frame_len_bytes = frame_len.to_le_bytes();
        self.sock.write_all(&frame_len_bytes[..]).await?;

        // This will always succeed on 32 or 64 bit architectures.
        let frame_len = usize::try_from(u32::from_le_bytes(frame_len_bytes)).unwrap();

        self.sock.write_all(&self.tx_buf[..frame_len]).await?;

        Ok(())
    }

    pub async fn send_message<M: Message>(&mut self, msg: &M) -> Result<()> {
        self.tx_buf.truncate(0);
        msg.write_to_vec(&mut self.tx_buf).map_err(VshWireError::SerializeProto)?;

        let frame_len = self.tx_buf.len();
        if frame_len > VSH_BUF_SIZE {
            return Err(VshWireError::MessageTooBig(frame_len));
        }

        // Cast is safe since we've verified frame_len is <= VSH_FRAME_SIZE < u32::max.
        self.send_frame(frame_len as u32).await.map_err(VshWireError::SendMessage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    use vsh_proto::vsh::*;

    #[test]
    fn send_recv_valid() {
        let (host_sock, guest_sock) = UnixStream::pair().unwrap();

        let mut host = VshWire::new(host_sock);
        let mut guest = VshWire::new(guest_sock);

        // Send a GuestMessage carrying a status update.
        let mut guest_msg1 = GuestMessage::new();
        let status_msg1 = guest_msg1.mut_status_message();
        status_msg1.set_status(ConnectionStatus::READY);
        let description = "vsh ready";
        status_msg1.set_description(description.to_string());
        let code = 123;
        status_msg1.set_code(code);
        host.send_message(&guest_msg1).unwrap();


        // Receive the GuestMessage and ensure the fields match what was sent.
        let mut guest_msg2 = GuestMessage::new();
        guest.receive_message(&mut guest_msg2).unwrap();
        assert!(guest_msg2.has_status_message());
        let status_msg2 = guest_msg2.get_status_message();
        assert_eq!(status_msg2.get_status(), ConnectionStatus::READY);
        assert_eq!(status_msg2.get_description(), description);
        assert_eq!(status_msg2.get_code(), code);
    }

    #[test]
    fn recv_invalid_frame_size() {
        let (mut host_sock, guest_sock) = UnixStream::pair().unwrap();

        let mut guest = VshWire::new(guest_sock);

        // Write a garbage frame size into one side of the socket, receiving should fail.
        host_sock.write_all(&std::u32::MAX.to_le_bytes()).unwrap();

        let mut guest_msg = GuestMessage::new();
        guest.receive_message(&mut guest_msg).expect_err("allowed invalid size");
    }

    #[test]
    fn send_invalid_size() {
        let (mut host_sock, guest_sock) = UnixStream::pair().unwrap();

        let mut guest = VshWire::new(guest_sock);

        // Write a garbage frame size into one side of the socket, receiving should fail.
        host_sock.write_all(&std::u32::MAX.to_le_bytes()).unwrap();

        let mut guest_msg = GuestMessage::new();
        guest.receive_message(&mut guest_msg).expect_err("allowed invalid size");
    }
}

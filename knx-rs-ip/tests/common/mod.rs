// SPDX-License-Identifier: GPL-3.0-only

//! Shared helpers for the loopback tests: a scripted raw KNXnet/IP tunnel
//! client that speaks the wire protocol directly, so a test controls exactly
//! when (and whether) it acknowledges a frame.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

pub mod knxd;

use std::net::SocketAddr;
use std::time::Duration;

use knx_rs_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::knxip::{
    ConnectionHeader, E_NO_ERROR, HostProtocol, Hpai, KnxIpFrame, ServiceType, tunnel_cri,
};
use knx_rs_core::message::MessageCode;
use knx_rs_core::types::Priority;
use tokio::net::UdpSocket;
use tokio::time::{Instant, timeout};

/// Upper bound for operations that must complete promptly on loopback.
pub const WAIT: Duration = Duration::from_secs(5);

/// A group write `1/1/1 = value` from 1.1.2, with the given message code.
pub fn group_write(code: MessageCode, value: u8) -> CemiFrame {
    group_write_from(0x1102, code, value)
}

/// A group write `1/1/1 = value` from the individual address `source` (raw).
pub fn group_write_from(source: u16, code: MessageCode, value: u8) -> CemiFrame {
    CemiFrame::new_l_data(
        code,
        IndividualAddress::from_raw(source),
        DestinationAddress::Group(GroupAddress::from_raw(0x0901)),
        Priority::Low,
        &[0x00, 0x80 | value],
    )
}

/// A scripted tunnel client on a plain UDP socket.
pub struct RawClient {
    pub sock: UdpSocket,
    pub server: SocketAddr,
    pub channel_id: u8,
    pub port: u16,
}

impl RawClient {
    /// Connect to `server` and return once the connect response arrived.
    pub async fn connect(server: SocketAddr) -> Self {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = sock.local_addr().unwrap().port();
        let hpai = Hpai {
            protocol: HostProtocol::Ipv4Udp,
            ip: [127, 0, 0, 1],
            port,
        };
        let request = KnxIpFrame::connect_request(hpai, hpai, &tunnel_cri());
        sock.send_to(&request.to_bytes(), server).await.unwrap();
        let mut client = Self {
            sock,
            server,
            channel_id: 0,
            port,
        };
        let response = client
            .recv_frame(WAIT)
            .await
            .expect("connect response expected");
        assert_eq!(response.service_type, ServiceType::ConnectResponse);
        assert_eq!(response.body[1], E_NO_ERROR, "connect must succeed");
        client.channel_id = response.body[0];
        client
    }

    /// Receive the next datagram within `wait`, parsed; `None` on timeout.
    pub async fn recv_frame(&self, wait: Duration) -> Option<KnxIpFrame> {
        let mut buf = [0u8; 1024];
        let (n, _) = timeout(wait, self.sock.recv_from(&mut buf))
            .await
            .ok()?
            .ok()?;
        Some(KnxIpFrame::parse(&buf[..n]).expect("server sent a malformed frame"))
    }

    /// Receive the next frame of `service` within `wait`, skipping others.
    pub async fn recv_service(&self, service: ServiceType, wait: Duration) -> Option<KnxIpFrame> {
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let frame = self.recv_frame(left).await?;
            if frame.service_type == service {
                return Some(frame);
            }
        }
    }

    /// Send a heartbeat and return the status of the response, skipping any
    /// tunnelling traffic that arrives in between.
    pub async fn heartbeat(&self) -> u8 {
        self.heartbeat_within(WAIT)
            .await
            .expect("heartbeat response")
    }

    /// Like [`Self::heartbeat`], with a deadline; `None` if no response came.
    pub async fn heartbeat_within(&self, wait: Duration) -> Option<u8> {
        let hpai = Hpai {
            protocol: HostProtocol::Ipv4Udp,
            ip: [127, 0, 0, 1],
            port: self.port,
        };
        let request = KnxIpFrame::connection_state_request(self.channel_id, hpai);
        self.sock
            .send_to(&request.to_bytes(), self.server)
            .await
            .unwrap();
        let response = self
            .recv_service(ServiceType::ConnectionStateResponse, wait)
            .await?;
        Some(response.body[1])
    }

    /// Acknowledge a server frame with sequence `seq`.
    pub async fn ack(&self, seq: u8) {
        let ack = KnxIpFrame::tunneling_ack(self.channel_id, seq, E_NO_ERROR);
        self.sock
            .send_to(&ack.to_bytes(), self.server)
            .await
            .unwrap();
    }

    /// Send a tunnelling request with sequence `seq`.
    pub async fn send_request(&self, seq: u8, cemi: &CemiFrame) {
        let request = KnxIpFrame::tunneling_request(self.channel_id, seq, cemi.as_bytes());
        self.sock
            .send_to(&request.to_bytes(), self.server)
            .await
            .unwrap();
    }

    /// Wait for the next tunnelling request from the server; returns its
    /// sequence counter and the cEMI bytes.
    pub async fn recv_request(&self, wait: Duration) -> Option<(u8, Vec<u8>)> {
        let frame = self
            .recv_service(ServiceType::TunnelingRequest, wait)
            .await?;
        let header = ConnectionHeader::parse(&frame.body).unwrap();
        Some((
            header.sequence_counter,
            frame.body[usize::from(ConnectionHeader::LEN)..].to_vec(),
        ))
    }

    /// Wait for a tunnelling ack with `seq`, skipping other frames.
    pub async fn recv_ack(&self, seq: u8, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(frame) = self.recv_service(ServiceType::TunnelingAck, left).await else {
                return false;
            };
            if ConnectionHeader::parse(&frame.body)
                .unwrap()
                .sequence_counter
                == seq
            {
                return true;
            }
        }
    }

    /// Acknowledge every tunnelling request from now on, in the background.
    /// The task ends when the client's socket is dropped with the handle.
    pub fn spawn_acker(self: std::sync::Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            while let Ok((n, _)) = self.sock.recv_from(&mut buf).await {
                if let Ok(frame) = KnxIpFrame::parse(&buf[..n])
                    && frame.service_type == ServiceType::TunnelingRequest
                    && let Some(header) = ConnectionHeader::parse(&frame.body)
                {
                    self.ack(header.sequence_counter).await;
                }
            }
        })
    }
}

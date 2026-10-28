#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! The tunnel client acknowledges a server frame at once, also while it waits
//! for the acknowledgement of a frame of its own.

mod common;

use std::time::Duration;

use common::{WAIT, group_write};
use knx_rs_core::knxip::{ConnectionHeader, E_NO_ERROR, Hpai, KnxIpFrame, ServiceType, tunnel_crd};
use knx_rs_core::message::MessageCode;
use knx_rs_ip::{KnxConnection, TunnelConnection};
use tokio::net::UdpSocket;
use tokio::time::{Instant, timeout};

/// A strict stop-and-wait server: while it waits for the acknowledgement of its
/// own frame it does not acknowledge anything the client sent.
async fn strict_server(sock: UdpSocket) -> bool {
    let mut buf = [0u8; 1024];

    // Connect handshake.
    let (_, client) = sock.recv_from(&mut buf).await.unwrap();
    let mut body = vec![9, E_NO_ERROR];
    body.extend_from_slice(&Hpai::nat_udp(sock.local_addr().unwrap().port()).to_bytes());
    body.extend_from_slice(&tunnel_crd(0xFF01));
    let response = KnxIpFrame {
        service_type: ServiceType::ConnectResponse,
        body,
    };
    sock.send_to(&response.to_bytes(), client).await.unwrap();

    // Wait for the client's request, but hold its acknowledgement back.
    let client_seq = loop {
        let (n, _) = sock.recv_from(&mut buf).await.unwrap();
        let frame = KnxIpFrame::parse(&buf[..n]).unwrap();
        if frame.service_type == ServiceType::TunnelingRequest {
            break ConnectionHeader::parse(&frame.body)
                .unwrap()
                .sequence_counter;
        }
    };

    // Send our own frame and wait for its ack, up to three tries.
    let cemi = group_write(MessageCode::LDataInd, 1);
    let request = KnxIpFrame::tunneling_request(9, 0, cemi.as_bytes());
    let mut acked = false;
    'tries: for _ in 0..3 {
        sock.send_to(&request.to_bytes(), client).await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while let Ok(Ok((n, _))) = timeout(
            deadline.saturating_duration_since(Instant::now()),
            sock.recv_from(&mut buf),
        )
        .await
        {
            let frame = KnxIpFrame::parse(&buf[..n]).unwrap();
            if frame.service_type == ServiceType::TunnelingAck {
                acked = true;
                break 'tries;
            }
        }
    }

    // Only now acknowledge the client's frame.
    let ack = KnxIpFrame::tunneling_ack(9, client_seq, E_NO_ERROR);
    sock.send_to(&ack.to_bytes(), client).await.unwrap();
    acked
}

#[tokio::test]
async fn client_acknowledges_a_server_frame_while_waiting_for_its_own_ack() {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = sock.local_addr().unwrap();
    let server = tokio::spawn(strict_server(sock));

    let conn = TunnelConnection::connect(addr).await.unwrap();
    let started = Instant::now();
    let result = timeout(WAIT, conn.send(group_write(MessageCode::LDataReq, 0)))
        .await
        .expect("send timed out");
    assert!(
        result.is_ok(),
        "the write failed after {:?}: {result:?}",
        started.elapsed()
    );
    assert!(server.await.unwrap(), "server frame was never acknowledged");
}

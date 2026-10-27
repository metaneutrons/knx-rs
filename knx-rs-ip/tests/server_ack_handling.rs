#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! The device server's handling of tunnel clients that do not acknowledge.

mod common;

use std::time::Duration;

use common::{RawClient, WAIT, group_write};
use knx_rs_core::knxip::{E_CONNECTION_ID, E_NO_ERROR};
use knx_rs_core::message::MessageCode;
use knx_rs_ip::DeviceServer;
use tokio::time::Instant;

async fn start() -> DeviceServer {
    DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn unacknowledged_tunnel_client_is_dropped_after_the_retries() {
    let server = start().await;
    let silent = RawClient::connect(server.local_addr()).await;
    server
        .send_frame(group_write(MessageCode::LDataInd, 1))
        .await
        .unwrap();

    // The frame is sent three times, one second apart, without an ack.
    let started = Instant::now();
    let mut transmissions = 0;
    while let Some((seq, _)) = silent
        .recv_request(Duration::from_millis(3500).saturating_sub(started.elapsed()))
        .await
    {
        assert_eq!(seq, 0, "retransmissions repeat the sequence counter");
        transmissions += 1;
    }
    assert_eq!(transmissions, 3, "frame transmissions");

    // The connection is lost: the server must not keep the channel.
    assert_eq!(
        silent.heartbeat_within(Duration::from_secs(2)).await,
        Some(E_CONNECTION_ID),
        "the tunnel of a client that never acknowledged must be gone"
    );
}

#[tokio::test]
async fn silent_client_costs_the_other_clients_at_most_one_frame() {
    let server = start().await;
    let silent = RawClient::connect(server.local_addr()).await;
    let live = RawClient::connect(server.local_addr()).await;

    let mut delays = Vec::new();
    for round in 0u8..3 {
        let sent = Instant::now();
        server
            .send_frame(group_write(MessageCode::LDataInd, round))
            .await
            .unwrap();
        let (seq, _) = live.recv_request(WAIT).await.expect("live client frame");
        live.ack(seq).await;
        delays.push(sent.elapsed());
    }
    drop(silent);
    // The first frame may wait for the silent client's retries once.
    for (round, delay) in delays.iter().enumerate().skip(1) {
        assert!(
            *delay < Duration::from_millis(500),
            "frame {round} reached the live client after {delay:?}: \
             a client that never acknowledged still holds up the server"
        );
    }
}

#[tokio::test]
async fn server_keeps_serving_other_clients_while_waiting_for_a_silent_one() {
    let server = start().await;
    let silent = RawClient::connect(server.local_addr()).await;
    let live = RawClient::connect(server.local_addr()).await;

    server
        .send_frame(group_write(MessageCode::LDataInd, 1))
        .await
        .unwrap();
    // The server is now waiting for the silent client's acknowledgement.
    assert!(
        silent
            .recv_request(Duration::from_millis(500))
            .await
            .is_some()
    );

    let started = Instant::now();
    let status = live.heartbeat_within(Duration::from_secs(2)).await;
    assert_eq!(status, Some(E_NO_ERROR), "heartbeat of the live client");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the heartbeat took {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn server_acknowledges_a_request_while_it_waits_for_the_clients_ack() {
    let mut server = start().await;
    let client = RawClient::connect(server.local_addr()).await;

    server
        .send_frame(group_write(MessageCode::LDataInd, 1))
        .await
        .unwrap();
    // The server's frame is in flight and unacknowledged. The client sends its
    // own request first, as any client with a frame of its own to send may.
    let (seq, _) = client.recv_request(WAIT).await.expect("server frame");
    client
        .send_request(0, &group_write(MessageCode::LDataReq, 0))
        .await;

    assert!(
        client.recv_ack(0, Duration::from_millis(500)).await,
        "the server must acknowledge the request without waiting for our ack"
    );
    client.ack(seq).await;

    let event = tokio::time::timeout(WAIT, server.recv())
        .await
        .expect("event timeout");
    assert!(matches!(
        event,
        Some(knx_rs_ip::ServerEvent::TunnelFrame(_))
    ));
}

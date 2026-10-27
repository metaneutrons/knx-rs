#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! The device server's handling of tunnel clients that do not acknowledge.

mod common;

use std::time::Duration;

use common::{RawClient, WAIT, group_write};
use knx_rs_core::knxip::E_CONNECTION_ID;
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

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

/// Each tunnel client has its own queue and its own acknowledgement, so a client that
/// never answers holds up no frame of the others, not even the first (issue 106). The
/// silent client connects first, which is where it used to be served first.
#[tokio::test]
async fn a_silent_client_delays_no_frame_to_the_other_clients() {
    let server = start().await;
    let silent = RawClient::connect(server.local_addr()).await;
    let live = RawClient::connect(server.local_addr()).await;

    for round in 0u8..3 {
        let sent = Instant::now();
        server
            .send_frame(group_write(MessageCode::LDataInd, round))
            .await
            .unwrap();
        let (seq, _) = live.recv_request(WAIT).await.expect("live client frame");
        live.ack(seq).await;
        assert!(
            sent.elapsed() < Duration::from_millis(500),
            "frame {round} reached the live client after {:?}: \
             a client that never acknowledged still holds up the server",
            sent.elapsed()
        );
    }
    drop(silent);
}

/// A client that acknowledges, only slowly, gets every frame, in order, although the
/// application sent them all at once: the frames wait in its queue, one on the wire.
#[tokio::test]
async fn a_slow_client_receives_every_frame_in_order() {
    let server = start().await;
    let slow = RawClient::connect(server.local_addr()).await;

    for value in 0u8..6 {
        server
            .send_frame(group_write(MessageCode::LDataInd, value))
            .await
            .unwrap();
    }
    for expected in 0u8..6 {
        let (seq, cemi) = slow.recv_request(WAIT).await.expect("queued frame");
        assert_eq!(seq, expected, "sequence counters count up");
        assert_eq!(
            *cemi.last().unwrap(),
            0x80 | expected,
            "frames arrive in the order they were sent"
        );
        // Nothing else is on the wire while this one is unacknowledged.
        assert!(
            slow.recv_request(Duration::from_millis(100))
                .await
                .is_none(),
            "a second frame was sent before the first was acknowledged"
        );
        slow.ack(seq).await;
    }
}

/// While a silent client runs through its retries and is dropped, the live one keeps
/// receiving a steady stream, without a gap.
#[tokio::test]
async fn the_live_client_keeps_receiving_while_the_silent_one_is_dropped() {
    let server = start().await;
    let silent = RawClient::connect(server.local_addr()).await;
    let live = RawClient::connect(server.local_addr()).await;

    let started = Instant::now();
    let mut longest_gap = Duration::ZERO;
    let mut last = Instant::now();
    for value in 0u8..14 {
        server
            .send_frame(group_write(MessageCode::LDataInd, value))
            .await
            .unwrap();
        let (seq, cemi) = live.recv_request(WAIT).await.expect("live client frame");
        assert_eq!(seq, value, "sequence counters count up without a gap");
        assert_eq!(*cemi.last().unwrap(), 0x80 | value);
        live.ack(seq).await;
        longest_gap = longest_gap.max(last.elapsed());
        last = Instant::now();
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    assert!(
        started.elapsed() > Duration::from_secs(3),
        "long enough for the retries"
    );
    assert!(
        longest_gap < Duration::from_millis(800),
        "a frame to the live client waited {longest_gap:?}"
    );
    assert_eq!(
        silent.heartbeat_within(Duration::from_secs(2)).await,
        Some(E_CONNECTION_ID),
        "the silent client's tunnel is gone"
    );
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

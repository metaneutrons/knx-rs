#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! Stopping a device server and taking its socket over again.

mod common;

use std::net::SocketAddr;
use std::time::Duration;

use common::{RawClient, group_write};
use knx_rs_core::knxip::ServiceType;
use knx_rs_core::message::MessageCode;
use knx_rs_ip::DeviceServer;
use tokio::net::UdpSocket;
use tokio::time::{Instant, timeout};

async fn start() -> DeviceServer {
    DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap()
}

/// Bind `addr` again, retrying for `within`; returns how long it took.
async fn rebind(addr: SocketAddr, within: Duration) -> Result<Duration, std::io::Error> {
    let started = Instant::now();
    loop {
        match UdpSocket::bind(addr).await {
            Ok(_socket) => return Ok(started.elapsed()),
            Err(e) if started.elapsed() >= within => return Err(e),
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
}

#[tokio::test]
async fn shutdown_returns_with_the_address_free() {
    let server = start().await;
    let addr = server.local_addr();
    server.shutdown().await;

    // No retry and no sleep: shutdown() waited for the socket to go.
    UdpSocket::bind(addr)
        .await
        .expect("the address must be free when shutdown() returns");
}

#[tokio::test]
async fn stop_and_drop_free_the_address_promptly() {
    let server = start().await;
    let addr = server.local_addr();
    server.stop().await;
    drop(server);

    let took = rebind(addr, Duration::from_secs(1))
        .await
        .expect("the address stayed taken after stop() and drop");
    assert!(took < Duration::from_secs(1), "took {took:?}");
}

#[tokio::test]
async fn dropping_a_server_with_a_silent_client_frees_the_address_quickly() {
    let server = start().await;
    let addr = server.local_addr();
    let _silent = RawClient::connect(addr).await;
    for round in 0..3 {
        server
            .send_frame(group_write(MessageCode::LDataInd, round))
            .await
            .unwrap();
    }
    drop(server);

    let took = rebind(addr, Duration::from_secs(2))
        .await
        .expect("the address stayed taken by the abandoned server task");
    assert!(took < Duration::from_secs(2), "took {took:?}");
}

#[tokio::test]
async fn shutdown_does_not_wait_for_a_silent_client_or_the_queued_frames() {
    let server = start().await;
    let addr = server.local_addr();
    let _silent = RawClient::connect(addr).await;
    // One frame in flight, the command queue full behind it.
    for round in 0..17 {
        server
            .send_frame(group_write(MessageCode::LDataInd, round & 1))
            .await
            .unwrap();
    }
    let started = Instant::now();
    timeout(Duration::from_secs(1), server.shutdown())
        .await
        .expect("shutdown waited for the frames queued for a silent client");
    assert!(started.elapsed() < Duration::from_secs(1));
    UdpSocket::bind(addr).await.expect("address free");
}

#[tokio::test]
async fn shutdown_tells_the_tunnel_clients() {
    let server = start().await;
    let client = RawClient::connect(server.local_addr()).await;
    server.shutdown().await;

    let request = client
        .recv_service(ServiceType::DisconnectRequest, Duration::from_secs(2))
        .await
        .expect("a DISCONNECT_REQUEST");
    assert_eq!(request.body[0], client.channel_id);
}

#[tokio::test]
async fn a_new_server_takes_over_the_address_and_accepts_clients() {
    let server = start().await;
    let addr = server.local_addr();
    let _old = RawClient::connect(addr).await;
    server.shutdown().await;

    let mut second = DeviceServer::start_at(addr).await.unwrap();
    let client = RawClient::connect(addr).await;
    client
        .send_request(0, &group_write(MessageCode::LDataReq, 1))
        .await;
    let event = timeout(Duration::from_secs(2), second.recv())
        .await
        .expect("event timeout");
    assert!(event.is_some());
    second.shutdown().await;
}

#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! A flood of datagrams must not starve the server's other work.
//!
//! The server task has one socket and several other things to do (commands
//! from the application, events for the application). With a fixed order of
//! its `select!` arms, a socket that is always readable would keep it from
//! ever getting to the others.

mod common;

use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::{RawClient, WAIT, group_write};
use knx_rs_core::knxip::{Hpai, KnxIpFrame, ServiceType};
use knx_rs_core::message::MessageCode;
use knx_rs_ip::{DeviceServer, ServerEvent};

/// Sends heartbeats for a channel that does not exist, until told to stop. Each
/// one is a datagram the server reads and answers. `per_second` of 0 means as
/// fast as the thread can, which keeps the server's socket readable all the time.
fn flood(
    target: SocketAddr,
    per_second: u64,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<u64> {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut body = vec![99u8, 0];
    body.extend_from_slice(&Hpai::nat_udp(1).to_bytes());
    let datagram = KnxIpFrame {
        service_type: ServiceType::ConnectionStateRequest,
        body,
    }
    .to_bytes();
    std::thread::spawn(move || {
        let start = Instant::now();
        let mut sent = 0u64;
        while !stop.load(Ordering::Relaxed) {
            if per_second > 0
                && u128::from(sent) * 1_000_000
                    > start.elapsed().as_micros() * u128::from(per_second)
            {
                std::hint::spin_loop();
                continue;
            }
            if socket.send_to(&datagram, target).is_ok() {
                sent += 1;
            }
            // Leave the CPU to the other threads now and then: on a runner with few
            // cores the flood must not keep the test's own tasks from running.
            if sent % 256 == 0 {
                std::thread::yield_now();
            }
        }
        sent
    })
}

/// A frame the application queues reaches a connected client although the
/// server's socket never runs dry. The client acknowledges every copy it
/// sees: under a flood the server may lose an acknowledgement and repeat.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_frame_is_sent_while_the_socket_is_flooded() {
    let server = DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = server.local_addr();
    let client = RawClient::connect(addr).await;

    let stop = Arc::new(AtomicBool::new(false));
    let flooder = flood(addr, 0, Arc::clone(&stop));
    tokio::time::sleep(Duration::from_millis(300)).await;

    let started = Instant::now();
    server
        .send_frame(group_write(MessageCode::LDataReq, 1))
        .await
        .unwrap();
    let mut delivered = false;
    while started.elapsed() < WAIT {
        if let Some((seq, _)) = client.recv_request(Duration::from_millis(500)).await {
            client.ack(seq).await;
            delivered = true;
            break;
        }
    }
    let latency = started.elapsed();

    stop.store(true, Ordering::Relaxed);
    let datagrams = flooder.join().unwrap();
    assert!(
        datagrams > 10_000,
        "the flood did not run ({datagrams} datagrams)"
    );
    assert!(
        // Without the fix the frame needs more than 4.6 s (or never arrives); with it a
        // few milliseconds on a fast machine. The limit leaves room for a slow runner
        // and a lost acknowledgement (the server repeats after one second).
        delivered && latency < Duration::from_secs(4),
        "the frame was starved by the flood (delivered: {delivered}, after {latency:?})"
    );
}

/// A request of a client reaches the application while the server keeps
/// receiving a steady stream of other datagrams (well below what saturates the
/// socket, so none is lost).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn request_reaches_the_application_while_datagrams_arrive_steadily() {
    let mut server = DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = server.local_addr();
    let client = RawClient::connect(addr).await;

    let stop = Arc::new(AtomicBool::new(false));
    let flooder = flood(addr, 20_000, Arc::clone(&stop));
    tokio::time::sleep(Duration::from_millis(300)).await;

    let started = Instant::now();
    client
        .send_request(0, &group_write(MessageCode::LDataReq, 2))
        .await;
    let received = tokio::time::timeout(WAIT, server.recv()).await;
    let latency = started.elapsed();

    stop.store(true, Ordering::Relaxed);
    let sent = flooder.join().unwrap();
    assert!(sent > 1_000, "the stream did not run ({sent} datagrams)");
    assert!(
        matches!(received, Ok(Some(ServerEvent::TunnelFrame(_)))),
        "the request never reached the application ({latency:?})"
    );
}

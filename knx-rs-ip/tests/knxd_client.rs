#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! The knx-rs tunnel client against knxd, a third-party KNXnet/IP server.
//!
//! The tests need Docker (see `tests/knxd/README.md`) and skip loudly without.
//! Each test prints the wire trace of the clients it ran, shown on failure or
//! with `--nocapture`.

mod common;

use std::time::Duration;

use common::knxd::{Knxd, Mode, Relay};
use common::{RawClient, WAIT, group_write};
use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::knxip::{E_CONNECTION_ID, E_NO_ERROR, ServiceType};
use knx_rs_core::message::MessageCode;
use knx_rs_ip::{KnxConnection, TunnelConnection};
use tokio::time::timeout;

const GA_1_1_1: u16 = 0x0901;
const GA_2_2_2: u16 = 0x1202;

/// Receive frames until one matches, ignoring the rest (confirmations and
/// the like); panics after [`WAIT`].
async fn recv_matching(
    conn: &mut TunnelConnection,
    what: &str,
    pred: impl Fn(&CemiFrame) -> bool + Send + Sync,
) -> CemiFrame {
    let found = timeout(WAIT, async {
        loop {
            let frame = conn.recv().await.expect("connection closed");
            if pred(&frame) {
                return frame;
            }
        }
    })
    .await;
    found.unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

fn group_read(ga: u16) -> CemiFrame {
    CemiFrame::new_l_data(
        MessageCode::LDataReq,
        knx_rs_core::address::IndividualAddress::from_raw(0),
        knx_rs_core::address::DestinationAddress::Group(
            knx_rs_core::address::GroupAddress::from_raw(ga),
        ),
        knx_rs_core::types::Priority::Low,
        &[0x00, 0x00],
    )
}

fn group_response(ga: u16, value: u8) -> CemiFrame {
    CemiFrame::new_l_data(
        MessageCode::LDataReq,
        knx_rs_core::address::IndividualAddress::from_raw(0),
        knx_rs_core::address::DestinationAddress::Group(
            knx_rs_core::address::GroupAddress::from_raw(ga),
        ),
        knx_rs_core::types::Priority::Low,
        &[0x00, 0x40 | value],
    )
}

fn print_traces(traces: &[(&str, &Relay)]) {
    for (name, relay) in traces {
        eprintln!("--- wire trace {name}\n{}", relay.trace_text());
    }
}

#[tokio::test]
async fn group_write_travels_between_two_clients_with_the_right_message_codes() {
    let Some(knxd) = Knxd::start(Mode::Server).await else {
        return;
    };
    let (relay_a, relay_b) = (knxd.relay().await, knxd.relay().await);
    let mut a = TunnelConnection::connect(relay_a.addr).await.unwrap();
    let mut b = TunnelConnection::connect(relay_b.addr).await.unwrap();

    // The write leaves the client as L_Data.req.
    a.send(group_write(MessageCode::LDataReq, 1)).await.unwrap();

    // The other client receives it as L_Data.ind.
    let ind = recv_matching(&mut b, "the indication", |f| {
        f.destination_address_raw() == GA_1_1_1
    })
    .await;
    assert_eq!(ind.message_code_raw(), MessageCode::LDataInd as u8);
    assert_eq!(ind.payload(), [0x00, 0x81]);

    // The sender gets the local confirmation, L_Data.con.
    let con = recv_matching(&mut a, "the confirmation", |f| {
        f.destination_address_raw() == GA_1_1_1
    })
    .await;
    assert_eq!(con.message_code_raw(), MessageCode::LDataCon as u8);

    // Seen on the wire: req from A, ind to B, con to A, all acknowledged.
    let codes = |relay: &Relay, from_client: bool| -> Vec<u8> {
        relay
            .trace()
            .iter()
            .filter(|d| d.from_client == from_client)
            .filter_map(common::knxd::Datagram::cemi_code)
            .collect()
    };
    assert_eq!(codes(&relay_a, true), [0x11]);
    assert_eq!(codes(&relay_a, false), [0x2E]);
    assert_eq!(codes(&relay_b, false), [0x29]);

    // Disconnect: knxd answers the client's DISCONNECT_REQUEST.
    a.close().await;
    relay_a
        .wait_for("DISCONNECT_RESPONSE", WAIT, |d| {
            !d.from_client
                && d.frame()
                    .is_some_and(|f| f.service_type == ServiceType::DisconnectResponse)
        })
        .await;
    print_traces(&[("A", &relay_a), ("B", &relay_b)]);
}

#[tokio::test]
async fn group_read_and_response_travel_both_ways() {
    let Some(knxd) = Knxd::start(Mode::Server).await else {
        return;
    };
    let (relay_a, relay_b) = (knxd.relay().await, knxd.relay().await);
    let mut a = TunnelConnection::connect(relay_a.addr).await.unwrap();
    let mut b = TunnelConnection::connect(relay_b.addr).await.unwrap();

    // A asks, B sees the read and answers, A sees the response.
    a.send(group_read(GA_1_1_1)).await.unwrap();
    let read = recv_matching(&mut b, "the read", |f| {
        f.destination_address_raw() == GA_1_1_1
    })
    .await;
    assert_eq!(read.payload(), [0x00, 0x00]);
    b.send(group_response(GA_1_1_1, 1)).await.unwrap();
    let response = recv_matching(&mut a, "the response", |f| {
        f.destination_address_raw() == GA_1_1_1 && f.payload() == [0x00, 0x41]
    })
    .await;
    assert_eq!(response.message_code_raw(), MessageCode::LDataInd as u8);

    // And the other way round.
    b.send(group_read(GA_2_2_2)).await.unwrap();
    recv_matching(&mut a, "the second read", |f| {
        f.destination_address_raw() == GA_2_2_2 && f.payload() == [0x00, 0x00]
    })
    .await;
    a.send(group_response(GA_2_2_2, 0)).await.unwrap();
    recv_matching(&mut b, "the second response", |f| {
        f.destination_address_raw() == GA_2_2_2 && f.payload() == [0x00, 0x40]
    })
    .await;
    print_traces(&[("A", &relay_a), ("B", &relay_b)]);
}

#[tokio::test]
async fn telegram_injected_into_knxd_reaches_the_client_as_an_indication() {
    let Some(knxd) = Knxd::start(Mode::Server).await else {
        return;
    };
    let relay = knxd.relay().await;
    let mut client = TunnelConnection::connect(relay.addr).await.unwrap();

    knxd.knxtool_group_write("2/2/2", "1");
    let ind = recv_matching(&mut client, "the telegram from knxtool", |f| {
        f.destination_address_raw() == GA_2_2_2
    })
    .await;
    assert_eq!(ind.message_code_raw(), MessageCode::LDataInd as u8);
    assert_eq!(ind.payload(), [0x00, 0x81]);
    print_traces(&[("client", &relay)]);
}

#[tokio::test]
async fn knxd_repeats_once_and_then_drops_a_client_that_does_not_acknowledge() {
    let Some(knxd) = Knxd::start(Mode::Server).await else {
        return;
    };
    let (relay_a, relay_silent) = (knxd.relay().await, knxd.relay().await);
    let mut a = TunnelConnection::connect(relay_a.addr).await.unwrap();
    let silent = RawClient::connect(relay_silent.addr).await;

    // Both receive this telegram; the raw client never acknowledges it.
    a.send(group_write(MessageCode::LDataReq, 1)).await.unwrap();
    let (seq, _) = silent.recv_request(WAIT).await.expect("first transmission");
    assert_eq!(seq, 0);
    let (again, _) = silent
        .recv_request(Duration::from_millis(1500))
        .await
        .expect("knxd repeats after one second");
    assert_eq!(again, 0, "the repetition keeps the sequence counter");
    assert!(
        silent
            .recv_request(Duration::from_millis(2500))
            .await
            .is_none(),
        "knxd stops after the repetition"
    );

    // The connection is gone, without a DISCONNECT_REQUEST to the client.
    assert_eq!(silent.heartbeat_within(WAIT).await, Some(E_CONNECTION_ID));
    let logs = knxd.logs();
    assert!(logs.contains("dropped no-ACK"), "knxd log:\n{logs}");
    let disconnects = relay_silent
        .trace()
        .iter()
        .filter(|d| {
            !d.from_client
                && d.frame()
                    .is_some_and(|f| f.service_type == ServiceType::DisconnectRequest)
        })
        .count();
    assert_eq!(disconnects, 0);

    // The well-behaved client is unaffected.
    assert_eq!(a.recv().await.map(|f| f.message_code_raw()), Some(0x2E));
    print_traces(&[("silent", &relay_silent)]);
}

#[tokio::test]
async fn client_write_while_knxd_delivers_a_frame_does_not_stall() {
    let Some(knxd) = Knxd::start(Mode::Server).await else {
        return;
    };
    let relay = knxd.relay().await;
    let client = TunnelConnection::connect(relay.addr).await.unwrap();

    // Writes by the client cross telegrams injected into knxd. knxd answers a
    // request while it waits for an acknowledgement of its own, so no write
    // may time out (a stop-and-wait peer is a different matter, see
    // `tunnel_client_ack.rs`).
    let started = tokio::time::Instant::now();
    for round in 0u8..10 {
        let inject = tokio::task::spawn_blocking({
            let name = knxd.name.clone();
            move || {
                let out = std::process::Command::new("docker")
                    .args([
                        "exec",
                        &name,
                        "knxtool",
                        "groupswrite",
                        "local:/run/knxd/knxd.sock",
                        "2/2/2",
                        "1",
                    ])
                    .output()
                    .unwrap();
                assert!(out.status.success());
            }
        });
        client
            .send(group_write(MessageCode::LDataReq, round & 1))
            .await
            .unwrap_or_else(|e| panic!("write {round} failed: {e}"));
        inject.await.unwrap();
    }
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "took {:?}",
        started.elapsed()
    );
    print_traces(&[("client", &relay)]);
}

/// Heartbeat responses seen so far (status 0).
fn heartbeat_responses(relay: &Relay) -> usize {
    relay
        .trace()
        .iter()
        .filter(|d| {
            !d.from_client
                && d.frame().is_some_and(|f| {
                    f.service_type == ServiceType::ConnectionStateResponse
                        && f.body[1] == E_NO_ERROR
                })
        })
        .count()
}

#[tokio::test]
async fn heartbeat_is_answered_by_knxd() {
    let Some(knxd) = Knxd::start(Mode::Server).await else {
        return;
    };
    let relay = knxd.relay().await;
    let _client = TunnelConnection::connect(relay.addr).await.unwrap();

    // The client sends its first heartbeat right after connecting.
    relay
        .wait_for("heartbeat response", WAIT, |d| {
            !d.from_client
                && d.frame()
                    .is_some_and(|f| f.service_type == ServiceType::ConnectionStateResponse)
        })
        .await;
    assert_eq!(heartbeat_responses(&relay), 1);
    print_traces(&[("client", &relay)]);
}

/// The client's heartbeat interval is fixed at the 60 s the specification
/// gives, so this test waits for the second one.
#[tokio::test]
#[ignore = "waits for the 60 s heartbeat interval; run with --ignored"]
async fn periodic_heartbeat_is_answered_by_knxd() {
    let Some(knxd) = Knxd::start(Mode::Server).await else {
        return;
    };
    let relay = knxd.relay().await;
    let client = TunnelConnection::connect(relay.addr).await.unwrap();

    let started = tokio::time::Instant::now();
    while heartbeat_responses(&relay) < 2 {
        assert!(
            started.elapsed() < Duration::from_secs(75),
            "no second heartbeat answered; wire trace:\n{}",
            relay.trace_text()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    // The channel is still alive afterwards.
    client
        .send(group_write(MessageCode::LDataReq, 1))
        .await
        .unwrap();
    print_traces(&[("client", &relay)]);
}

#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! The knx-rs `DeviceServer` as the tunnelling server knxd connects to
//! (knxd's `ipt` driver), with a test client attached to knxd to see what
//! knxd forwards.
//!
//! The tests need Docker (see `tests/knxd/README.md`) and skip loudly without.

mod common;

use std::net::SocketAddr;
use std::time::Duration;

use common::knxd::{Knxd, Mode, Relay};
use common::{WAIT, group_write_from};
use knx_rs_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::message::MessageCode;
use knx_rs_core::types::Priority;
use knx_rs_ip::{DeviceServer, KnxConnection, ServerEvent, TunnelConnection};
use tokio::time::{Instant, timeout};

/// Group address 15/7/200, used to find out when knxd's link is up.
const PROBE_GA: u16 = 0x7FC8;
const GA_1_1_1: u16 = 0x0901;
/// The individual address of the device the server stands for (1.1.1).
const DEVICE: u16 = 0x1101;

/// Receive frames until one matches; panics after `wait`.
async fn recv_matching(
    conn: &mut TunnelConnection,
    what: &str,
    wait: Duration,
    pred: impl Fn(&CemiFrame) -> bool + Send + Sync,
) -> CemiFrame {
    timeout(wait, async {
        loop {
            let frame = conn.recv().await.expect("connection closed");
            if pred(&frame) {
                return frame;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

/// A device server, knxd linked to it, and a client attached to knxd.
struct Rig {
    server: DeviceServer,
    knxd: Knxd,
    relay: Relay,
    observer: TunnelConnection,
}

impl Rig {
    /// `None` only when Docker is unavailable.
    async fn start(heartbeat_secs: u32) -> Option<Self> {
        let server = DeviceServer::start_at("0.0.0.0:0".parse().unwrap())
            .await
            .unwrap();
        let host_port = server.local_addr().port();
        let knxd = Knxd::start(Mode::Ipt {
            host_port,
            heartbeat_secs,
        })
        .await?;
        let relay = knxd.relay().await;
        let mut observer = TunnelConnection::connect(relay.addr).await.unwrap();

        // The server has no list of its tunnels, so send probes until one
        // arrives at the observer: knxd's link is then up.
        let started = Instant::now();
        loop {
            server.send_frame(probe()).await.unwrap();
            let seen = timeout(Duration::from_millis(300), async {
                loop {
                    let frame = observer.recv().await?;
                    if frame.destination_address_raw() == PROBE_GA {
                        return Some(frame);
                    }
                }
            })
            .await;
            if matches!(seen, Ok(Some(_))) {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "knxd never linked to the device server; knxd log:\n{}",
                knxd.logs()
            );
        }
        Some(Self {
            server,
            knxd,
            relay,
            observer,
        })
    }
}

/// A group write to [`PROBE_GA`] from the device.
fn probe() -> CemiFrame {
    CemiFrame::new_l_data(
        MessageCode::LDataInd,
        IndividualAddress::from_raw(DEVICE),
        DestinationAddress::Group(GroupAddress::from_raw(PROBE_GA)),
        Priority::Low,
        &[0x00, 0x81],
    )
}

#[tokio::test]
async fn knxd_forwards_an_indication_from_the_device_server() {
    let Some(mut rig) = Rig::start(30).await else {
        return;
    };
    rig.knxd.capture_start().await;

    rig.server
        .send_frame(group_write_from(DEVICE, MessageCode::LDataInd, 1))
        .await
        .unwrap();
    let frame = recv_matching(&mut rig.observer, "the indication", WAIT, |f| {
        f.destination_address_raw() == GA_1_1_1
    })
    .await;
    assert_eq!(frame.message_code_raw(), MessageCode::LDataInd as u8);
    assert_eq!(frame.payload(), [0x00, 0x81]);

    eprintln!("--- knxd tcpdump\n{}", rig.knxd.capture_stop().await);
    eprintln!("--- observer wire trace\n{}", rig.relay.trace_text());
}

#[tokio::test]
async fn frames_from_knxd_reach_the_device_server() {
    let Some(rig) = Rig::start(30).await else {
        return;
    };
    let Rig {
        mut server,
        knxd,
        relay,
        observer,
    } = rig;

    // knxd sends what its other clients write down its ipt link, one frame at
    // a time, each acknowledged by the server.
    for round in 0u8..3 {
        observer
            .send(group_write_from(0, MessageCode::LDataReq, round & 1))
            .await
            .unwrap();
        let event = timeout(WAIT, server.recv()).await.unwrap_or_else(|_| {
            panic!(
                "no event; wire trace:\n{}\nknxd log:\n{}",
                relay.trace_text(),
                knxd.logs()
            )
        });
        let Some(ServerEvent::TunnelFrame(frame)) = event else {
            panic!("expected a tunnel frame, got {event:?}");
        };
        assert_eq!(frame.destination_address_raw(), GA_1_1_1);
        assert_eq!(
            frame.message_code_raw(),
            MessageCode::LDataReq as u8,
            "knxd's ipt driver sends L_Data.req to a server"
        );
    }
    let logs = knxd.logs();
    assert!(!logs.contains("Too many retransmits"), "knxd log:\n{logs}");
    eprintln!("--- observer wire trace\n{}", relay.trace_text());
}

#[tokio::test]
async fn knxd_gives_up_on_a_server_that_never_acknowledges() {
    use knx_rs_core::knxip::{E_NO_ERROR, Hpai, KnxIpFrame, ServiceType, tunnel_crd};
    use tokio::net::UdpSocket;

    // A server that accepts the connection and then ignores every request.
    let sock = UdpSocket::bind("0.0.0.0:0").await.unwrap();
    let port = sock.local_addr().unwrap().port();
    let Some(knxd) = Knxd::start(Mode::Ipt {
        host_port: port,
        heartbeat_secs: 30,
    })
    .await
    else {
        return;
    };
    let relay = knxd.relay().await;
    let observer = TunnelConnection::connect(relay.addr).await.unwrap();

    let mut buf = [0u8; 1024];
    let mut client: Option<SocketAddr> = None;
    let mut requests = Vec::new();
    let started = Instant::now();
    let mut sent_one = false;
    while started.elapsed() < Duration::from_secs(12) {
        let Ok(Ok((n, from))) = timeout(Duration::from_millis(100), sock.recv_from(&mut buf)).await
        else {
            // Once knxd is linked, make it forward one telegram.
            if client.is_some() && !sent_one {
                observer
                    .send(group_write_from(0, MessageCode::LDataReq, 1))
                    .await
                    .unwrap();
                sent_one = true;
            }
            continue;
        };
        let Ok(frame) = KnxIpFrame::parse(&buf[..n]) else {
            continue;
        };
        match frame.service_type {
            ServiceType::ConnectRequest => {
                let mut body = vec![5, E_NO_ERROR];
                body.extend_from_slice(&Hpai::nat_udp(port).to_bytes());
                body.extend_from_slice(&tunnel_crd(0xFF01));
                let response = KnxIpFrame {
                    service_type: ServiceType::ConnectResponse,
                    body,
                };
                sock.send_to(&response.to_bytes(), from).await.unwrap();
                client = Some(from);
            }
            ServiceType::TunnelingRequest => requests.push(started.elapsed()),
            ServiceType::DisconnectRequest => {
                let response = KnxIpFrame {
                    service_type: ServiceType::DisconnectResponse,
                    body: vec![5, E_NO_ERROR],
                };
                sock.send_to(&response.to_bytes(), from).await.unwrap();
            }
            _ => {}
        }
    }
    eprintln!("tunnelling requests at {requests:?}");
    assert!(
        requests.len() >= 3,
        "knxd gave up after {} transmissions",
        requests.len()
    );
    let logs = knxd.logs();
    assert!(logs.contains("Too many retransmits"), "knxd log:\n{logs}");
}

#[tokio::test]
async fn knxd_forwards_what_a_bus_access_unit_answers() {
    use knx_rs_core::dpt::{DPT_VALUE_TEMP, DptValue};
    use knx_rs_device::bau::Bau;
    use knx_rs_device::device_object;
    use knx_rs_device::group_object::ComFlag;

    // A device with one temperature object at 1/0/1, as in the loopback test.
    let device = device_object::new_device_object([0x00, 0xFA, 0xDE, 0xD0, 0x00, 0x01], [0x00; 6]);
    let mut bau = Bau::new(device, 2, 2);
    device_object::set_individual_address(bau.device_mut(), DEVICE);
    bau.group_objects_mut()
        .get_mut(1)
        .unwrap()
        .set_dpt(DPT_VALUE_TEMP);
    bau.address_table_mut()
        .load(&[0x00, 0x02, 0x08, 0x01, 0x08, 0x02]);
    bau.association_table_mut()
        .load(&[0x00, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x02]);
    let go = bau.group_objects_mut().get_mut(1).unwrap();
    go.set_value(&DptValue::Float(21.5)).unwrap();
    go.set_comm_flag(ComFlag::Ok);

    let Some(rig) = Rig::start(30).await else {
        return;
    };
    let Rig {
        mut server,
        knxd,
        relay,
        mut observer,
    } = rig;

    // A visualisation reads 1/0/1 through knxd; knxd hands the read to the
    // device server, which feeds the bus access unit.
    let read = CemiFrame::new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0),
        DestinationAddress::Group(GroupAddress::from_raw(0x0801)),
        Priority::Low,
        &[0x00, 0x00],
    );
    observer.send(read).await.unwrap();
    let event = timeout(WAIT, server.recv()).await.expect("read event");
    let Some(ServerEvent::TunnelFrame(frame)) = event else {
        panic!("expected a tunnel frame, got {event:?}");
    };
    bau.process_frame(&frame, 0);
    bau.poll(0);
    let response = bau.next_outgoing_frame().expect("a group value response");
    server.send_frame(response).await.unwrap();

    // The response must come back out of knxd.
    let answer = timeout(WAIT, async {
        loop {
            let frame = observer.recv().await.expect("connection closed");
            if frame.destination_address_raw() == 0x0801 && frame.payload().len() >= 3 {
                return frame;
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "the response never came out of knxd; wire trace:\n{}\nknxd log:\n{}",
            relay.trace_text(),
            knxd.logs()
        )
    });
    assert_eq!(answer.message_code_raw(), MessageCode::LDataInd as u8);
    assert_eq!(answer.source_address(), IndividualAddress::from_raw(DEVICE));
    eprintln!("--- observer wire trace\n{}", relay.trace_text());
    eprintln!("--- knxd log\n{}", knxd.logs());
}

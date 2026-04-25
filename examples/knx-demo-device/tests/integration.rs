#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! Integration test: demo device with tunnel client interaction.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use knx_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
use knx_core::cemi::CemiFrame;
use knx_core::knxip::{ConnectionHeader, HostProtocol, Hpai, KnxIpFrame, ServiceType};
use knx_core::message::MessageCode;
use knx_core::types::Priority;
use knx_device::group_object::ComFlag;
use tokio::net::UdpSocket;
use tokio::time::timeout;

use knx_demo_device::create_demo_bau;
use knx_ip::tunnel_server::{DeviceServer, ServerEvent};

const TIMEOUT: Duration = Duration::from_secs(5);

async fn send_recv(socket: &UdpSocket, target: SocketAddr, data: &[u8]) -> Vec<u8> {
    socket.send_to(data, target).await.unwrap();
    let mut buf = [0u8; 512];
    let (n, _) = timeout(TIMEOUT, socket.recv_from(&mut buf))
        .await
        .expect("timeout")
        .unwrap();
    buf[..n].to_vec()
}

fn connect_request(port: u16) -> Vec<u8> {
    let h = Hpai {
        protocol: HostProtocol::Ipv4Udp,
        ip: [127, 0, 0, 1],
        port,
    }
    .to_bytes();
    let mut body = Vec::new();
    body.extend_from_slice(&h);
    body.extend_from_slice(&h);
    body.extend_from_slice(&[0x04, 0x04, 0x02, 0x00]);
    KnxIpFrame {
        service_type: ServiceType::ConnectRequest,
        body,
    }
    .to_bytes()
}

fn tunneling_request(ch_id: u8, seq: u8, cemi: &CemiFrame) -> Vec<u8> {
    let ch = ConnectionHeader {
        channel_id: ch_id,
        sequence_counter: seq,
        status: 0,
    };
    let mut body = Vec::new();
    body.extend_from_slice(&ch.to_bytes());
    body.extend_from_slice(cemi.as_bytes());
    KnxIpFrame {
        service_type: ServiceType::TunnelingRequest,
        body,
    }
    .to_bytes()
}

fn disconnect_request(ch_id: u8, port: u16) -> Vec<u8> {
    let h = Hpai {
        protocol: HostProtocol::Ipv4Udp,
        ip: [127, 0, 0, 1],
        port,
    }
    .to_bytes();
    let mut body = Vec::new();
    body.push(ch_id);
    body.push(0);
    body.extend_from_slice(&h);
    KnxIpFrame {
        service_type: ServiceType::DisconnectRequest,
        body,
    }
    .to_bytes()
}

#[tokio::test]
async fn demo_device_full_flow() {
    let mut bau = create_demo_bau(0x110A);
    let mut server = DeviceServer::start(Ipv4Addr::LOCALHOST).await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let port = client.local_addr().unwrap().port();
    let target: SocketAddr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 3671).into();

    // ── Connect ───────────────────────────────────────────────
    let resp = send_recv(&client, target, &connect_request(port)).await;
    let frame = KnxIpFrame::parse(&resp).unwrap();
    assert_eq!(frame.service_type, ServiceType::ConnectResponse);
    let ch_id = frame.body[0];
    assert_eq!(frame.body[1], 0x00, "connect should succeed");

    // ── Send GroupValueWrite (switch ON → 1/0/2) ──────────────
    let cemi = CemiFrame::new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0x1102),
        DestinationAddress::Group(GroupAddress::from_raw(0x0802)),
        Priority::Low,
        &[0x00, 0x81],
    );
    let ack = send_recv(&client, target, &tunneling_request(ch_id, 0, &cemi)).await;
    assert_eq!(
        KnxIpFrame::parse(&ack).unwrap().service_type,
        ServiceType::TunnelingAck
    );

    // Server should deliver the frame as an event
    let event = timeout(TIMEOUT, server.recv()).await.unwrap().unwrap();
    if let ServerEvent::TunnelFrame(f) = event {
        bau.process_frame(&f, 0);
    }

    // GO 2 should be updated with value 1
    let go2 = bau.group_objects().get(2).unwrap();
    assert_eq!(go2.comm_flag(), ComFlag::Updated);
    assert_eq!(go2.value_ref()[0], 0x01);

    // ── Send GroupValueRead (temperature → 1/0/1) ─────────────
    let read_cemi = CemiFrame::new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0x1102),
        DestinationAddress::Group(GroupAddress::from_raw(0x0801)),
        Priority::Low,
        &[0x00, 0x00], // GroupValueRead
    );
    let ack2 = send_recv(&client, target, &tunneling_request(ch_id, 1, &read_cemi)).await;
    assert_eq!(
        KnxIpFrame::parse(&ack2).unwrap().service_type,
        ServiceType::TunnelingAck
    );

    // Process the read request in the BAU
    let event2 = timeout(TIMEOUT, server.recv()).await.unwrap().unwrap();
    if let ServerEvent::TunnelFrame(f) = event2 {
        bau.process_frame(&f, 0);
    }

    // BAU should have generated a GroupValueResponse
    bau.poll(0);
    let response = bau.next_outgoing_frame();
    assert!(
        response.is_some(),
        "BAU should generate a GroupValueResponse"
    );

    // ── Disconnect ────────────────────────────────────────────
    let disc = send_recv(&client, target, &disconnect_request(ch_id, port)).await;
    assert_eq!(
        KnxIpFrame::parse(&disc).unwrap().service_type,
        ServiceType::DisconnectResponse
    );
}

#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! Integration test: tunnel server ↔ tunnel client on localhost.
//!
//! Spawns a `DeviceServer`, connects a `TunnelConnection` to it,
//! and verifies end-to-end frame exchange.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use knx_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
use knx_core::cemi::CemiFrame;
use knx_core::knxip::{ConnectionHeader, HostProtocol, Hpai, KnxIpFrame, ServiceType};
use knx_core::message::MessageCode;
use knx_core::types::Priority;
use tokio::net::UdpSocket;
use tokio::time::timeout;

use knx_ip::tunnel_server::{DeviceServer, ServerEvent};

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Build a connect request frame.
fn build_connect_request(local_port: u16) -> Vec<u8> {
    let hpai = Hpai {
        protocol: HostProtocol::Ipv4Udp,
        ip: [127, 0, 0, 1],
        port: local_port,
    };
    let hpai_bytes = hpai.to_bytes();
    let cri = [0x04, 0x04, 0x02, 0x00]; // tunnel, link layer

    let mut body = Vec::new();
    body.extend_from_slice(&hpai_bytes); // control endpoint
    body.extend_from_slice(&hpai_bytes); // data endpoint
    body.extend_from_slice(&cri);

    let frame = KnxIpFrame {
        service_type: ServiceType::ConnectRequest,
        body,
    };
    frame.to_bytes()
}

/// Build a heartbeat (connection state request).
fn build_heartbeat(channel_id: u8, local_port: u16) -> Vec<u8> {
    let hpai = Hpai {
        protocol: HostProtocol::Ipv4Udp,
        ip: [127, 0, 0, 1],
        port: local_port,
    };
    let mut body = Vec::new();
    body.push(channel_id);
    body.push(0);
    body.extend_from_slice(&hpai.to_bytes());

    KnxIpFrame {
        service_type: ServiceType::ConnectionStateRequest,
        body,
    }
    .to_bytes()
}

/// Build a tunneling request with a CEMI frame.
fn build_tunneling_request(channel_id: u8, seq: u8, cemi: &CemiFrame) -> Vec<u8> {
    let ch = ConnectionHeader {
        channel_id,
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

/// Build a disconnect request.
fn build_disconnect(channel_id: u8, local_port: u16) -> Vec<u8> {
    let hpai = Hpai {
        protocol: HostProtocol::Ipv4Udp,
        ip: [127, 0, 0, 1],
        port: local_port,
    };
    let mut body = Vec::new();
    body.push(channel_id);
    body.push(0);
    body.extend_from_slice(&hpai.to_bytes());

    KnxIpFrame {
        service_type: ServiceType::DisconnectRequest,
        body,
    }
    .to_bytes()
}

/// Helper: send a packet and receive the response.
async fn send_recv(socket: &UdpSocket, target: SocketAddr, data: &[u8]) -> Vec<u8> {
    socket.send_to(data, target).await.unwrap();
    let mut buf = [0u8; 512];
    let (n, _) = timeout(TEST_TIMEOUT, socket.recv_from(&mut buf))
        .await
        .expect("timeout waiting for response")
        .unwrap();
    buf[..n].to_vec()
}

#[tokio::test]
async fn connect_and_disconnect() {
    let server = DeviceServer::start(Ipv4Addr::LOCALHOST).await.unwrap();

    // Client socket
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_port = client.local_addr().unwrap().port();
    let server_addr: SocketAddr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 3671).into();

    // Connect
    let resp_bytes = send_recv(&client, server_addr, &build_connect_request(client_port)).await;
    let resp = KnxIpFrame::parse(&resp_bytes).unwrap();
    assert_eq!(resp.service_type, ServiceType::ConnectResponse);
    assert!(resp.body.len() >= 2);
    let channel_id = resp.body[0];
    let status = resp.body[1];
    assert_eq!(status, 0x00, "connect should succeed");
    assert_ne!(channel_id, 0);

    // Heartbeat
    let hb_resp = send_recv(
        &client,
        server_addr,
        &build_heartbeat(channel_id, client_port),
    )
    .await;
    let hb = KnxIpFrame::parse(&hb_resp).unwrap();
    assert_eq!(hb.service_type, ServiceType::ConnectionStateResponse);
    assert_eq!(hb.body[0], channel_id);
    assert_eq!(hb.body[1], 0x00); // no error

    // Disconnect
    let disc_resp = send_recv(
        &client,
        server_addr,
        &build_disconnect(channel_id, client_port),
    )
    .await;
    let disc = KnxIpFrame::parse(&disc_resp).unwrap();
    assert_eq!(disc.service_type, ServiceType::DisconnectResponse);
    assert_eq!(disc.body[0], channel_id);

    server.stop().await;
}

#[tokio::test]
async fn tunnel_frame_exchange() {
    let mut server = DeviceServer::start(Ipv4Addr::LOCALHOST).await.unwrap();

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_port = client.local_addr().unwrap().port();
    let server_addr: SocketAddr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 3671).into();

    // Connect
    let resp_bytes = send_recv(&client, server_addr, &build_connect_request(client_port)).await;
    let resp = KnxIpFrame::parse(&resp_bytes).unwrap();
    let channel_id = resp.body[0];

    // Send a CEMI frame through the tunnel
    let cemi = CemiFrame::new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0x1101),
        DestinationAddress::Group(GroupAddress::from_raw(0x0801)),
        Priority::Low,
        &[0x00, 0x81], // GroupValueWrite(true)
    );

    let tunnel_req = build_tunneling_request(channel_id, 0, &cemi);
    let ack_bytes = send_recv(&client, server_addr, &tunnel_req).await;
    let ack = KnxIpFrame::parse(&ack_bytes).unwrap();
    assert_eq!(ack.service_type, ServiceType::TunnelingAck);

    // Server should have received the frame as a TunnelFrame event
    let event = timeout(TEST_TIMEOUT, server.recv())
        .await
        .expect("timeout")
        .expect("event");
    assert!(matches!(event, ServerEvent::TunnelFrame(_)));

    // Cleanup
    client
        .send_to(&build_disconnect(channel_id, client_port), server_addr)
        .await
        .unwrap();
    server.stop().await;
}

#[tokio::test]
async fn server_sends_frame_to_tunnel_client() {
    let server = DeviceServer::start(Ipv4Addr::LOCALHOST).await.unwrap();

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_port = client.local_addr().unwrap().port();
    let server_addr: SocketAddr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 3671).into();

    // Connect
    let resp_bytes = send_recv(&client, server_addr, &build_connect_request(client_port)).await;
    let resp = KnxIpFrame::parse(&resp_bytes).unwrap();
    let channel_id = resp.body[0];

    // Server sends a frame to the tunnel client
    let cemi = CemiFrame::new_l_data(
        MessageCode::LDataInd,
        IndividualAddress::from_raw(0x1102),
        DestinationAddress::Group(GroupAddress::from_raw(0x0801)),
        Priority::Low,
        &[0x00, 0x80], // GroupValueWrite(false)
    );
    server.send_to_tunnel(channel_id, cemi).await.unwrap();

    // Client should receive a tunneling request
    let mut buf = [0u8; 512];
    let (n, _) = timeout(TEST_TIMEOUT, client.recv_from(&mut buf))
        .await
        .expect("timeout")
        .unwrap();
    let frame = KnxIpFrame::parse(&buf[..n]).unwrap();
    assert_eq!(frame.service_type, ServiceType::TunnelingRequest);

    let ch = ConnectionHeader::parse(&frame.body).unwrap();
    assert_eq!(ch.channel_id, channel_id);
    assert_eq!(ch.sequence_counter, 0);

    // Cleanup
    client
        .send_to(&build_disconnect(channel_id, client_port), server_addr)
        .await
        .unwrap();
    server.stop().await;
}

#[tokio::test]
async fn heartbeat_with_wrong_channel_returns_error() {
    let _server = DeviceServer::start(Ipv4Addr::LOCALHOST).await.unwrap();

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_port = client.local_addr().unwrap().port();
    let server_addr: SocketAddr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 3671).into();

    // Heartbeat with non-existent channel
    let hb_resp = send_recv(&client, server_addr, &build_heartbeat(0xFF, client_port)).await;
    let hb = KnxIpFrame::parse(&hb_resp).unwrap();
    assert_eq!(hb.service_type, ServiceType::ConnectionStateResponse);
    assert_eq!(hb.body[0], 0xFF);
    assert_ne!(hb.body[1], 0x00); // should be error
}

#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! The device server answers search and description requests (issue 105).

mod common;

use std::net::SocketAddr;
use std::time::Duration;

use common::{RawClient, WAIT};
use knx_rs_core::address::IndividualAddress;
use knx_rs_core::knxip::dib::{
    DeviceInformationDib, DibSequence, DibType, KnxMedium, ServiceFamily,
};
use knx_rs_core::knxip::srp::{
    mac_address_srp, programming_mode_srp, request_dibs_srp, service_srp,
};
use knx_rs_core::knxip::{E_NO_ERROR, HostProtocol, Hpai, KnxIpFrame, ServiceType};
use knx_rs_ip::{DeviceDescription, DeviceServer};
use tokio::net::UdpSocket;

const SILENCE: Duration = Duration::from_millis(400);

fn description() -> DeviceDescription {
    DeviceDescription::new(
        "SnapDog Living",
        IndividualAddress::from_raw(0x1164),
        [0, 1, 2, 3, b'S', b'D'],
        [0x02, 0x42, 0xAC, 0x11, 0x00, 0x10],
    )
}

async fn start_with(description: DeviceDescription) -> DeviceServer {
    DeviceServer::start_at_with("127.0.0.1:0".parse().unwrap(), description)
        .await
        .unwrap()
}

async fn client() -> UdpSocket {
    UdpSocket::bind("127.0.0.1:0").await.unwrap()
}

async fn receive(socket: &UdpSocket, wait: Duration) -> Option<(KnxIpFrame, SocketAddr)> {
    let mut buf = [0u8; 512];
    let (n, from) = tokio::time::timeout(wait, socket.recv_from(&mut buf))
        .await
        .ok()?
        .ok()?;
    Some((KnxIpFrame::parse(&buf[..n]).unwrap(), from))
}

/// The `(family, version)` pairs of the Supported Service Families DIB.
fn families(dibs: &[u8]) -> Vec<(u8, u8)> {
    let sequence = DibSequence::parse(dibs).unwrap();
    let dib = sequence.get(DibType::SupportedServiceFamilies).unwrap();
    dib.data()
        .chunks(2)
        .map(|pair| (pair[0], pair[1]))
        .collect()
}

/// Send `request` to the server from `socket`, and wait for its answer there.
async fn ask(
    socket: &UdpSocket,
    server: SocketAddr,
    request: &KnxIpFrame,
) -> (KnxIpFrame, SocketAddr) {
    socket.send_to(&request.to_bytes(), server).await.unwrap();
    receive(socket, WAIT).await.expect("an answer")
}

fn search_from(socket: &UdpSocket) -> KnxIpFrame {
    KnxIpFrame::search_request(Hpai {
        protocol: HostProtocol::Ipv4Udp,
        ip: [127, 0, 0, 1],
        port: socket.local_addr().unwrap().port(),
    })
}

#[tokio::test]
async fn a_search_request_is_answered_with_the_endpoint_and_the_description() {
    let server = start_with(description()).await;
    let socket = client().await;
    let (response, from) = ask(&socket, server.local_addr(), &search_from(&socket)).await;
    assert_eq!(
        from,
        server.local_addr(),
        "the answer comes from the server's own port"
    );
    assert_eq!(response.service_type, ServiceType::SearchResponse);

    // Control endpoint: the address and port the server listens on.
    let control = Hpai::parse(&response.body).unwrap();
    assert_eq!(control.protocol, HostProtocol::Ipv4Udp);
    assert_eq!(control.ip, [127, 0, 0, 1]);
    assert_eq!(control.port, server.local_addr().port());

    let dibs = &response.body[usize::from(Hpai::LEN)..];
    let device = DeviceInformationDib::find_in(dibs)
        .unwrap()
        .expect("device information");
    assert_eq!(device.friendly_name(), "SnapDog Living");
    assert_eq!(device.individual_address(), 0x1164);
    assert_eq!(device.knx_medium(), Some(KnxMedium::Ip));
    assert_eq!(device.serial_number(), [0, 1, 2, 3, b'S', b'D']);
    assert_eq!(device.mac_address(), [0x02, 0x42, 0xAC, 0x11, 0x00, 0x10]);
    assert!(!device.is_programming_mode());
    assert_eq!(
        device.routing_multicast_address(),
        [0; 4],
        "no multicast group, no routing"
    );
    // Core and tunnelling, no routing, no device management.
    assert_eq!(families(dibs), [(0x02, 1), (0x04, 1)]);
}

#[tokio::test]
async fn a_description_request_is_answered_with_the_same_blocks() {
    let server = start_with(description()).await;
    let socket = client().await;
    let (search, _) = ask(&socket, server.local_addr(), &search_from(&socket)).await;

    let request = KnxIpFrame::description_request(Hpai {
        protocol: HostProtocol::Ipv4Udp,
        ip: [127, 0, 0, 1],
        port: socket.local_addr().unwrap().port(),
    });
    let (response, _) = ask(&socket, server.local_addr(), &request).await;
    assert_eq!(response.service_type, ServiceType::DescriptionResponse);
    assert_eq!(
        response.body,
        search.body[usize::from(Hpai::LEN)..],
        "a description is the blocks of a search response, without the endpoint"
    );
}

#[tokio::test]
async fn the_answer_goes_to_the_endpoint_the_request_names() {
    let server = start_with(description()).await;
    let asking = client().await;
    let listening = client().await;
    let request = KnxIpFrame::search_request(Hpai {
        protocol: HostProtocol::Ipv4Udp,
        ip: [127, 0, 0, 1],
        port: listening.local_addr().unwrap().port(),
    });
    asking
        .send_to(&request.to_bytes(), server.local_addr())
        .await
        .unwrap();

    let (response, _) = receive(&listening, WAIT)
        .await
        .expect("an answer at the named endpoint");
    assert_eq!(response.service_type, ServiceType::SearchResponse);
    assert!(
        receive(&asking, SILENCE).await.is_none(),
        "nothing goes to the sender when the request names another endpoint"
    );
}

#[tokio::test]
async fn a_request_in_nat_mode_is_answered_to_its_sender() {
    let server = start_with(description()).await;
    let socket = client().await;
    // 0.0.0.0:0, what a client behind NAT (or one with an IPv6 socket) sends.
    for request in [
        KnxIpFrame::search_request(Hpai::nat_udp(0)),
        KnxIpFrame::description_request(Hpai::nat_udp(0)),
    ] {
        let (response, from) = ask(&socket, server.local_addr(), &request).await;
        assert_eq!(from, server.local_addr());
        assert!(matches!(
            response.service_type,
            ServiceType::SearchResponse | ServiceType::DescriptionResponse
        ));
    }
}

/// A server bound to all interfaces cannot name its address from the socket: the
/// control endpoint has to be one the requester can reach, not 0.0.0.0.
#[tokio::test]
async fn bound_to_all_interfaces_the_control_endpoint_is_the_address_that_reaches_the_requester() {
    let server = DeviceServer::start_at_with("0.0.0.0:0".parse().unwrap(), description())
        .await
        .unwrap();
    let socket = client().await;
    let target: SocketAddr = format!("127.0.0.1:{}", server.local_addr().port())
        .parse()
        .unwrap();
    let (response, _) = ask(&socket, target, &search_from(&socket)).await;
    let control = Hpai::parse(&response.body).unwrap();
    assert!(
        !control.is_unspecified(),
        "a wildcard is no endpoint to connect to"
    );
    assert_eq!(
        control.ip,
        [127, 0, 0, 1],
        "the address towards the requester"
    );
    assert_eq!(control.port, server.local_addr().port());
}

/// The description the server was started with is what a client connected by
/// that endpoint can use: a tunnel connects to the control endpoint of the answer.
#[tokio::test]
async fn the_control_endpoint_of_the_answer_accepts_a_tunnel() {
    let server = start_with(description()).await;
    let socket = client().await;
    let (response, _) = ask(&socket, server.local_addr(), &search_from(&socket)).await;
    let control = Hpai::parse(&response.body).unwrap();
    let endpoint: SocketAddr = format!(
        "{}.{}.{}.{}:{}",
        control.ip[0], control.ip[1], control.ip[2], control.ip[3], control.port
    )
    .parse()
    .unwrap();
    let tunnel = RawClient::connect(endpoint).await;
    assert_eq!(tunnel.heartbeat().await, E_NO_ERROR);
}

#[tokio::test]
async fn a_server_without_a_description_does_not_answer() {
    let server = DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let socket = client().await;
    let search = search_from(&socket);
    let description = KnxIpFrame::description_request(Hpai::nat_udp(0));
    for request in [&search, &description] {
        socket
            .send_to(&request.to_bytes(), server.local_addr())
            .await
            .unwrap();
        assert!(
            receive(&socket, SILENCE).await.is_none(),
            "{:?} answered without a description",
            request.service_type
        );
    }
    // It serves tunnels all the same.
    assert_eq!(
        RawClient::connect(server.local_addr())
            .await
            .heartbeat()
            .await,
        E_NO_ERROR
    );
}

#[tokio::test]
async fn set_description_starts_the_answers_and_follows_address_and_programming_mode() {
    let server = DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let socket = client().await;
    socket
        .send_to(&search_from(&socket).to_bytes(), server.local_addr())
        .await
        .unwrap();
    assert!(receive(&socket, SILENCE).await.is_none());

    server.set_description(description());
    let device = |response: &KnxIpFrame| {
        let dibs = &response.body[usize::from(Hpai::LEN)..];
        let device = DeviceInformationDib::find_in(dibs).unwrap().unwrap();
        (device.individual_address(), device.is_programming_mode())
    };
    let (response, _) = ask(&socket, server.local_addr(), &search_from(&socket)).await;
    assert_eq!(device(&response), (0x1164, false));

    // ETS assigned an address, and the device is in programming mode.
    let mut changed = description();
    changed.individual_address = IndividualAddress::from_raw(0x1201);
    changed.programming_mode = true;
    server.set_description(changed);
    let (response, _) = ask(&socket, server.local_addr(), &search_from(&socket)).await;
    assert_eq!(device(&response), (0x1201, true));
}

#[tokio::test]
async fn requests_that_cannot_be_answered_are_ignored_and_the_server_goes_on() {
    let server = start_with(description()).await;
    let socket = client().await;
    let service = |service_type, body: Vec<u8>| KnxIpFrame { service_type, body }.to_bytes();
    let tcp_endpoint = Hpai {
        protocol: HostProtocol::Ipv4Tcp,
        ip: [127, 0, 0, 1],
        port: 3671,
    };
    for datagram in [
        service(ServiceType::SearchRequest, Vec::new()),
        service(ServiceType::SearchRequest, vec![8, 1, 127]),
        service(ServiceType::DescriptionRequest, vec![0xFF; 8]),
        // A TCP endpoint is not one a UDP answer can go to.
        service(ServiceType::SearchRequest, tcp_endpoint.to_bytes().to_vec()),
        // The extended search with a body that holds no HPAI.
        service(ServiceType::SearchRequestExtended, vec![8, 1, 127]),
    ] {
        socket
            .send_to(&datagram, server.local_addr())
            .await
            .unwrap();
        assert!(receive(&socket, Duration::from_millis(150)).await.is_none());
    }
    let (response, _) = ask(&socket, server.local_addr(), &search_from(&socket)).await;
    assert_eq!(response.service_type, ServiceType::SearchResponse);
}

/// With the multicast group joined, the server routes too, and says so. This one binds
/// the real port 3671 like the tunnel integration tests do, one process per test.
#[tokio::test]
async fn a_server_that_joined_the_multicast_group_advertises_routing() {
    let server = DeviceServer::start_with(std::net::Ipv4Addr::LOCALHOST, description())
        .await
        .unwrap();
    let socket = client().await;
    let target: SocketAddr = format!("127.0.0.1:{}", server.local_addr().port())
        .parse()
        .unwrap();
    let (response, _) = ask(&socket, target, &search_from(&socket)).await;
    let dibs = &response.body[usize::from(Hpai::LEN)..];
    let device = DeviceInformationDib::find_in(dibs).unwrap().unwrap();
    assert_eq!(device.routing_multicast_address(), [224, 0, 23, 12]);
    assert_eq!(families(dibs), [(0x02, 1), (0x04, 1), (0x05, 1)]);
}

// ── The extended search ─────────────────────────────────────────

fn extended_from(socket: &UdpSocket, srps: &[u8]) -> KnxIpFrame {
    KnxIpFrame::search_request_extended(
        Hpai {
            protocol: HostProtocol::Ipv4Udp,
            ip: [127, 0, 0, 1],
            port: socket.local_addr().unwrap().port(),
        },
        srps,
    )
}

/// Send `request`, and return the answer if there is one within a moment.
async fn answered(
    socket: &UdpSocket,
    server: SocketAddr,
    request: &KnxIpFrame,
) -> Option<KnxIpFrame> {
    socket.send_to(&request.to_bytes(), server).await.unwrap();
    receive(socket, SILENCE).await.map(|(frame, _)| frame)
}

#[tokio::test]
async fn an_extended_search_is_answered_with_the_blocks_of_a_plain_search() {
    let server = start_with(description()).await;
    let socket = client().await;
    let (plain, _) = ask(&socket, server.local_addr(), &search_from(&socket)).await;
    let (extended, from) = ask(&socket, server.local_addr(), &extended_from(&socket, &[])).await;
    assert_eq!(from, server.local_addr());
    assert_eq!(extended.service_type, ServiceType::SearchResponseExtended);
    assert_eq!(
        extended.body, plain.body,
        "the same control endpoint and the same blocks, another service type"
    );
}

/// What XKNX's gateway scanner sends: an extended search asking for four blocks, and a
/// plain one for devices of the first version. Both are answered, naming the same
/// control endpoint, so that the scanner (which keys what it finds by that endpoint)
/// sees one gateway.
#[tokio::test]
async fn the_requests_of_xknxs_scanner_are_both_answered_with_one_gateway() {
    let server = start_with(description()).await;
    let socket = client().await;
    let wanted = request_dibs_srp(&[
        DibType::DeviceInformation,
        DibType::SupportedServiceFamilies,
        DibType::SecuredServiceFamilies,
        DibType::TunnelingInformation,
    ])
    .unwrap();
    socket
        .send_to(
            &extended_from(&socket, &wanted).to_bytes(),
            server.local_addr(),
        )
        .await
        .unwrap();
    socket
        .send_to(&search_from(&socket).to_bytes(), server.local_addr())
        .await
        .unwrap();

    let mut answers = Vec::new();
    while let Some((frame, _)) = receive(&socket, SILENCE).await {
        answers.push(frame);
    }
    assert_eq!(answers.len(), 2, "one answer to each request");
    let extended = answers
        .iter()
        .find(|f| f.service_type == ServiceType::SearchResponseExtended)
        .expect("the extended answer");
    let plain = answers
        .iter()
        .find(|f| f.service_type == ServiceType::SearchResponse)
        .expect("the plain answer");
    assert_eq!(Hpai::parse(&extended.body), Hpai::parse(&plain.body));

    // The blocks the server does not have are left out, and the two it has are there.
    let dibs = &extended.body[usize::from(Hpai::LEN)..];
    let sequence = DibSequence::parse(dibs).unwrap();
    assert!(sequence.get(DibType::DeviceInformation).is_some());
    assert!(sequence.get(DibType::SupportedServiceFamilies).is_some());
    assert!(sequence.get(DibType::SecuredServiceFamilies).is_none());
    assert!(sequence.get(DibType::TunnelingInformation).is_none());
    // Core in version 1: a client that sees version 2 would expect more than the server has.
    assert_eq!(families(dibs), [(0x02, 1), (0x04, 1)]);
}

#[tokio::test]
async fn the_programming_mode_selector_makes_only_a_server_in_programming_mode_answer() {
    let server = start_with(description()).await;
    let socket = client().await;
    let request = extended_from(&socket, &programming_mode_srp());
    assert!(
        answered(&socket, server.local_addr(), &request)
            .await
            .is_none(),
        "a server that is not in programming mode stays silent"
    );

    let mut programming = description();
    programming.programming_mode = true;
    server.set_description(programming);
    let response = answered(&socket, server.local_addr(), &request)
        .await
        .expect("in programming mode, the server answers");
    let dibs = &response.body[usize::from(Hpai::LEN)..];
    assert!(
        DeviceInformationDib::find_in(dibs)
            .unwrap()
            .unwrap()
            .is_programming_mode()
    );
}

#[tokio::test]
async fn the_mac_selector_makes_only_the_server_with_that_address_answer() {
    let server = start_with(description()).await;
    let socket = client().await;
    let own = [0x02, 0x42, 0xAC, 0x11, 0x00, 0x10];
    let ask_for = |mac| extended_from(&socket, &mac_address_srp(mac));
    assert!(
        answered(&socket, server.local_addr(), &ask_for(own))
            .await
            .is_some()
    );
    assert!(
        answered(&socket, server.local_addr(), &ask_for([1; 6]))
            .await
            .is_none()
    );
}

#[tokio::test]
async fn the_service_selector_makes_only_a_server_with_the_family_in_the_version_answer() {
    let server = start_with(description()).await;
    let socket = client().await;
    let ask_for = |family, version| extended_from(&socket, &service_srp(family, version));
    assert!(
        answered(
            &socket,
            server.local_addr(),
            &ask_for(ServiceFamily::Tunneling, 1)
        )
        .await
        .is_some()
    );
    // Version 2 of tunnelling is the TCP form; routing needs the multicast group.
    for (family, version) in [
        (ServiceFamily::Tunneling, 2),
        (ServiceFamily::Core, 2),
        (ServiceFamily::Routing, 1),
    ] {
        assert!(
            answered(&socket, server.local_addr(), &ask_for(family, version))
                .await
                .is_none(),
            "{family:?} {version}"
        );
    }
}

#[tokio::test]
async fn an_unknown_parameter_silences_the_server_only_when_it_is_mandatory() {
    let server = start_with(description()).await;
    let socket = client().await;
    // The reference "invalid" SRP type, with and without the mandatory flag.
    assert!(
        answered(
            &socket,
            server.local_addr(),
            &extended_from(&socket, &[0x02, 0x80])
        )
        .await
        .is_none()
    );
    assert!(
        answered(
            &socket,
            server.local_addr(),
            &extended_from(&socket, &[0x02, 0x00])
        )
        .await
        .is_some()
    );
}

#[tokio::test]
async fn a_malformed_parameter_block_is_not_answered_and_the_server_goes_on() {
    let server = start_with(description()).await;
    let socket = client().await;
    for srps in [&[0x08u8, 0x82, 1, 2][..], &[0x01, 0x81], &[0x02]] {
        assert!(
            answered(&socket, server.local_addr(), &extended_from(&socket, srps))
                .await
                .is_none(),
            "{srps:02X?}"
        );
    }
    assert!(
        answered(&socket, server.local_addr(), &extended_from(&socket, &[]))
            .await
            .is_some()
    );
}

#[tokio::test]
async fn the_extended_answer_goes_to_the_endpoint_the_request_names_and_never_without_a_description()
 {
    let server = start_with(description()).await;
    let asking = client().await;
    let listening = client().await;
    asking
        .send_to(
            &extended_from(&listening, &[]).to_bytes(),
            server.local_addr(),
        )
        .await
        .unwrap();
    let (response, _) = receive(&listening, WAIT)
        .await
        .expect("an answer at the named endpoint");
    assert_eq!(response.service_type, ServiceType::SearchResponseExtended);
    assert!(receive(&asking, SILENCE).await.is_none());

    let silent = DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    assert!(
        answered(&asking, silent.local_addr(), &extended_from(&asking, &[]))
            .await
            .is_none(),
        "a server without a description does not answer the extended search either"
    );
}

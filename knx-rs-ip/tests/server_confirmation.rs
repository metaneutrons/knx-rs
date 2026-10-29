#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! A device server confirms what a tunnel client sent it (`L_Data.con`).

mod common;

use std::time::Duration;

use common::{RawClient, WAIT, group_write};
use knx_rs_core::message::MessageCode;
use knx_rs_ip::{DeviceServer, ServerEvent};

#[tokio::test]
async fn a_request_of_a_client_is_confirmed_and_still_reaches_the_application() {
    let mut server = DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let client = RawClient::connect(server.local_addr()).await;

    let request = group_write(MessageCode::LDataReq, 7);
    client.send_request(0, &request).await;
    assert!(
        client.recv_ack(0, WAIT).await,
        "the request is acknowledged"
    );

    let (seq, cemi) = client.recv_request(WAIT).await.expect("confirmation");
    client.ack(seq).await;
    assert_eq!(cemi[0], MessageCode::LDataCon as u8, "message code");
    assert_eq!(
        &cemi[1..],
        &request.as_bytes()[1..],
        "everything but the message code is the client's frame (confirm flag clear)"
    );

    let Some(ServerEvent::TunnelFrame(frame)) = server.recv().await else {
        panic!("the application did not get the frame");
    };
    assert_eq!(frame.as_bytes(), request.as_bytes());
}

#[tokio::test]
async fn confirmations_come_in_the_order_of_the_requests() {
    let server = DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let client = RawClient::connect(server.local_addr()).await;

    for (seq, value) in [(0u8, 1u8), (1, 2), (2, 3)] {
        client
            .send_request(seq, &group_write(MessageCode::LDataReq, value))
            .await;
        assert!(client.recv_ack(seq, WAIT).await);
    }
    for value in [1u8, 2, 3] {
        let (seq, cemi) = client.recv_request(WAIT).await.expect("confirmation");
        client.ack(seq).await;
        assert_eq!(cemi[0], MessageCode::LDataCon as u8);
        assert_eq!(
            cemi,
            confirmation_bytes(&group_write(MessageCode::LDataReq, value)),
            "confirmation of request {value}"
        );
    }
}

#[tokio::test]
async fn an_indication_of_a_client_is_not_confirmed() {
    let server = DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let client = RawClient::connect(server.local_addr()).await;

    client
        .send_request(0, &group_write(MessageCode::LDataInd, 7))
        .await;
    assert!(client.recv_ack(0, WAIT).await);
    assert!(
        client
            .recv_request(Duration::from_millis(700))
            .await
            .is_none(),
        "only an L_Data.req is confirmed"
    );
}

fn confirmation_bytes(request: &knx_rs_core::cemi::CemiFrame) -> Vec<u8> {
    let mut bytes = request.as_bytes().to_vec();
    bytes[0] = MessageCode::LDataCon as u8;
    bytes
}

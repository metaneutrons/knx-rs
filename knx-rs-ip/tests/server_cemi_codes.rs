#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! Message codes of the frames a device server delivers to tunnel clients.

mod common;

use common::{RawClient, WAIT, group_write};
use knx_rs_core::message::MessageCode;
use knx_rs_ip::DeviceServer;

#[tokio::test]
async fn frames_handed_over_as_requests_are_delivered_as_indications() {
    let server = DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let client = RawClient::connect(server.local_addr()).await;

    server
        .send_frame(group_write(MessageCode::LDataReq, 1))
        .await
        .unwrap();
    let (seq, cemi) = client.recv_request(WAIT).await.expect("tunnelling request");
    client.ack(seq).await;
    assert_eq!(cemi[0], MessageCode::LDataInd as u8, "send_frame");

    server
        .send_to_tunnel(client.channel_id, group_write(MessageCode::LDataReq, 1))
        .await
        .unwrap();
    let (seq, cemi) = client.recv_request(WAIT).await.expect("tunnelling request");
    client.ack(seq).await;
    assert_eq!(cemi[0], MessageCode::LDataInd as u8, "send_to_tunnel");

    // A confirmation is not touched.
    server
        .send_to_tunnel(client.channel_id, group_write(MessageCode::LDataCon, 1))
        .await
        .unwrap();
    let (seq, cemi) = client.recv_request(WAIT).await.expect("tunnelling request");
    client.ack(seq).await;
    assert_eq!(cemi[0], MessageCode::LDataCon as u8, "confirmation");
}

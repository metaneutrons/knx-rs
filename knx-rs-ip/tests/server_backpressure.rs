#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! The server task must never wait for its consumer.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{RawClient, group_write};
use knx_rs_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::message::MessageCode;
use knx_rs_core::types::Priority;
use knx_rs_ip::{DeviceServer, ServerEvent};
use tokio::time::timeout;

/// A group write from 1.1.2 to the group address `ga` (raw).
fn write_to(ga: u16) -> CemiFrame {
    CemiFrame::new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0x1102),
        DestinationAddress::Group(GroupAddress::from_raw(ga)),
        Priority::Low,
        &[0x00, 0x81],
    )
}

#[tokio::test]
async fn consumer_that_sends_from_its_receive_loop_does_not_deadlock() {
    const BURST: u16 = 400;
    const SENDS_PER_EVENT: usize = 20;

    let mut server = DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let client = Arc::new(RawClient::connect(server.local_addr()).await);
    let _acker = Arc::clone(&client).spawn_acker();

    // A burst larger than the event queue (64) arrives in one go. The group
    // address counts the requests, so the order can be checked.
    for i in 0..BURST {
        let seq = u8::try_from(i % 256).unwrap();
        client.send_request(seq, &write_to(i)).await;
    }

    // The consumer answers every event with a batch of frames, from the same
    // task that receives, which is how a bus access unit is driven.
    let consumer = async {
        for expected in 0..BURST {
            let Some(ServerEvent::TunnelFrame(frame)) = server.recv().await else {
                panic!("expected a tunnel frame");
            };
            assert_eq!(frame.destination_address_raw(), expected, "event order");
            for _ in 0..SENDS_PER_EVENT {
                server
                    .send_frame(group_write(MessageCode::LDataInd, 0))
                    .await
                    .unwrap();
            }
        }
    };
    timeout(Duration::from_secs(30), consumer)
        .await
        .expect("server task and consumer deadlocked");
}

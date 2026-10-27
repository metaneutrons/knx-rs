#![allow(clippy::unwrap_used, clippy::expect_used)]
// SPDX-License-Identifier: GPL-3.0-only

//! A connection-oriented management session, as ETS runs it, through a tunnel,
//! the `DeviceServer` and a `Bau`: connect, read the device descriptor, write
//! and read memory, reset, disconnect.
//!
//! The frame-driven tests of the device stack use connectionless services; this
//! one drives the transport layer, the numbered data and the acknowledgements
//! as well, over the whole stack.

mod common;

use std::time::Duration;

use common::RawClient;
use knx_rs_core::address::{DestinationAddress, IndividualAddress};
use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::message::{ApduType, MessageCode, TpduType};
use knx_rs_core::tpdu::{Tpdu, encode_control, encode_data_connected};
use knx_rs_core::types::Priority;
use knx_rs_device::bau::Bau;
use knx_rs_device::device_object;
use knx_rs_ip::{DeviceServer, ServerEvent};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// The device under test, 1.1.1, and the programming tool, 15.15.177.
const DEVICE: u16 = 0x1101;
const MASTER: u16 = 0xFFB1;

/// Drive `bau` from `server` until told to stop; hands the stack back.
fn run_device(mut server: DeviceServer, mut bau: Bau) -> (oneshot::Sender<()>, JoinHandle<Bau>) {
    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let started = Instant::now();
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        loop {
            let now_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            tokio::select! {
                _ = &mut stop_rx => break,
                event = server.recv() => match event {
                    Some(ServerEvent::TunnelFrame(frame) | ServerEvent::RoutingFrame(frame)) => {
                        bau.process_frame(&frame, now_ms);
                    }
                    None => break,
                },
                _ = tick.tick() => bau.poll(now_ms),
            }
            while let Some(frame) = bau.next_outgoing_frame() {
                server.send_frame(frame).await.unwrap();
            }
        }
        server.shutdown().await;
        bau
    });
    (stop_tx, handle)
}

/// The programming tool's side of the session.
struct Master {
    client: RawClient,
    /// Tunnelling sequence counter of our requests.
    tunnel_seq: u8,
    /// Transport sequence number of our next numbered data.
    data_seq: u8,
}

impl Master {
    async fn send_tpdu(&mut self, tpdu: &[u8]) {
        let frame = CemiFrame::new_l_data(
            MessageCode::LDataReq,
            IndividualAddress::from_raw(MASTER),
            DestinationAddress::Individual(IndividualAddress::from_raw(DEVICE)),
            Priority::System,
            tpdu,
        );
        self.client.send_request(self.tunnel_seq, &frame).await;
        self.tunnel_seq = self.tunnel_seq.wrapping_add(1);
    }

    /// Send a numbered A_ service (`apdu` starts at the APCI octets).
    async fn send_apdu(&mut self, apdu: &[u8]) {
        let tpdu = encode_data_connected(self.data_seq, apdu);
        self.data_seq += 1;
        self.send_tpdu(&tpdu).await;
    }

    /// Collect what the device sends for a moment, acknowledging its numbered
    /// data and its tunnelling requests; returns the TPDU octets of every frame
    /// addressed to us.
    async fn collect(&mut self) -> Vec<Vec<u8>> {
        let mut seen = Vec::new();
        while let Some((seq, cemi)) = self.client.recv_request(Duration::from_millis(300)).await {
            self.client.ack(seq).await;
            let frame = CemiFrame::parse(&cemi).unwrap();
            if frame.destination_address_raw() != MASTER {
                continue;
            }
            if let Some(Tpdu::Data {
                tpdu_type: TpduType::DataConnected,
                sequence_number,
                ..
            }) = frame.tpdu()
            {
                self.send_tpdu(&encode_control(TpduType::Ack, sequence_number))
                    .await;
            } else if frame.tpdu().is_none() && frame.payload()[0] & 0x40 != 0 {
                // Numbered data of a service this crate cannot name (the master
                // reset response, APCI 0x3A1): acknowledge it by its sequence.
                let sequence_number = (frame.payload()[0] >> 2) & 0x0F;
                self.send_tpdu(&encode_control(TpduType::Ack, sequence_number))
                    .await;
            }
            seen.push(frame.payload().to_vec());
        }
        seen
    }
}

/// The APDU octets of the numbered data in `frames` that is of type `t`.
fn apdu_of(frames: &[Vec<u8>], t: ApduType) -> Option<Vec<u8>> {
    let apci = t as u16;
    frames.iter().find_map(|tpdu| {
        let numbered = tpdu.len() >= 2 && tpdu[0] & 0xC0 == 0x40;
        let matches = u16::from_be_bytes([tpdu[0] & 0x03, tpdu[1]]) & 0x3FF;
        (numbered && (matches == apci || matches & 0x3C0 == apci & 0x3C0 && apci < 0x2C0))
            .then(|| tpdu[1..].to_vec())
    })
}

#[tokio::test]
async fn connection_oriented_management_session_over_a_tunnel() {
    let device = device_object::new_device_object([0x00, 0xFA, 0xDE, 0xD0, 0x00, 0x01], [0x00; 6]);
    let mut bau = Bau::new(device, 2, 2);
    device_object::set_individual_address(bau.device_mut(), DEVICE);

    let server = DeviceServer::start_at("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = server.local_addr();
    let (stop, device) = run_device(server, bau);
    let mut master = Master {
        client: RawClient::connect(addr).await,
        tunnel_seq: 0,
        data_seq: 0,
    };

    // T_Connect.
    master
        .send_tpdu(&encode_control(TpduType::Connect, 0))
        .await;
    master.collect().await;

    // A_DeviceDescriptor_Read, type 0: the mask version.
    master.send_apdu(&[0x03, 0x00]).await;
    let frames = master.collect().await;
    let descriptor = apdu_of(&frames, ApduType::DeviceDescriptorResponse)
        .unwrap_or_else(|| panic!("no descriptor response in {frames:02x?}"));
    assert_eq!(descriptor[1..], [0x57, 0xB0], "mask version 0x57B0");

    // A_Memory_Write of four octets, then A_Memory_Read of them.
    master
        .send_apdu(&[0x02, 0x84, 0x00, 0x10, 1, 2, 3, 4])
        .await;
    master.collect().await;
    master.send_apdu(&[0x02, 0x04, 0x00, 0x10]).await;
    let frames = master.collect().await;
    let read = apdu_of(&frames, ApduType::MemoryResponse)
        .unwrap_or_else(|| panic!("no memory response in {frames:02x?}"));
    assert_eq!(read[0] & 0x0F, 4, "number of octets");
    assert_eq!(read[1..], [0x00, 0x10, 1, 2, 3, 4], "address and data");

    // A read longer than a response can say is answered, with nothing in it.
    master.send_apdu(&[0x02, 0x10, 0x00, 0x10]).await;
    let frames = master.collect().await;
    let read = apdu_of(&frames, ApduType::MemoryResponse)
        .unwrap_or_else(|| panic!("no memory response in {frames:02x?}"));
    assert_eq!(read[0] & 0x0F, 0, "number of octets");

    // A_Restart, master reset with erase code 2 (factory reset), channel 0.
    master.send_apdu(&[0x03, 0x81, 0x02, 0x00]).await;
    let frames = master.collect().await;
    let reset = frames
        .iter()
        .find(|tpdu| tpdu.len() >= 5 && tpdu[0] & 0x03 == 0x03 && tpdu[1] == 0xA1)
        .unwrap_or_else(|| panic!("no A_Restart response in {frames:02x?}"));
    assert_eq!(reset[2], 0x00, "error code: reset accepted");

    let _ = stop.send(());
    let mut bau = device.await.unwrap();
    assert!(
        bau.take_restart_pending(),
        "the host must be asked to restart"
    );
}

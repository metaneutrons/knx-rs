// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Application-level group operations for KNX connections.
//!
//! The [`GroupOps`] extension trait adds high-level group communication
//! methods to any [`KnxConnection`]. Import it to use `group_write`,
//! `group_read`, and DPT-aware variants.
//!
//! ```rust,no_run
//! use knx_rs_ip::{KnxConnection, connect, parse_url};
//! use knx_rs_ip::ops::GroupOps;
//! use knx_rs_core::address::GroupAddress;
//! use knx_rs_core::dpt::{DptValue, DPT_SWITCH, DPT_VALUE_TEMP};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut conn = connect(parse_url("udp://192.168.1.50:3671")?).await?;
//! let ga = "1/0/1".parse()?;
//!
//! conn.group_write(ga, &[0x01]).await?;
//! conn.group_write_value(ga, DPT_SWITCH, &true.into()).await?;
//! conn.group_write_value(ga, DPT_VALUE_TEMP, &21.5.into()).await?;
//! conn.group_read(ga).await?;
//! # Ok(())
//! # }
//! ```

use knx_rs_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
use knx_rs_core::apdu::Apdu;
use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::dpt::{self, Dpt, DptValue};
use knx_rs_core::message::{ApduType, MessageCode};
use knx_rs_core::types::Priority;

use crate::error::Result;
use crate::{KnxConnection, KnxFuture};

/// TPCI bits for unnumbered (connectionless) group data.
const TPCI_DATA_GROUP: u8 = 0x00;

/// Extension trait for group-level KNX operations.
///
/// Provides high-level methods on top of any [`KnxConnection`].
/// All APDU encoding is handled internally.
pub trait GroupOps: KnxConnection {
    /// Write a raw value to a group address.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`](crate::KnxIpError) if the frame could not be sent.
    fn group_write(&self, ga: GroupAddress, data: &[u8]) -> KnxFuture<'_, Result<()>> {
        let frame = match build_group_write(ga, data) {
            Ok(frame) => frame,
            Err(err) => return Box::pin(core::future::ready(Err(err))),
        };
        self.send(frame)
    }

    /// Write a DPT-encoded [`DptValue`] to a group address.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`](crate::KnxIpError) if encoding fails or the frame could not be sent.
    fn group_write_value(
        &self,
        ga: GroupAddress,
        dpt: Dpt,
        value: &DptValue,
    ) -> KnxFuture<'_, Result<()>> {
        let encoded = match dpt::encode(dpt, value) {
            Ok(encoded) => encoded,
            Err(err) => return Box::pin(core::future::ready(Err(err.into()))),
        };
        let frame = match build_group_write(ga, &encoded) {
            Ok(frame) => frame,
            Err(err) => return Box::pin(core::future::ready(Err(err))),
        };
        self.send(frame)
    }

    /// Send a group read request.
    ///
    /// The response (if any) will arrive as a normal received frame.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`](crate::KnxIpError) if the frame could not be sent.
    fn group_read(&self, ga: GroupAddress) -> KnxFuture<'_, Result<()>> {
        let frame = match build_group_read(ga) {
            Ok(frame) => frame,
            Err(err) => return Box::pin(core::future::ready(Err(err))),
        };
        self.send(frame)
    }

    /// Send a group value response.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`](crate::KnxIpError) if the frame could not be sent.
    fn group_respond(&self, ga: GroupAddress, data: &[u8]) -> KnxFuture<'_, Result<()>> {
        let frame = match build_group_response(ga, data) {
            Ok(frame) => frame,
            Err(err) => return Box::pin(core::future::ready(Err(err))),
        };
        self.send(frame)
    }
}

// Blanket implementation for all KnxConnection types.
impl<T: KnxConnection> GroupOps for T {}

// ── Frame builders (internal) ─────────────────────────────────

/// Build an `L_Data.req` cEMI frame carrying a group-value APDU.
///
/// APDU encoding (short/long form, APCI bytes) is delegated to
/// [`knx_rs_core::apdu::Apdu`] so opcodes and the short-form rule live in one
/// place; the source address is left zero for the gateway to fill in.
fn build_group_frame(ga: GroupAddress, apdu_type: ApduType, data: &[u8]) -> Result<CemiFrame> {
    let apdu = Apdu {
        apdu_type,
        data: data.to_vec(),
    };
    let payload = apdu.to_bytes(TPCI_DATA_GROUP);
    Ok(CemiFrame::try_new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0x0000),
        DestinationAddress::Group(ga),
        Priority::Low,
        &payload,
    )?)
}

fn build_group_write(ga: GroupAddress, data: &[u8]) -> Result<CemiFrame> {
    build_group_frame(ga, ApduType::GroupValueWrite, data)
}

fn build_group_read(ga: GroupAddress) -> Result<CemiFrame> {
    build_group_frame(ga, ApduType::GroupValueRead, &[])
}

fn build_group_response(ga: GroupAddress, data: &[u8]) -> Result<CemiFrame> {
    build_group_frame(ga, ApduType::GroupValueResponse, data)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use knx_rs_core::address::GroupAddress;
    use knx_rs_core::dpt::{DPT_SWITCH, DPT_VALUE_TEMP};

    #[test]
    fn build_group_write_short() {
        let frame = build_group_write(GroupAddress::from_raw(0x0801), &[0x01]).unwrap();
        assert_eq!(frame.destination_address_raw(), 0x0801);
        let payload = frame.payload();
        assert_eq!(payload[0], 0x00); // TPCI
        assert_eq!(payload[1], 0x81); // GroupValueWrite | 0x01
    }

    #[test]
    fn build_group_write_long() {
        let data = [0x0C, 0x34]; // DPT9 temperature
        let frame = build_group_write(GroupAddress::from_raw(0x0801), &data).unwrap();
        let payload = frame.payload();
        assert_eq!(payload[0], 0x00);
        assert_eq!(payload[1], 0x80); // GroupValueWrite
        assert_eq!(&payload[2..], &[0x0C, 0x34]);
    }

    #[test]
    fn build_group_read_frame() {
        let frame = build_group_read(GroupAddress::from_raw(0x0801)).unwrap();
        let payload = frame.payload();
        assert_eq!(payload, &[0x00, 0x00]);
    }

    #[test]
    fn build_group_response_short() {
        let frame = build_group_response(GroupAddress::from_raw(0x0801), &[0x01]).unwrap();
        let payload = frame.payload();
        assert_eq!(payload[1], 0x41); // GroupValueResponse | 0x01
    }

    #[test]
    fn dpt_encoding_in_write() {
        let encoded = dpt::encode(DPT_SWITCH, &DptValue::Bool(true)).unwrap();
        let frame = build_group_write(GroupAddress::from_raw(0x0802), &encoded).unwrap();
        let payload = frame.payload();
        assert_eq!(payload[1], 0x81); // GroupValueWrite | 1

        let encoded = dpt::encode(DPT_VALUE_TEMP, &DptValue::Float(21.5)).unwrap();
        let frame = build_group_write(GroupAddress::from_raw(0x0801), &encoded).unwrap();
        assert_eq!(frame.payload().len(), 4); // TPCI + APCI + 2 bytes DPT9
    }
}

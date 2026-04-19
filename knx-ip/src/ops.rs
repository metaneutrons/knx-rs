// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! Application-level group operations for KNX connections.
//!
//! The [`GroupOps`] extension trait adds high-level group communication
//! methods to any [`KnxConnection`]. Import it to use `group_write`,
//! `group_read`, and DPT-aware variants.
//!
//! ```rust,no_run
//! use knx_ip::{KnxConnection, connect, parse_url};
//! use knx_ip::ops::GroupOps;
//! use knx_core::address::GroupAddress;
//! use knx_core::dpt::DPT_SWITCH;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut conn = connect(parse_url("udp://192.168.1.50:3671")?).await?;
//! let ga = "1/0/1".parse()?;
//!
//! conn.group_write(ga, &[0x01]).await?;
//! conn.group_write_dpt(ga, DPT_SWITCH, 1.0).await?;
//! conn.group_read(ga).await?;
//! # Ok(())
//! # }
//! ```

use knx_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
use knx_core::cemi::CemiFrame;
use knx_core::dpt::{self, Dpt};
use knx_core::message::MessageCode;
use knx_core::types::Priority;

use crate::KnxConnection;
use crate::error::KnxIpError;

/// Extension trait for group-level KNX operations.
///
/// Provides high-level methods on top of any [`KnxConnection`].
/// All APDU encoding is handled internally.
#[allow(async_fn_in_trait)]
pub trait GroupOps: KnxConnection {
    /// Write a raw value to a group address.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the frame could not be sent.
    async fn group_write(&self, ga: GroupAddress, data: &[u8]) -> Result<(), KnxIpError> {
        let frame = build_group_write(ga, data);
        self.send(frame).await
    }

    /// Write a DPT-encoded value to a group address.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if encoding fails or the frame could not be sent.
    async fn group_write_dpt(
        &self,
        ga: GroupAddress,
        dpt: Dpt,
        value: f64,
    ) -> Result<(), KnxIpError> {
        let encoded = dpt::encode(dpt, value).map_err(|e| KnxIpError::Protocol(e.to_string()))?;
        self.group_write(ga, &encoded).await
    }

    /// Write a string value to a group address (DPT 16/28).
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if encoding fails or the frame could not be sent.
    async fn group_write_string(
        &self,
        ga: GroupAddress,
        dpt: Dpt,
        value: &str,
    ) -> Result<(), KnxIpError> {
        let encoded =
            dpt::encode_string(dpt, value).map_err(|e| KnxIpError::Protocol(e.to_string()))?;
        self.group_write(ga, &encoded).await
    }

    /// Send a group read request.
    ///
    /// The response (if any) will arrive as a normal received frame.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the frame could not be sent.
    async fn group_read(&self, ga: GroupAddress) -> Result<(), KnxIpError> {
        let frame = build_group_read(ga);
        self.send(frame).await
    }

    /// Send a group value response.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the frame could not be sent.
    async fn group_respond(&self, ga: GroupAddress, data: &[u8]) -> Result<(), KnxIpError> {
        let frame = build_group_response(ga, data);
        self.send(frame).await
    }
}

// Blanket implementation for all KnxConnection types.
impl<T: KnxConnection> GroupOps for T {}

// ── Frame builders (internal) ─────────────────────────────────

fn build_group_write(ga: GroupAddress, data: &[u8]) -> CemiFrame {
    let mut payload = Vec::with_capacity(2 + data.len());
    payload.push(0x00); // TPCI: unnumbered data
    if data.len() == 1 && data[0] <= 0x3F {
        payload.push(0x80 | (data[0] & 0x3F)); // short GroupValueWrite
    } else {
        payload.push(0x80); // GroupValueWrite APCI
        payload.extend_from_slice(data);
    }
    CemiFrame::new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0x0000), // filled by gateway
        DestinationAddress::Group(ga),
        Priority::Low,
        &payload,
    )
}

fn build_group_read(ga: GroupAddress) -> CemiFrame {
    CemiFrame::new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0x0000),
        DestinationAddress::Group(ga),
        Priority::Low,
        &[0x00, 0x00], // GroupValueRead
    )
}

fn build_group_response(ga: GroupAddress, data: &[u8]) -> CemiFrame {
    let mut payload = Vec::with_capacity(2 + data.len());
    payload.push(0x00);
    if data.len() == 1 && data[0] <= 0x3F {
        payload.push(0x40 | (data[0] & 0x3F)); // short GroupValueResponse
    } else {
        payload.push(0x40); // GroupValueResponse APCI
        payload.extend_from_slice(data);
    }
    CemiFrame::new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0x0000),
        DestinationAddress::Group(ga),
        Priority::Low,
        &payload,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use knx_core::address::GroupAddress;
    use knx_core::dpt::{DPT_SWITCH, DPT_VALUE_TEMP};

    #[test]
    fn build_group_write_short() {
        let frame = build_group_write(GroupAddress::from_raw(0x0801), &[0x01]);
        assert_eq!(frame.destination_address_raw(), 0x0801);
        let payload = frame.payload();
        assert_eq!(payload[0], 0x00); // TPCI
        assert_eq!(payload[1], 0x81); // GroupValueWrite | 0x01
    }

    #[test]
    fn build_group_write_long() {
        let data = [0x0C, 0x34]; // DPT9 temperature
        let frame = build_group_write(GroupAddress::from_raw(0x0801), &data);
        let payload = frame.payload();
        assert_eq!(payload[0], 0x00);
        assert_eq!(payload[1], 0x80); // GroupValueWrite
        assert_eq!(&payload[2..], &[0x0C, 0x34]);
    }

    #[test]
    fn build_group_read_frame() {
        let frame = build_group_read(GroupAddress::from_raw(0x0801));
        let payload = frame.payload();
        assert_eq!(payload, &[0x00, 0x00]);
    }

    #[test]
    fn build_group_response_short() {
        let frame = build_group_response(GroupAddress::from_raw(0x0801), &[0x01]);
        let payload = frame.payload();
        assert_eq!(payload[1], 0x41); // GroupValueResponse | 0x01
    }

    #[test]
    fn dpt_encoding_in_write() {
        let encoded = dpt::encode(DPT_SWITCH, 1.0).unwrap();
        let frame = build_group_write(GroupAddress::from_raw(0x0802), &encoded);
        let payload = frame.payload();
        assert_eq!(payload[1], 0x81); // GroupValueWrite | 1

        let encoded = dpt::encode(DPT_VALUE_TEMP, 21.5).unwrap();
        let frame = build_group_write(GroupAddress::from_raw(0x0801), &encoded);
        assert_eq!(frame.payload().len(), 4); // TPCI + APCI + 2 bytes DPT9
    }
}

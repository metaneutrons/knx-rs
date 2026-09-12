// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Application-level group operations for KNX connections.
//!
//! The [`GroupOps`] extension trait adds high-level group communication
//! methods to any [`KnxConnection`]. Import it to use `group_write_raw`,
//! `group_read`, and DPT-aware variants.
//!
//! # Migration from 0.9.x
//!
//! `group_write` and `group_respond` were removed for the 0.10.0 API because
//! they inferred the KNX wire representation from the encoded numeric value.
//! Use [`GroupOps::group_write_raw`] or [`GroupOps::group_respond_raw`] with
//! an explicit [`GroupValuePayload`], or prefer the DPT-aware
//! [`GroupOps::group_write_value`] and [`GroupOps::group_respond_value`]
//! methods.
//!
//! ```rust,no_run
//! use knx_rs_ip::{KnxConnection, connect, parse_url};
//! use knx_rs_ip::ops::{GroupOps, GroupValuePayload};
//! use knx_rs_core::address::GroupAddress;
//! use knx_rs_core::dpt::{DPT_SWITCH, DPT_VALUE_TEMP};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut conn = connect(parse_url("udp://192.168.1.50:3671")?).await?;
//! let ga = "1/0/1".parse()?;
//!
//! conn.group_write_raw(ga, GroupValuePayload::Inline(0x01)).await?;
//! conn.group_write_value(ga, DPT_SWITCH, &true.into()).await?;
//! conn.group_write_value(ga, DPT_VALUE_TEMP, &21.5.into()).await?;
//! conn.group_read(ga).await?;
//! # Ok(())
//! # }
//! ```

use knx_rs_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
use knx_rs_core::apdu::GroupValueApdu;
use knx_rs_core::cemi::CemiFrame;
use knx_rs_core::dpt::{self, Dpt, DptValue};
use knx_rs_core::message::MessageCode;
use knx_rs_core::types::Priority;

pub use knx_rs_core::apdu::GroupValuePayload;

use crate::error::Result;
use crate::{KnxConnection, KnxFuture};

/// TPCI bits for unnumbered (connectionless) group data.
const TPCI_DATA_GROUP: u8 = 0x00;

/// Extension trait for group-level KNX operations.
///
/// Provides high-level methods on top of any [`KnxConnection`].
/// All APDU encoding is handled internally.
pub trait GroupOps: KnxConnection {
    /// Write a raw group value with an explicit wire representation.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`](crate::KnxIpError) if the payload is invalid or
    /// the frame could not be sent.
    fn group_write_raw(
        &self,
        ga: GroupAddress,
        payload: GroupValuePayload<'_>,
    ) -> KnxFuture<'_, Result<()>> {
        let frame = match build_group_value_frame(ga, GroupValueApdu::Write(payload)) {
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
        let frame = match build_dpt_group_frame(ga, GroupValueService::Write, dpt, value) {
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

    /// Respond with a raw group value using an explicit wire representation.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`](crate::KnxIpError) if the payload is invalid or
    /// the frame could not be sent.
    fn group_respond_raw(
        &self,
        ga: GroupAddress,
        payload: GroupValuePayload<'_>,
    ) -> KnxFuture<'_, Result<()>> {
        let frame = match build_group_value_frame(ga, GroupValueApdu::Response(payload)) {
            Ok(frame) => frame,
            Err(err) => return Box::pin(core::future::ready(Err(err))),
        };
        self.send(frame)
    }

    /// Encode and respond with a DPT-aware group value.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`](crate::KnxIpError) if encoding fails or the frame could not be sent.
    fn group_respond_value(
        &self,
        ga: GroupAddress,
        dpt: Dpt,
        value: &DptValue,
    ) -> KnxFuture<'_, Result<()>> {
        let frame = match build_dpt_group_frame(ga, GroupValueService::Response, dpt, value) {
            Ok(frame) => frame,
            Err(err) => return Box::pin(core::future::ready(Err(err))),
        };
        self.send(frame)
    }
}

// Blanket implementation for all KnxConnection types.
impl<T: KnxConnection> GroupOps for T {}

// ── Frame builders (internal) ─────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupValueService {
    Response,
    Write,
}

impl GroupValueService {
    const fn with_payload(self, payload: GroupValuePayload<'_>) -> GroupValueApdu<'_> {
        match self {
            Self::Response => GroupValueApdu::Response(payload),
            Self::Write => GroupValueApdu::Write(payload),
        }
    }
}

fn build_cemi_group_frame(ga: GroupAddress, payload: &[u8]) -> Result<CemiFrame> {
    Ok(CemiFrame::try_new_l_data(
        MessageCode::LDataReq,
        IndividualAddress::from_raw(0x0000),
        DestinationAddress::Group(ga),
        Priority::Low,
        payload,
    )?)
}

fn build_group_value_frame(ga: GroupAddress, apdu: GroupValueApdu<'_>) -> Result<CemiFrame> {
    let payload = apdu.try_to_bytes(TPCI_DATA_GROUP)?;
    build_cemi_group_frame(ga, &payload)
}

fn build_dpt_group_frame(
    ga: GroupAddress,
    service: GroupValueService,
    dpt: Dpt,
    value: &DptValue,
) -> Result<CemiFrame> {
    let encoded = dpt::encode(dpt, value)?;
    let payload = GroupValuePayload::from_dpt_encoded(dpt, &encoded)?;
    build_group_value_frame(ga, service.with_payload(payload))
}

fn build_group_read(ga: GroupAddress) -> Result<CemiFrame> {
    build_group_value_frame(ga, GroupValueApdu::Read)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use knx_rs_core::address::GroupAddress;
    use knx_rs_core::dpt::{DPT_SCALING, DPT_SCENE_NUMBER, DPT_SWITCH, DPT_VALUE_TEMP};

    const TEST_GROUP: GroupAddress = GroupAddress::from_raw(0x0801);

    fn assert_dpt_write(dpt: Dpt, value: &DptValue, expected: &[u8]) {
        let frame =
            build_dpt_group_frame(TEST_GROUP, GroupValueService::Write, dpt, value).unwrap();
        assert_eq!(frame.destination_address_raw(), TEST_GROUP.raw());
        assert_eq!(frame.payload(), expected);
        assert_eq!(usize::from(frame.npdu_length()) + 1, expected.len());
    }

    #[test]
    fn explicit_raw_payload_controls_the_wire_form() {
        let inline = build_group_value_frame(
            TEST_GROUP,
            GroupValueApdu::Write(GroupValuePayload::Inline(1)),
        )
        .unwrap();
        assert_eq!(inline.payload(), &[0x00, 0x81]);
        assert_eq!(inline.npdu_length(), 1);

        let bytes = build_group_value_frame(
            TEST_GROUP,
            GroupValueApdu::Write(GroupValuePayload::Bytes(&[1])),
        )
        .unwrap();
        assert_eq!(bytes.payload(), &[0x00, 0x80, 0x01]);
        assert_eq!(bytes.npdu_length(), 2);
    }

    #[test]
    fn sub_octet_dpts_use_the_inline_apci_field() {
        assert_dpt_write(DPT_SWITCH, &DptValue::Bool(true), &[0x00, 0x81]);
        assert_dpt_write(Dpt::new(2, 1), &DptValue::UInt(3), &[0x00, 0x83]);
        assert_dpt_write(Dpt::new(3, 7), &DptValue::UInt(15), &[0x00, 0x8F]);
    }

    #[test]
    fn byte_sized_low_values_remain_separate_octets() {
        assert_dpt_write(DPT_SCALING, &DptValue::Float(0.0), &[0x00, 0x80, 0x00]);
        assert_dpt_write(DPT_SCALING, &DptValue::Float(30.0), &[0x00, 0x80, 0x4D]);
        assert_dpt_write(DPT_SCENE_NUMBER, &DptValue::UInt(1), &[0x00, 0x80, 0x01]);
    }

    #[test]
    fn multi_octet_dpt_remains_separate() {
        let frame = build_dpt_group_frame(
            TEST_GROUP,
            GroupValueService::Write,
            DPT_VALUE_TEMP,
            &DptValue::Float(21.5),
        )
        .unwrap();
        assert_eq!(&frame.payload()[..2], &[0x00, 0x80]);
        assert_eq!(frame.payload().len(), 4);
        assert_eq!(frame.npdu_length(), 3);
    }

    #[test]
    fn dpt_response_uses_the_same_wire_size_pipeline() {
        let inline = build_dpt_group_frame(
            TEST_GROUP,
            GroupValueService::Response,
            Dpt::new(3, 7),
            &DptValue::UInt(15),
        )
        .unwrap();
        assert_eq!(inline.payload(), &[0x00, 0x4F]);

        let octets = build_dpt_group_frame(
            TEST_GROUP,
            GroupValueService::Response,
            DPT_SCALING,
            &DptValue::Float(30.0),
        )
        .unwrap();
        assert_eq!(octets.payload(), &[0x00, 0x40, 0x4D]);
    }

    #[test]
    fn group_read_has_no_payload_form_to_misconfigure() {
        let frame = build_group_read(TEST_GROUP).unwrap();
        assert_eq!(frame.payload(), &[0x00, 0x00]);
    }
}

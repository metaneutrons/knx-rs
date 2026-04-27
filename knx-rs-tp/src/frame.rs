// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! TP bus frame encoding and decoding.
//!
//! Handles conversion between CEMI frames and TP wire format,
//! including CRC calculation.

use knx_rs_core::address::IndividualAddress;
use knx_rs_core::cemi::CemiFrame;

/// A TP bus frame (raw bytes on the twisted-pair wire).
///
/// Standard frame: ctrl(1) + src(2) + dst(2) + len(1) + apdu(n) + crc(1)
/// Extended frame: ctrl(1) + ctrl2(1) + src(2) + dst(2) + len(1) + apdu(n) + crc(1)
#[derive(Debug)]
pub struct TpFrame {
    data: [u8; 64],
    len: usize,
}

impl TpFrame {
    /// Create from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 7 || data.len() > 64 {
            return None;
        }
        let mut frame = Self {
            data: [0u8; 64],
            len: data.len(),
        };
        frame.data[..data.len()].copy_from_slice(data);
        Some(frame)
    }

    /// The raw frame bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data[..self.len]
    }

    /// Frame length.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the frame is empty.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether this is an extended frame.
    pub const fn is_extended(&self) -> bool {
        (self.data[0] & super::commands::L_DATA_MASK) == super::commands::L_DATA_EXTENDED_IND
    }

    /// Source address.
    pub const fn source(&self) -> IndividualAddress {
        let raw = if self.is_extended() {
            u16::from_be_bytes([self.data[2], self.data[3]])
        } else {
            u16::from_be_bytes([self.data[1], self.data[2]])
        };
        IndividualAddress::from_raw(raw)
    }

    /// Destination address (raw u16 — could be individual or group).
    pub const fn destination_raw(&self) -> u16 {
        if self.is_extended() {
            u16::from_be_bytes([self.data[4], self.data[5]])
        } else {
            u16::from_be_bytes([self.data[3], self.data[4]])
        }
    }

    /// APDU size (payload length).
    pub const fn apdu_size(&self) -> u8 {
        if self.is_extended() {
            self.data[6]
        } else {
            self.data[5] & 0x0F
        }
    }

    /// Calculate CRC-8 (XOR of all bytes, inverted).
    pub fn calc_crc(&self) -> u8 {
        CemiFrame::calc_crc_tp(&self.data[..self.len - 1])
    }

    /// Verify the CRC.
    pub fn is_valid(&self) -> bool {
        self.len >= 7 && self.data[self.len - 1] == self.calc_crc()
    }

    /// Convert a CEMI frame to a TP wire frame.
    ///
    /// Produces a standard frame (if APDU ≤ 15 bytes) or extended frame.
    pub fn from_cemi(cemi: &CemiFrame) -> Self {
        let payload = cemi.payload();
        let npdu_len = cemi.npdu_length();

        let mut frame = Self {
            data: [0u8; 64],
            len: 0,
        };

        if npdu_len <= 15 {
            // Standard frame
            let ctrl = cemi.as_bytes()[2]; // ctrl1 from CEMI
            frame.data[0] = ctrl;
            // Source address
            let src = cemi.source_address().to_bytes();
            frame.data[1] = src[0];
            frame.data[2] = src[1];
            // Destination address
            let dst_raw = cemi.destination_address_raw().to_be_bytes();
            frame.data[3] = dst_raw[0];
            frame.data[4] = dst_raw[1];
            // AT + hop count + length
            let ctrl2 = cemi.as_bytes()[3]; // ctrl2 from CEMI
            frame.data[5] = (ctrl2 & 0xF0) | (npdu_len & 0x0F);
            // APDU
            let apdu_len = payload.len();
            frame.data[6..6 + apdu_len].copy_from_slice(payload);
            frame.len = 6 + apdu_len + 1; // +1 for CRC
        } else {
            // Extended frame
            frame.data[0] = cemi.as_bytes()[2]; // ctrl1
            frame.data[1] = cemi.as_bytes()[3]; // ctrl2
            let src = cemi.source_address().to_bytes();
            frame.data[2] = src[0];
            frame.data[3] = src[1];
            let dst_raw = cemi.destination_address_raw().to_be_bytes();
            frame.data[4] = dst_raw[0];
            frame.data[5] = dst_raw[1];
            frame.data[6] = npdu_len;
            let apdu_len = payload.len();
            frame.data[7..7 + apdu_len].copy_from_slice(payload);
            frame.len = 7 + apdu_len + 1;
        }

        // Calculate and append CRC
        let crc = CemiFrame::calc_crc_tp(&frame.data[..frame.len - 1]);
        frame.data[frame.len - 1] = crc;

        frame
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use knx_rs_core::address::{DestinationAddress, GroupAddress, IndividualAddress};
    use knx_rs_core::message::MessageCode;
    use knx_rs_core::types::Priority;

    #[test]
    fn cemi_to_tp_standard_frame() {
        let cemi = CemiFrame::new_l_data(
            MessageCode::LDataInd,
            IndividualAddress::from_raw(0x1101),
            DestinationAddress::Group(GroupAddress::from_raw(0x0801)),
            Priority::Low,
            &[0x00, 0x81],
        );
        let tp = TpFrame::from_cemi(&cemi);
        assert!(!tp.is_extended());
        assert_eq!(tp.source().raw(), 0x1101);
        assert_eq!(tp.destination_raw(), 0x0801);
        assert_eq!(tp.apdu_size(), 1);
        assert!(tp.is_valid());
    }

    #[test]
    fn tp_frame_from_bytes() {
        // Standard frame: ctrl + src(2) + dst(2) + at_len + apdu(2) + crc
        let mut data = [0xBC, 0x11, 0x01, 0x08, 0x01, 0xE1, 0x00, 0x81, 0x00];
        data[8] = CemiFrame::calc_crc_tp(&data[..8]);
        let frame = TpFrame::from_bytes(&data).unwrap();
        assert!(!frame.is_extended());
        assert_eq!(frame.source().raw(), 0x1101);
        assert_eq!(frame.destination_raw(), 0x0801);
        assert!(frame.is_valid());
    }

    #[test]
    fn crc_validation() {
        let data = [0xBC, 0x11, 0x01, 0x08, 0x01, 0xE1, 0x00, 0x81, 0xFF]; // bad CRC
        let frame = TpFrame::from_bytes(&data).unwrap();
        assert!(!frame.is_valid());
    }

    #[test]
    fn too_short_rejected() {
        assert!(TpFrame::from_bytes(&[0x00; 3]).is_none());
    }
}

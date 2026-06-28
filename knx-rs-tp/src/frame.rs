// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! TP bus frame encoding and decoding.
//!
//! Handles conversion between CEMI frames and TP wire format,
//! including CRC calculation.

use knx_rs_core::address::IndividualAddress;
use knx_rs_core::cemi::CemiFrame;

use crate::commands::{L_DATA_EXTENDED_IND, L_DATA_MASK};

/// Maximum TP frame length the on-chip receive buffer can hold (bytes).
pub const MAX_FRAME_LEN: usize = 64;
/// Standard-frame header length: ctrl(1) + src(2) + dst(2) + len(1).
pub const HEADER_STD: usize = 6;
/// Extended-frame header length: ctrl(1) + ctrl2(1) + src(2) + dst(2) + len(1).
pub const HEADER_EXT: usize = 7;
/// Trailing CRC-8 byte length.
pub const CRC_LEN: usize = 1;
/// Mask for the APDU-length nibble in a standard frame's length octet.
pub const APDU_LEN_MASK: u8 = 0x0F;
/// Control-field frame-type bit (set = standard, clear = extended).
pub const CTRL_FRAME_TYPE: u8 = 0x80;

/// Whether a control-field byte encodes an extended frame.
#[must_use]
pub const fn is_extended_ctrl(ctrl: u8) -> bool {
    (ctrl & L_DATA_MASK) == L_DATA_EXTENDED_IND
}

/// A TP bus frame (raw bytes on the twisted-pair wire).
///
/// Standard frame: ctrl(1) + src(2) + dst(2) + len(1) + apdu(n) + crc(1)
/// Extended frame: ctrl(1) + ctrl2(1) + src(2) + dst(2) + len(1) + apdu(n) + crc(1)
#[derive(Debug)]
pub struct TpFrame {
    data: [u8; MAX_FRAME_LEN],
    len: usize,
}

impl TpFrame {
    /// Create from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_STD + CRC_LEN || data.len() > MAX_FRAME_LEN {
            return None;
        }
        let mut frame = Self {
            data: [0u8; MAX_FRAME_LEN],
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
        is_extended_ctrl(self.data[0])
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
            self.data[5] & APDU_LEN_MASK
        }
    }

    /// Calculate CRC-8 (XOR of all bytes, inverted).
    ///
    /// Returns `0` for an empty frame; well-formed frames always have
    /// `len >= HEADER_STD + CRC_LEN`, so this guard is purely defensive.
    pub fn calc_crc(&self) -> u8 {
        if self.len == 0 {
            return 0;
        }
        CemiFrame::calc_crc_tp(&self.data[..self.len - 1])
    }

    /// Verify the CRC.
    pub fn is_valid(&self) -> bool {
        self.len >= 7 && self.data[self.len - 1] == self.calc_crc()
    }

    /// Convert a CEMI frame to a TP wire frame.
    ///
    /// Produces a standard frame (if APDU ≤ 15 bytes) or an extended frame.
    ///
    /// Returns `None` if the APDU does not fit in [`MAX_FRAME_LEN`] (header +
    /// APDU + CRC), since the on-wire frame must fit the receive buffer.
    pub fn from_cemi(cemi: &CemiFrame) -> Option<Self> {
        let payload = cemi.payload();
        let npdu_len = cemi.npdu_length();
        let apdu_len = payload.len();

        let mut frame = Self {
            data: [0u8; MAX_FRAME_LEN],
            len: 0,
        };

        let src = cemi.source_address().to_bytes();
        let dst_raw = cemi.destination_address_raw().to_be_bytes();
        let ctrl1 = cemi.as_bytes()[2];
        let ctrl2 = cemi.as_bytes()[3];

        if npdu_len <= APDU_LEN_MASK {
            // Standard frame. The cEMI ctrl1 already has the frame-type bit set.
            if HEADER_STD + apdu_len + CRC_LEN > MAX_FRAME_LEN {
                return None;
            }
            frame.data[0] = ctrl1 | CTRL_FRAME_TYPE;
            frame.data[1] = src[0];
            frame.data[2] = src[1];
            frame.data[3] = dst_raw[0];
            frame.data[4] = dst_raw[1];
            // AT + hop count + length nibble
            frame.data[5] = (ctrl2 & 0xF0) | (npdu_len & APDU_LEN_MASK);
            frame.data[HEADER_STD..HEADER_STD + apdu_len].copy_from_slice(payload);
            frame.len = HEADER_STD + apdu_len + CRC_LEN;
        } else {
            // Extended frame: clear the frame-type bit so ctrl1 encodes EXTENDED
            // on the wire (otherwise is_extended() misreads it as standard).
            if HEADER_EXT + apdu_len + CRC_LEN > MAX_FRAME_LEN {
                return None;
            }
            frame.data[0] = ctrl1 & !CTRL_FRAME_TYPE;
            frame.data[1] = ctrl2;
            frame.data[2] = src[0];
            frame.data[3] = src[1];
            frame.data[4] = dst_raw[0];
            frame.data[5] = dst_raw[1];
            frame.data[6] = npdu_len;
            frame.data[HEADER_EXT..HEADER_EXT + apdu_len].copy_from_slice(payload);
            frame.len = HEADER_EXT + apdu_len + CRC_LEN;
        }

        // Calculate and append CRC.
        let crc = CemiFrame::calc_crc_tp(&frame.data[..frame.len - 1]);
        frame.data[frame.len - 1] = crc;

        Some(frame)
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
        let tp = TpFrame::from_cemi(&cemi).unwrap();
        assert!(!tp.is_extended());
        assert_eq!(tp.source().raw(), 0x1101);
        assert_eq!(tp.destination_raw(), 0x0801);
        assert_eq!(tp.apdu_size(), 1);
        assert!(tp.is_valid());
    }

    #[test]
    fn cemi_to_tp_extended_frame_roundtrips() {
        // APDU > 15 bytes forces an extended frame.
        let mut apdu = [0u8; 20];
        apdu[0] = 0x00;
        apdu[1] = 0x80; // GroupValueWrite
        let cemi = CemiFrame::new_l_data(
            MessageCode::LDataInd,
            IndividualAddress::from_raw(0x1101),
            DestinationAddress::Group(GroupAddress::from_raw(0x0801)),
            Priority::Low,
            &apdu,
        );
        let tp = TpFrame::from_cemi(&cemi).unwrap();
        // The frame-type bit must be cleared so it reads back as extended.
        assert!(tp.is_extended());
        assert_eq!(tp.source().raw(), 0x1101);
        assert_eq!(tp.destination_raw(), 0x0801);
        // The length octet excludes the TPCI byte (payload.len() - 1).
        assert_eq!(tp.apdu_size() as usize, apdu.len() - 1);
        assert!(tp.is_valid());
    }

    #[test]
    fn from_cemi_rejects_oversized_apdu() {
        // An APDU that cannot fit MAX_FRAME_LEN must return None, not panic.
        let apdu = [0xAAu8; 80];
        let cemi = CemiFrame::new_l_data(
            MessageCode::LDataInd,
            IndividualAddress::from_raw(0x1101),
            DestinationAddress::Group(GroupAddress::from_raw(0x0801)),
            Priority::Low,
            &apdu,
        );
        assert!(TpFrame::from_cemi(&cemi).is_none());
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

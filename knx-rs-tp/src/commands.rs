// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! TP-UART command and indication byte constants.

/// Reset the TP-UART chip.
pub const U_RESET_REQ: u8 = 0x01;
/// Request chip state.
pub const U_STATE_REQ: u8 = 0x02;
/// Enter bus monitor mode.
pub const U_BUSMON_REQ: u8 = 0x05;
/// System state request (NCN5120).
pub const U_SYSTEM_STATE_REQ: u8 = 0x0D;
/// Enter stop mode.
pub const U_STOP_MODE_REQ: u8 = 0x0E;
/// Exit stop mode.
pub const U_EXIT_STOP_MODE_REQ: u8 = 0x0F;
/// ACK request base.
pub const U_ACK_REQ: u8 = 0x10;
/// ACK flag: addressed.
pub const ACK_ADDRESSED: u8 = 0x01;
/// ACK flag: busy.
pub const ACK_BUSY: u8 = 0x02;
/// ACK flag: nack.
pub const ACK_NACK: u8 = 0x04;
/// Start of frame transmission.
pub const U_L_DATA_START_REQ: u8 = 0x80;
/// Continuation of frame transmission.
pub const U_L_DATA_CONT_REQ: u8 = 0x80;
/// End of frame transmission.
pub const U_L_DATA_END_REQ: u8 = 0x40;
/// NCN5120: set individual address.
pub const U_NCN5120_SET_ADDRESS_REQ: u8 = 0xF1;
/// TP-UART 2: set individual address.
pub const U_TPUART2_SET_ADDRESS_REQ: u8 = 0x28;
/// Reset indication.
pub const U_RESET_IND: u8 = 0x03;
/// State indication.
pub const U_STATE_IND: u8 = 0x07;
/// State mask.
pub const U_STATE_MASK: u8 = 0x07;
/// Frame state indication.
pub const U_FRAME_STATE_IND: u8 = 0x13;
/// Frame state mask.
pub const U_FRAME_STATE_MASK: u8 = 0x17;
/// Standard frame indication.
pub const L_DATA_STANDARD_IND: u8 = 0x90;
/// Extended frame indication.
pub const L_DATA_EXTENDED_IND: u8 = 0x10;
/// Frame type mask.
pub const L_DATA_MASK: u8 = 0xD3;

/// Supported transceiver types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BcuType {
    /// Siemens TP-UART 2.
    TpUart2,
    /// `NCN5120` / `NCN5121` / `NCN5130`.
    Ncn5120,
}

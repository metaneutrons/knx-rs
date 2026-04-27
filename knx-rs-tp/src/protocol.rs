// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! TP-UART host-side protocol.

use crate::commands::{
    self, BcuType, U_ACK_REQ, U_L_DATA_CONT_REQ, U_L_DATA_END_REQ, U_L_DATA_START_REQ, U_RESET_IND,
    U_RESET_REQ, U_STATE_REQ,
};
use crate::frame::TpFrame;

/// Trait for UART byte-level I/O.
pub trait UartInterface {
    /// Write a single byte.
    fn write_byte(&mut self, byte: u8);
    /// Read a single byte, if available.
    fn read_byte(&mut self) -> Option<u8>;
    /// Check if data is available.
    fn available(&self) -> bool;
}

/// Received indication from the chip.
#[derive(Debug)]
pub enum TpIndication {
    /// Chip was reset.
    Reset,
    /// Chip state byte.
    State(u8),
    /// Complete TP frame received.
    Frame(TpFrame),
    /// Transmit confirmation (true = success).
    TransmitConfirm(bool),
}

/// TP-UART protocol handler.
pub struct TpUartProtocol {
    bcu_type: BcuType,
    rx_buf: [u8; 64],
    rx_pos: usize,
    rx_expected: usize,
}

impl TpUartProtocol {
    /// Create a new protocol handler.
    pub const fn new(bcu_type: BcuType) -> Self {
        Self {
            bcu_type,
            rx_buf: [0u8; 64],
            rx_pos: 0,
            rx_expected: 0,
        }
    }

    /// The configured BCU type.
    pub const fn bcu_type(&self) -> BcuType {
        self.bcu_type
    }

    /// Send a reset request.
    pub fn reset(uart: &mut impl UartInterface) {
        uart.write_byte(U_RESET_REQ);
    }

    /// Send a state request.
    pub fn request_state(uart: &mut impl UartInterface) {
        uart.write_byte(U_STATE_REQ);
    }

    /// Set the individual address on the chip.
    pub fn set_address(&self, uart: &mut impl UartInterface, address: u16) {
        let cmd = match self.bcu_type {
            BcuType::Ncn5120 => commands::U_NCN5120_SET_ADDRESS_REQ,
            BcuType::TpUart2 => commands::U_TPUART2_SET_ADDRESS_REQ,
        };
        uart.write_byte(cmd);
        uart.write_byte((address >> 8) as u8);
        uart.write_byte((address & 0xFF) as u8);
    }

    /// Send an ACK for the current frame.
    pub fn send_ack(uart: &mut impl UartInterface, addressed: bool, busy: bool, nack: bool) {
        let mut ack = U_ACK_REQ;
        if addressed {
            ack |= commands::ACK_ADDRESSED;
        }
        if busy {
            ack |= commands::ACK_BUSY;
        }
        if nack {
            ack |= commands::ACK_NACK;
        }
        uart.write_byte(ack);
    }

    /// Transmit a TP frame to the bus.
    pub fn transmit(uart: &mut impl UartInterface, frame: &TpFrame) {
        let data = frame.as_bytes();
        let last = data.len() - 1;
        for (i, &byte) in data.iter().enumerate() {
            #[expect(clippy::cast_possible_truncation)]
            let idx = i as u8 & 0x3F;
            let cmd = if i == 0 {
                U_L_DATA_START_REQ | idx
            } else if i == last {
                U_L_DATA_END_REQ | idx
            } else {
                U_L_DATA_CONT_REQ | idx
            };
            uart.write_byte(cmd);
            uart.write_byte(byte);
        }
    }

    /// Process incoming bytes. Call in your main loop.
    pub fn process(&mut self, uart: &mut impl UartInterface) -> Option<TpIndication> {
        let byte = uart.read_byte()?;

        if self.rx_pos == 0 {
            if byte == U_RESET_IND {
                return Some(TpIndication::Reset);
            }
            if byte & commands::U_STATE_MASK == commands::U_STATE_IND {
                return Some(TpIndication::State(byte));
            }
            if byte & commands::U_FRAME_STATE_MASK == commands::U_FRAME_STATE_IND {
                return Some(TpIndication::TransmitConfirm(byte & 0x80 == 0));
            }
            if is_frame_start(byte) {
                self.rx_buf[0] = byte;
                self.rx_pos = 1;
                self.rx_expected = 0;
                return None;
            }
            return None;
        }

        if self.rx_pos < self.rx_buf.len() {
            self.rx_buf[self.rx_pos] = byte;
            self.rx_pos += 1;
        }

        if self.rx_expected == 0 && self.rx_pos >= 7 {
            let is_ext = (self.rx_buf[0] & commands::L_DATA_MASK) == commands::L_DATA_EXTENDED_IND;
            let apdu_len = if is_ext {
                self.rx_buf[6] as usize
            } else {
                (self.rx_buf[5] & 0x0F) as usize
            };
            self.rx_expected = if is_ext { 8 } else { 7 } + apdu_len;
        }

        if self.rx_expected > 0 && self.rx_pos >= self.rx_expected {
            let frame = TpFrame::from_bytes(&self.rx_buf[..self.rx_pos]);
            self.rx_pos = 0;
            self.rx_expected = 0;
            return frame.map(TpIndication::Frame);
        }

        None
    }
}

const fn is_frame_start(byte: u8) -> bool {
    let masked = byte & commands::L_DATA_MASK;
    masked == commands::L_DATA_STANDARD_IND || masked == commands::L_DATA_EXTENDED_IND
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    extern crate alloc;
    use alloc::vec::Vec;

    struct MockUart {
        tx: Vec<u8>,
        rx: Vec<u8>,
        rx_pos: usize,
    }

    impl MockUart {
        fn new(rx_data: &[u8]) -> Self {
            Self {
                tx: Vec::new(),
                rx: rx_data.to_vec(),
                rx_pos: 0,
            }
        }
    }

    impl UartInterface for MockUart {
        fn write_byte(&mut self, byte: u8) {
            self.tx.push(byte);
        }
        fn read_byte(&mut self) -> Option<u8> {
            if self.rx_pos < self.rx.len() {
                let b = self.rx[self.rx_pos];
                self.rx_pos += 1;
                Some(b)
            } else {
                None
            }
        }
        fn available(&self) -> bool {
            self.rx_pos < self.rx.len()
        }
    }

    #[test]
    fn reset_sends_correct_byte() {
        let mut uart = MockUart::new(&[]);
        TpUartProtocol::reset(&mut uart);
        assert_eq!(uart.tx, &[U_RESET_REQ]);
    }

    #[test]
    fn process_reset_indication() {
        let mut proto = TpUartProtocol::new(BcuType::Ncn5120);
        let mut uart = MockUart::new(&[U_RESET_IND]);
        assert!(matches!(
            proto.process(&mut uart),
            Some(TpIndication::Reset)
        ));
    }

    #[test]
    fn process_state_indication() {
        let mut proto = TpUartProtocol::new(BcuType::Ncn5120);
        let mut uart = MockUart::new(&[0x07]);
        assert!(matches!(
            proto.process(&mut uart),
            Some(TpIndication::State(0x07))
        ));
    }

    #[test]
    fn process_transmit_confirm() {
        let mut proto = TpUartProtocol::new(BcuType::Ncn5120);
        let mut uart = MockUart::new(&[0x13]);
        assert!(matches!(
            proto.process(&mut uart),
            Some(TpIndication::TransmitConfirm(true))
        ));
    }

    #[test]
    fn set_address_ncn5120() {
        let proto = TpUartProtocol::new(BcuType::Ncn5120);
        let mut uart = MockUart::new(&[]);
        proto.set_address(&mut uart, 0x1101);
        assert_eq!(uart.tx, &[commands::U_NCN5120_SET_ADDRESS_REQ, 0x11, 0x01]);
    }

    #[test]
    fn set_address_tpuart2() {
        let proto = TpUartProtocol::new(BcuType::TpUart2);
        let mut uart = MockUart::new(&[]);
        proto.set_address(&mut uart, 0x1101);
        assert_eq!(uart.tx, &[commands::U_TPUART2_SET_ADDRESS_REQ, 0x11, 0x01]);
    }

    #[test]
    fn transmit_frame() {
        let mut uart = MockUart::new(&[]);
        let data = [0xBC, 0x11, 0x01, 0x08, 0x01, 0xE1, 0x00, 0x81];
        let crc = knx_rs_core::cemi::CemiFrame::calc_crc_tp(&data);
        let mut frame_data = [0u8; 9];
        frame_data[..8].copy_from_slice(&data);
        frame_data[8] = crc;
        let frame = TpFrame::from_bytes(&frame_data).unwrap();
        TpUartProtocol::transmit(&mut uart, &frame);
        assert_eq!(uart.tx.len(), 18);
        assert_eq!(uart.tx[0], U_L_DATA_START_REQ);
        assert_eq!(uart.tx[1], 0xBC);
        assert_eq!(uart.tx[16], U_L_DATA_END_REQ | 8);
        assert_eq!(uart.tx[17], crc);
    }
}

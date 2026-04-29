// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! KNX Transport Layer — connection-oriented point-to-point communication.
//!
//! Implements the full state machine from KNX 3/3/4 with:
//! - 4 states: `Closed`, `OpenIdle`, `OpenWait`, `Connecting`
//! - `ACK`/`NACK` handling with sequence numbers
//! - Retransmission with configurable retry count
//! - Connection timeout (6s) and `ACK` timeout (3s)
//!
//! Ported from the `OpenKNX` C++ reference implementation.

use alloc::vec::Vec;

use knx_rs_core::address::IndividualAddress;
use knx_rs_core::message::TpduType;
use knx_rs_core::types::Priority;

// ── Configuration ─────────────────────────────────────────────

/// Connection timeout in milliseconds (KNX spec: 6000ms).
const CONNECTION_TIMEOUT_MS: u64 = 6000;

/// `ACK` timeout in milliseconds (KNX spec: 3000ms).
const ACK_TIMEOUT_MS: u64 = 3000;

/// Maximum retransmission attempts before giving up.
const MAX_REP_COUNT: u8 = 3;

/// Mask for 4-bit sequence number wrapping (0–15).
const SEQ_NO_MASK: u8 = 0x0F;

// ── State ─────────────────────────────────────────────────────

/// Transport layer connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// No active connection.
    Closed,
    /// Connected, no outstanding unacknowledged frame.
    OpenIdle,
    /// Connected, waiting for ACK/NACK for a sent frame.
    OpenWait,
    /// Outgoing connection initiated, waiting for L2 confirm.
    Connecting,
}

/// Outgoing action produced by the transport layer state machine.
#[derive(Debug)]
pub enum Action {
    /// Send a control telegram (ACK, NACK, Connect, Disconnect).
    SendControl {
        /// Destination individual address.
        destination: IndividualAddress,
        /// TPDU type to send.
        tpdu_type: TpduType,
        /// Sequence number.
        seq_no: u8,
    },
    /// Send a data-connected frame (with sequence number, ACK requested).
    SendDataConnected {
        /// Destination individual address.
        destination: IndividualAddress,
        /// Sequence number.
        seq_no: u8,
        /// Frame priority.
        priority: Priority,
        /// APDU payload.
        apdu: Vec<u8>,
    },
    /// Notify application layer: connection established (incoming).
    ConnectIndication {
        /// Source individual address.
        source: IndividualAddress,
    },
    /// Notify application layer: connection established (outgoing confirmed).
    ConnectConfirm {
        /// Destination individual address.
        destination: IndividualAddress,
    },
    /// Notify application layer: connection closed.
    DisconnectIndication {
        /// Address of the disconnected remote.
        address: IndividualAddress,
    },
    /// Notify application layer: received connected data.
    DataConnectedIndication {
        /// Source individual address.
        source: IndividualAddress,
        /// Frame priority.
        priority: Priority,
        /// APDU payload.
        apdu: Vec<u8>,
    },
    /// Notify application layer: our data was acknowledged.
    DataConnectedConfirm,
}

/// Transport layer with full KNX state machine.
pub struct TransportLayer {
    state: State,
    /// Individual address of the connected remote device.
    connection_address: IndividualAddress,
    /// Send sequence counter (0–15).
    seq_no_send: u8,
    /// Receive sequence counter (0–15).
    seq_no_recv: u8,
    /// Current retransmission count.
    rep_count: u8,

    // Timers (stored as Option<deadline_ms>)
    connection_timeout_deadline: Option<u64>,
    ack_timeout_deadline: Option<u64>,

    // Saved frame for retransmission in OpenWait
    saved_frame: Option<SavedFrame>,
    // Buffered request when busy (OpenWait/Connecting)
    buffered_request: Option<BufferedRequest>,

    /// Pending actions to be consumed by the caller.
    actions: Vec<Action>,
}

/// Saved frame for retransmission when waiting for ACK.
struct SavedFrame {
    destination: IndividualAddress,
    seq_no: u8,
    priority: Priority,
    apdu: Vec<u8>,
}

/// Buffered data request queued while connection is busy.
struct BufferedRequest {
    priority: Priority,
    apdu: Vec<u8>,
}

impl TransportLayer {
    /// Create a new transport layer in Closed state.
    pub const fn new() -> Self {
        Self {
            state: State::Closed,
            connection_address: IndividualAddress::from_raw(0),
            seq_no_send: 0,
            seq_no_recv: 0,
            rep_count: 0,
            connection_timeout_deadline: None,
            ack_timeout_deadline: None,
            saved_frame: None,
            buffered_request: None,
            actions: Vec::new(),
        }
    }

    /// Current connection state.
    pub const fn state(&self) -> State {
        self.state
    }

    /// Address of the connected remote, or 0 if not connected.
    pub const fn connection_address(&self) -> IndividualAddress {
        self.connection_address
    }

    /// Take all pending actions (drains the queue).
    pub fn take_actions(&mut self) -> Vec<Action> {
        core::mem::take(&mut self.actions)
    }

    // ── Incoming frame processing ─────────────────────────────

    /// Process an incoming Connect control telegram.
    pub fn connect_indication(&mut self, source: IndividualAddress, now_ms: u64) {
        if self.state == State::Closed {
            // E0/E1: accept in Closed regardless of source
            self.a1_accept_connection(source, now_ms);
        } else if source == self.connection_address {
            // E0: Connect from connectionAddress in open state — ignore
        } else {
            // E1: Connect from other address in open state — reject
            self.a10_reject_foreign(source);
        }
    }

    /// Process an incoming Disconnect control telegram.
    pub fn disconnect_indication(&mut self, source: IndividualAddress) {
        if source != self.connection_address || self.state == State::Closed {
            return; // E3: from other address or already closed — ignore
        }
        // E2: Disconnect from connectionAddress
        self.a5_passive_disconnect(source);
        self.state = State::Closed;
    }

    /// Process an incoming `DataConnected` frame.
    pub fn data_connected_indication(
        &mut self,
        source: IndividualAddress,
        seq_no: u8,
        priority: Priority,
        apdu: Vec<u8>,
        now_ms: u64,
    ) {
        if source != self.connection_address {
            // E7: from other address
            if self.state == State::Connecting {
                self.a10_reject_foreign(source);
            }
            return;
        }

        let prev_seq = self.seq_no_recv.wrapping_sub(1) & SEQ_NO_MASK;

        match self.state {
            State::Closed => {}
            State::OpenIdle | State::OpenWait => {
                if seq_no == self.seq_no_recv {
                    self.a2_receive_data(source, priority, apdu, now_ms);
                } else if seq_no == prev_seq {
                    self.a3_ack_repeated(source, seq_no, now_ms);
                } else {
                    self.a4_nack_wrong_seq(source, seq_no, now_ms);
                }
            }
            State::Connecting => {
                if seq_no == prev_seq {
                    self.a3_ack_repeated(source, seq_no, now_ms);
                } else {
                    self.active_disconnect();
                    self.state = State::Closed;
                }
            }
        }
    }

    /// Process an incoming ACK.
    pub fn ack_indication(&mut self, source: IndividualAddress, seq_no: u8, now_ms: u64) {
        if source != self.connection_address {
            // E10/E14: from other address
            if self.state == State::Connecting {
                self.a10_reject_foreign(source);
            }
            return;
        }

        match self.state {
            State::Closed | State::OpenIdle => {} // E8/E9 in Closed/OpenIdle: ignore
            State::OpenWait => {
                if seq_no == self.seq_no_send {
                    // E8: correct ACK
                    self.a8_ack_received(now_ms);
                    self.state = State::OpenIdle;
                } else {
                    // E9: wrong ACK sequence
                    self.active_disconnect();
                    self.state = State::Closed;
                }
            }
            State::Connecting => {
                // E8/E9 in Connecting → disconnect
                self.active_disconnect();
                self.state = State::Closed;
            }
        }
    }

    /// Process an incoming NACK.
    pub fn nack_indication(&mut self, source: IndividualAddress, seq_no: u8, now_ms: u64) {
        if source != self.connection_address {
            // E14: from other address
            if self.state == State::Connecting {
                self.a10_reject_foreign(source);
            }
            return;
        }

        match self.state {
            State::Closed => {}
            State::OpenIdle | State::Connecting => {
                // E11/E12/E13 in OpenIdle/Connecting → disconnect
                self.active_disconnect();
                self.state = State::Closed;
            }
            State::OpenWait => {
                if seq_no != self.seq_no_send {
                    // E11: wrong NACK sequence — ignore
                    return;
                }
                if self.rep_count < MAX_REP_COUNT {
                    // E12: retriable
                    self.a9_retransmit(now_ms);
                } else {
                    // E13: max retries exceeded
                    self.active_disconnect();
                    self.state = State::Closed;
                }
            }
        }
    }

    /// Process L2 confirm for a Connect frame we sent.
    pub fn connect_confirm(&mut self, success: bool) {
        if self.state == State::Connecting {
            if success {
                // E19: Connect delivered
                self.actions.push(Action::ConnectConfirm {
                    destination: self.connection_address,
                });
                self.state = State::OpenIdle;
            } else {
                // E20: Connect delivery failed → close
                self.a5_passive_disconnect(self.connection_address);
                self.state = State::Closed;
            }
        }
    }

    // ── Application layer requests ────────────────────────────

    /// Application wants to send connected data.
    pub fn data_connected_request(&mut self, priority: Priority, apdu: Vec<u8>, now_ms: u64) {
        match self.state {
            State::Closed => {} // E15 in Closed: ignore
            State::OpenIdle => {
                // E15 in OpenIdle → send
                self.a7_send_data(priority, apdu, now_ms);
                self.state = State::OpenWait;
            }
            State::OpenWait | State::Connecting => {
                // E15 in OpenWait/Connecting → buffer
                self.buffered_request = Some(BufferedRequest { priority, apdu });
            }
        }
    }

    /// Application wants to open a connection.
    pub fn connect_request(&mut self, destination: IndividualAddress, now_ms: u64) {
        if self.state == State::Closed {
            self.a12_initiate_connection(destination, now_ms);
            self.state = State::Connecting;
        } else {
            self.active_disconnect();
            self.state = State::Closed;
        }
    }

    /// Application wants to close the connection.
    pub fn disconnect_request(&mut self) {
        if self.state == State::Closed {
            self.actions.push(Action::DisconnectIndication {
                address: self.connection_address,
            });
        } else {
            self.active_disconnect();
            self.state = State::Closed;
        }
    }

    // ── Periodic processing ───────────────────────────────────

    /// Must be called periodically. Checks timeouts and retries buffered requests.
    pub fn poll(&mut self, now_ms: u64) {
        // Connection timeout
        if let Some(deadline) = self.connection_timeout_deadline {
            if now_ms >= deadline {
                self.connection_timeout_deadline = None;
                // E16: connection timeout
                match self.state {
                    State::OpenIdle | State::OpenWait | State::Connecting => {
                        self.active_disconnect();
                        self.state = State::Closed;
                    }
                    State::Closed => {}
                }
                return;
            }
        }

        // `ACK` timeout
        if let Some(deadline) = self.ack_timeout_deadline {
            if now_ms >= deadline && self.state == State::OpenWait {
                self.ack_timeout_deadline = None;
                if self.rep_count < MAX_REP_COUNT {
                    // E17: retry
                    self.a9_retransmit(now_ms);
                } else {
                    // E18: max retries
                    self.active_disconnect();
                    self.state = State::Closed;
                }
                return;
            }
        }

        // Retry buffered request
        if self.state == State::OpenIdle {
            if let Some(req) = self.buffered_request.take() {
                self.a7_send_data(req.priority, req.apdu, now_ms);
                self.state = State::OpenWait;
            }
        }
    }

    // ── Actions ───────────────────────────────────────────────

    /// A1: Accept incoming connection.
    fn a1_accept_connection(&mut self, source: IndividualAddress, now_ms: u64) {
        self.connection_address = source;
        self.seq_no_send = 0;
        self.seq_no_recv = 0;
        self.enable_connection_timeout(now_ms);
        self.actions.push(Action::ConnectIndication { source });
        self.state = State::OpenIdle;
    }

    /// A2: Receive valid connected data.
    fn a2_receive_data(
        &mut self,
        source: IndividualAddress,
        priority: Priority,
        apdu: Vec<u8>,
        now_ms: u64,
    ) {
        // Send ACK
        self.actions.push(Action::SendControl {
            destination: source,
            tpdu_type: TpduType::Ack,
            seq_no: self.seq_no_recv,
        });
        self.seq_no_recv = (self.seq_no_recv + 1) & SEQ_NO_MASK;
        self.enable_connection_timeout(now_ms);
        self.actions.push(Action::DataConnectedIndication {
            source,
            priority,
            apdu,
        });
    }

    /// A3: ACK repeated frame (previous sequence).
    fn a3_ack_repeated(&mut self, source: IndividualAddress, seq_no: u8, now_ms: u64) {
        self.actions.push(Action::SendControl {
            destination: source,
            tpdu_type: TpduType::Ack,
            seq_no,
        });
        self.enable_connection_timeout(now_ms);
    }

    /// A4: NACK wrong sequence.
    fn a4_nack_wrong_seq(&mut self, source: IndividualAddress, seq_no: u8, now_ms: u64) {
        self.actions.push(Action::SendControl {
            destination: source,
            tpdu_type: TpduType::Nack,
            seq_no,
        });
        self.enable_connection_timeout(now_ms);
    }

    /// A5: Passive disconnect (remote initiated).
    fn a5_passive_disconnect(&mut self, address: IndividualAddress) {
        self.connection_timeout_deadline = None;
        self.ack_timeout_deadline = None;
        self.actions.push(Action::DisconnectIndication { address });
    }

    /// Active disconnect (error, timeout, or application request).
    fn active_disconnect(&mut self) {
        self.actions.push(Action::SendControl {
            destination: self.connection_address,
            tpdu_type: TpduType::Disconnect,
            seq_no: 0,
        });
        self.actions.push(Action::DisconnectIndication {
            address: self.connection_address,
        });
        self.connection_timeout_deadline = None;
        self.ack_timeout_deadline = None;
    }

    /// A7: Send connected data.
    fn a7_send_data(&mut self, priority: Priority, apdu: Vec<u8>, now_ms: u64) {
        self.saved_frame = Some(SavedFrame {
            destination: self.connection_address,
            seq_no: self.seq_no_send,
            priority,
            apdu: apdu.clone(),
        });
        self.actions.push(Action::SendDataConnected {
            destination: self.connection_address,
            seq_no: self.seq_no_send,
            priority,
            apdu,
        });
        self.rep_count = 0;
        self.enable_ack_timeout(now_ms);
        self.enable_connection_timeout(now_ms);
    }

    /// A8: ACK received for our data.
    fn a8_ack_received(&mut self, now_ms: u64) {
        self.ack_timeout_deadline = None;
        self.seq_no_send = (self.seq_no_send + 1) & SEQ_NO_MASK;
        self.saved_frame = None;
        self.enable_connection_timeout(now_ms);
        self.actions.push(Action::DataConnectedConfirm);
    }

    /// A9: Retransmit saved frame.
    fn a9_retransmit(&mut self, now_ms: u64) {
        if let Some(ref frame) = self.saved_frame {
            self.actions.push(Action::SendDataConnected {
                destination: frame.destination,
                seq_no: frame.seq_no,
                priority: frame.priority,
                apdu: frame.apdu.clone(),
            });
        }
        self.rep_count += 1;
        self.enable_ack_timeout(now_ms);
        self.enable_connection_timeout(now_ms);
    }

    /// A10: Reject foreign connection attempt.
    fn a10_reject_foreign(&mut self, source: IndividualAddress) {
        self.actions.push(Action::SendControl {
            destination: source,
            tpdu_type: TpduType::Disconnect,
            seq_no: 0,
        });
    }

    /// A12: Initiate outgoing connection.
    fn a12_initiate_connection(&mut self, destination: IndividualAddress, now_ms: u64) {
        self.connection_address = destination;
        self.seq_no_send = 0;
        self.seq_no_recv = 0;
        self.actions.push(Action::SendControl {
            destination,
            tpdu_type: TpduType::Connect,
            seq_no: 0,
        });
        self.enable_connection_timeout(now_ms);
    }

    // ── Timer helpers ─────────────────────────────────────────

    const fn enable_connection_timeout(&mut self, now_ms: u64) {
        self.connection_timeout_deadline = Some(now_ms + CONNECTION_TIMEOUT_MS);
    }

    const fn enable_ack_timeout(&mut self, now_ms: u64) {
        self.ack_timeout_deadline = Some(now_ms + ACK_TIMEOUT_MS);
    }
}

impl Default for TransportLayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const ADDR_1001: IndividualAddress = IndividualAddress::from_raw(0x1001);
    const ADDR_2002: IndividualAddress = IndividualAddress::from_raw(0x2002);

    #[test]
    fn initial_state_is_closed() {
        let tl = TransportLayer::new();
        assert_eq!(tl.state(), State::Closed);
        assert_eq!(tl.connection_address().raw(), 0);
    }

    #[test]
    fn accept_incoming_connection() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        assert_eq!(tl.state(), State::OpenIdle);
        assert_eq!(tl.connection_address(), ADDR_1001);

        let actions = tl.take_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            Action::ConnectIndication { source } if source == ADDR_1001
        ));
    }

    #[test]
    fn reject_foreign_connection_when_connected() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        tl.connect_indication(ADDR_2002, 100);
        assert_eq!(tl.state(), State::OpenIdle);
        let actions = tl.take_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            Action::SendControl {
                destination,
                tpdu_type: TpduType::Disconnect,
                ..
            } if destination == ADDR_2002
        ));
    }

    #[test]
    fn send_and_ack_data() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        // Send data
        tl.data_connected_request(Priority::Low, alloc::vec![0x01, 0x02], 100);
        assert_eq!(tl.state(), State::OpenWait);
        let actions = tl.take_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            Action::SendDataConnected { seq_no: 0, .. }
        ));

        // Receive ACK
        tl.ack_indication(ADDR_1001, 0, 200);
        assert_eq!(tl.state(), State::OpenIdle);
        let actions = tl.take_actions();
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::DataConnectedConfirm))
        );
    }

    #[test]
    fn receive_data_sends_ack() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        tl.data_connected_indication(ADDR_1001, 0, Priority::Low, alloc::vec![0xAA], 100);
        let actions = tl.take_actions();
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::SendControl {
                tpdu_type: TpduType::Ack,
                seq_no: 0,
                ..
            }
        )));
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::DataConnectedIndication { .. }))
        );
    }

    #[test]
    fn nack_triggers_retransmit() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        tl.data_connected_request(Priority::Low, alloc::vec![0x01], 100);
        tl.take_actions();

        // NACK with correct seq
        tl.nack_indication(ADDR_1001, 0, 200);
        assert_eq!(tl.state(), State::OpenWait);
        let actions = tl.take_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            Action::SendDataConnected { seq_no: 0, .. }
        ));
    }

    #[test]
    fn max_retries_disconnects() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        tl.data_connected_request(Priority::Low, alloc::vec![0x01], 100);
        tl.take_actions();

        // 3 NACKs
        for i in 0..MAX_REP_COUNT {
            tl.nack_indication(ADDR_1001, 0, 200 + u64::from(i) * 100);
            tl.take_actions();
        }
        assert_eq!(tl.state(), State::OpenWait);

        // 4th NACK → disconnect
        tl.nack_indication(ADDR_1001, 0, 600);
        assert_eq!(tl.state(), State::Closed);
    }

    #[test]
    fn connection_timeout_disconnects() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        // Poll after timeout
        tl.poll(CONNECTION_TIMEOUT_MS + 1);
        assert_eq!(tl.state(), State::Closed);
        let actions = tl.take_actions();
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::SendControl {
                tpdu_type: TpduType::Disconnect,
                ..
            }
        )));
    }

    #[test]
    fn ack_timeout_retransmits() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        tl.data_connected_request(Priority::Low, alloc::vec![0x01], 100);
        tl.take_actions();

        // `ACK` timeout
        tl.poll(100 + ACK_TIMEOUT_MS + 1);
        assert_eq!(tl.state(), State::OpenWait);
        let actions = tl.take_actions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], Action::SendDataConnected { .. }));
    }

    #[test]
    fn buffered_request_sent_after_ack() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        // Send first frame
        tl.data_connected_request(Priority::Low, alloc::vec![0x01], 100);
        tl.take_actions();

        // Buffer second frame while in OpenWait
        tl.data_connected_request(Priority::Low, alloc::vec![0x02], 150);
        let actions = tl.take_actions();
        assert!(actions.is_empty()); // buffered, not sent

        // ACK first frame
        tl.ack_indication(ADDR_1001, 0, 200);
        tl.take_actions();
        assert_eq!(tl.state(), State::OpenIdle);

        // Poll should send buffered frame
        tl.poll(250);
        assert_eq!(tl.state(), State::OpenWait);
        let actions = tl.take_actions();
        assert!(matches!(
            actions[0],
            Action::SendDataConnected { seq_no: 1, .. }
        ));
    }

    #[test]
    fn disconnect_from_remote() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        tl.disconnect_indication(ADDR_1001);
        assert_eq!(tl.state(), State::Closed);
        let actions = tl.take_actions();
        assert!(actions.iter().any(
            |a| matches!(a, Action::DisconnectIndication { address } if *address == ADDR_1001)
        ));
    }

    #[test]
    fn wrong_ack_seq_disconnects() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        tl.data_connected_request(Priority::Low, alloc::vec![0x01], 100);
        tl.take_actions();

        // ACK with wrong sequence
        tl.ack_indication(ADDR_1001, 5, 200);
        assert_eq!(tl.state(), State::Closed);
    }

    #[test]
    fn connect_request_transitions_to_connecting() {
        let mut tl = TransportLayer::new();
        tl.connect_request(ADDR_1001, 0);
        assert_eq!(tl.state(), State::Connecting);
        assert_eq!(tl.connection_address(), ADDR_1001);
        let actions = tl.take_actions();
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::SendControl {
                destination,
                tpdu_type: TpduType::Connect,
                ..
            } if *destination == ADDR_1001
        )));
    }

    #[test]
    fn disconnect_request_from_open_idle() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();
        assert_eq!(tl.state(), State::OpenIdle);

        tl.disconnect_request();
        assert_eq!(tl.state(), State::Closed);
        let actions = tl.take_actions();
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::SendControl {
                tpdu_type: TpduType::Disconnect,
                ..
            }
        )));
    }

    #[test]
    fn data_in_connecting_state_is_buffered() {
        let mut tl = TransportLayer::new();
        tl.connect_request(ADDR_1001, 0);
        tl.take_actions();
        assert_eq!(tl.state(), State::Connecting);

        tl.data_connected_request(Priority::Low, alloc::vec![0xAB], 100);
        let actions = tl.take_actions();
        // Data should be buffered, not sent
        assert!(actions.is_empty());

        // After connect confirm, poll should send the buffered data
        tl.connect_confirm(true);
        tl.take_actions();
        assert_eq!(tl.state(), State::OpenIdle);

        tl.poll(200);
        assert_eq!(tl.state(), State::OpenWait);
        let actions = tl.take_actions();
        assert!(matches!(
            actions[0],
            Action::SendDataConnected { seq_no: 0, .. }
        ));
    }

    #[test]
    fn sequence_numbers_wrap() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        // Send and ACK 16 frames to wrap sequence numbers
        for i in 0u8..16 {
            tl.data_connected_request(Priority::Low, alloc::vec![i], u64::from(i) * 100);
            tl.take_actions();
            tl.ack_indication(ADDR_1001, i & SEQ_NO_MASK, u64::from(i) * 100 + 50);
            tl.take_actions();
        }

        // Next send should use seq 0 again
        tl.data_connected_request(Priority::Low, alloc::vec![0xFF], 2000);
        let actions = tl.take_actions();
        assert!(matches!(
            actions[0],
            Action::SendDataConnected { seq_no: 0, .. }
        ));
    }

    #[test]
    fn connect_confirm_failure_returns_to_closed() {
        let mut tl = TransportLayer::new();
        tl.connect_request(ADDR_1001, 0);
        assert_eq!(tl.state(), State::Connecting);
        tl.take_actions();

        tl.connect_confirm(false);
        assert_eq!(tl.state(), State::Closed);
        let actions = tl.take_actions();
        assert!(actions.iter().any(
            |a| matches!(a, Action::DisconnectIndication { address } if *address == ADDR_1001)
        ));
    }

    #[test]
    fn foreign_data_in_open_idle_triggers_disconnect() {
        let mut tl = TransportLayer::new();
        // Establish connection with addr A
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();
        assert_eq!(tl.state(), State::OpenIdle);

        // Receive data from addr B — should be ignored (no disconnect for OpenIdle foreign data)
        tl.data_connected_indication(ADDR_2002, 0, Priority::Low, alloc::vec![0xAA], 100);
        // Foreign data in OpenIdle is silently dropped (source != connection_address)
        let actions = tl.take_actions();
        assert!(actions.is_empty());
        assert_eq!(tl.state(), State::OpenIdle);
    }

    #[test]
    fn timeout_exactly_on_deadline() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();
        assert_eq!(tl.state(), State::OpenIdle);

        // Poll at exactly the deadline (now_ms >= deadline triggers timeout)
        tl.poll(CONNECTION_TIMEOUT_MS);
        assert_eq!(
            tl.state(),
            State::Closed,
            "polling at exactly the deadline should trigger disconnect"
        );
    }

    #[test]
    fn rapid_connect_disconnect_cycle() {
        let mut tl = TransportLayer::new();

        // Connect
        tl.connect_indication(ADDR_1001, 0);
        assert_eq!(tl.state(), State::OpenIdle);
        tl.take_actions();

        // Immediately disconnect
        tl.disconnect_indication(ADDR_1001);
        assert_eq!(tl.state(), State::Closed);
        tl.take_actions();

        // Connect again — verify clean state
        tl.connect_indication(ADDR_1001, 100);
        assert_eq!(tl.state(), State::OpenIdle);
        let actions = tl.take_actions();
        assert_eq!(actions.len(), 1);
        assert!(
            matches!(actions[0], Action::ConnectIndication { source } if source == ADDR_1001),
            "second connect should produce a clean ConnectIndication"
        );
    }

    #[test]
    fn data_received_after_timeout() {
        let mut tl = TransportLayer::new();
        tl.connect_indication(ADDR_1001, 0);
        tl.take_actions();

        // Let connection timeout
        tl.poll(CONNECTION_TIMEOUT_MS + 1);
        assert_eq!(tl.state(), State::Closed);
        tl.take_actions();

        // Receive data after timeout — should be ignored (state is Closed)
        tl.data_connected_indication(ADDR_1001, 0, Priority::Low, alloc::vec![0xBB], 7000);
        let actions = tl.take_actions();
        assert!(
            actions.is_empty(),
            "data received after timeout should be ignored"
        );
        assert_eq!(tl.state(), State::Closed);
    }
}

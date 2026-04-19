// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! KNX Transport Layer — connection management and data routing.
//!
//! Handles connection-oriented (point-to-point) and connectionless
//! (group/broadcast) communication. Manages sequence numbers for
//! connected-mode transport.

/// Transport layer connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionState {
    /// No active connection.
    #[default]
    Closed,
    /// Connection open to a remote device.
    Open {
        /// Remote individual address.
        remote: u16,
        /// Send sequence counter.
        send_seq: u8,
        /// Receive sequence counter.
        recv_seq: u8,
    },
}

/// Transport layer state.
pub struct TransportLayer {
    connection: ConnectionState,
}

impl Default for TransportLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportLayer {
    /// Create a new transport layer.
    pub const fn new() -> Self {
        Self {
            connection: ConnectionState::Closed,
        }
    }

    /// Current connection state.
    pub const fn connection(&self) -> ConnectionState {
        self.connection
    }

    /// Open a connection to a remote device (ETS connecting to us).
    pub const fn connect(&mut self, remote: u16) {
        self.connection = ConnectionState::Open {
            remote,
            send_seq: 0,
            recv_seq: 0,
        };
    }

    /// Close the current connection.
    pub const fn disconnect(&mut self) {
        self.connection = ConnectionState::Closed;
    }

    /// Check if a connection is open to the given remote address.
    pub const fn is_connected_to(&self, remote: u16) -> bool {
        matches!(self.connection, ConnectionState::Open { remote: r, .. } if r == remote)
    }

    /// Increment the send sequence counter. Returns the current value before increment.
    #[expect(clippy::missing_const_for_fn)] // mutable match arms prevent const
    pub fn next_send_seq(&mut self) -> Option<u8> {
        if let ConnectionState::Open {
            ref mut send_seq, ..
        } = self.connection
        {
            let seq = *send_seq;
            *send_seq = (*send_seq + 1) & 0x0F;
            Some(seq)
        } else {
            None
        }
    }

    /// Check and increment the receive sequence counter.
    /// Returns `true` if the sequence number matches (valid).
    #[expect(clippy::missing_const_for_fn)] // mutable match arms prevent const
    pub fn check_recv_seq(&mut self, seq: u8) -> bool {
        if let ConnectionState::Open {
            ref mut recv_seq, ..
        } = self.connection
        {
            if seq == *recv_seq {
                *recv_seq = (*recv_seq + 1) & 0x0F;
                true
            } else {
                false
            }
        } else {
            false
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn default_closed() {
        let tl = TransportLayer::new();
        assert_eq!(tl.connection(), ConnectionState::Closed);
    }

    #[test]
    fn connect_disconnect() {
        let mut tl = TransportLayer::new();
        tl.connect(0x1101);
        assert!(tl.is_connected_to(0x1101));
        assert!(!tl.is_connected_to(0x1102));
        tl.disconnect();
        assert_eq!(tl.connection(), ConnectionState::Closed);
    }

    #[test]
    fn sequence_counting() {
        let mut tl = TransportLayer::new();
        tl.connect(0x1101);

        assert_eq!(tl.next_send_seq(), Some(0));
        assert_eq!(tl.next_send_seq(), Some(1));
        assert_eq!(tl.next_send_seq(), Some(2));

        assert!(tl.check_recv_seq(0));
        assert!(tl.check_recv_seq(1));
        assert!(!tl.check_recv_seq(5)); // out of sequence
    }

    #[test]
    fn sequence_wraps_at_16() {
        let mut tl = TransportLayer::new();
        tl.connect(0x1101);
        for i in 0..16 {
            assert_eq!(tl.next_send_seq(), Some(i));
        }
        assert_eq!(tl.next_send_seq(), Some(0)); // wrapped
    }

    #[test]
    fn no_sequence_when_closed() {
        let mut tl = TransportLayer::new();
        assert_eq!(tl.next_send_seq(), None);
        assert!(!tl.check_recv_seq(0));
    }
}

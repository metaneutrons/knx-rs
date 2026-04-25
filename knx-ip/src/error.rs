// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Error types for KNXnet/IP connections.

/// Errors that can occur during KNXnet/IP communication.
#[derive(Debug, thiserror::Error)]
pub enum KnxIpError {
    /// UDP socket I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The remote end did not respond in time.
    #[error("timeout waiting for {0}")]
    Timeout(&'static str),

    /// The remote end rejected the connection.
    #[error("connection rejected: status {0:#04x}")]
    ConnectionRejected(u8),

    /// Received a malformed KNXnet/IP frame.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// The connection was closed.
    #[error("connection closed")]
    Closed,

    /// Invalid URL or connection specification.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
}

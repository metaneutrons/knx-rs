// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Error types for KNXnet/IP connections.

use knx_rs_core::cemi::CemiError;
use knx_rs_core::dpt::DptError;
use knx_rs_core::knxip::KnxIpParseError;

/// Convenience alias for results from this crate.
pub type Result<T> = core::result::Result<T, KnxIpError>;

/// Errors that can occur during KNXnet/IP communication.
#[derive(Debug, thiserror::Error)]
pub enum KnxIpError {
    /// UDP socket I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse or serialize a KNXnet/IP frame.
    #[error("frame error: {0}")]
    Frame(#[from] KnxIpParseError),

    /// Failed to build or parse a cEMI frame.
    #[error("cEMI error: {0}")]
    Cemi(#[from] CemiError),

    /// Failed to encode or decode a datapoint value.
    #[error("DPT error: {0}")]
    Dpt(#[from] DptError),

    /// Failed to join a multicast group.
    #[error("join multicast {group}: {source}")]
    Multicast {
        /// The multicast group address.
        group: String,
        /// The underlying socket error.
        source: std::io::Error,
    },

    /// The connection specification or configuration is invalid.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    /// The remote end did not respond in time.
    #[error("timeout waiting for {0}")]
    Timeout(&'static str),

    /// The remote end rejected the connection.
    #[error("connection rejected: status {0:#04x}")]
    ConnectionRejected(u8),

    /// A protocol-level error that does not fit a more specific variant.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// The connection was closed.
    #[error("connection closed")]
    Closed,

    /// Invalid URL or connection specification.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
}

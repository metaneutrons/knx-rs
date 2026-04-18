// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025 Fabian Schmieder

//! `knx-tp` — KNX TP-UART data link layer for embedded targets.
//!
//! Supports Siemens TP-UART 2 and `NCN5120`/`NCN5121`/`NCN5130` transceivers.
//!
//! # Architecture
//!
//! The TP-UART chip handles physical bus timing and ACK generation.
//! This crate implements the host-side serial protocol:
//!
//! - [`TpUartProtocol`] — command/indication protocol over UART
//! - [`TpFrame`] — TP bus frame encoding/decoding with CRC-8
//! - [`UartInterface`] — trait for platform-specific UART access

#![no_std]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

pub mod commands;
pub mod frame;
pub mod protocol;

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Connection multiplexer — fan out a single connection into multiple handles.
//!
//! Each [`MultiplexHandle`] independently receives all incoming CEMI frames
//! and can send frames through the shared connection.

use knx_core::cemi::CemiFrame;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::KnxConnection;
use crate::error::KnxIpError;

/// Multiplexes a single [`KnxConnection`] into multiple independent handles.
///
/// Call [`handle()`](Multiplexer::handle) to create handles before calling
/// [`run()`](Multiplexer::run). The `run` method consumes the multiplexer
/// and drives the underlying connection.
pub struct Multiplexer<C: KnxConnection> {
    conn: C,
    broadcast_tx: broadcast::Sender<CemiFrame>,
    cmd_rx: mpsc::Receiver<MuxCmd>,
    cmd_tx: mpsc::Sender<MuxCmd>,
}

enum MuxCmd {
    Send(CemiFrame, oneshot::Sender<Result<(), KnxIpError>>),
}

impl<C: KnxConnection + 'static> Multiplexer<C> {
    /// Create a new multiplexer wrapping the given connection.
    ///
    /// The broadcast channel capacity determines how many frames can be
    /// buffered before slow handles start missing frames.
    pub fn new(conn: C) -> Self {
        let (broadcast_tx, _) = broadcast::channel(128);
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        Self {
            conn,
            broadcast_tx,
            cmd_rx,
            cmd_tx,
        }
    }

    /// Create a new [`MultiplexHandle`] for this connection.
    ///
    /// Each handle independently receives all incoming frames and can send.
    /// Call this before [`run()`](Multiplexer::run).
    pub fn handle(&self) -> MultiplexHandle {
        MultiplexHandle {
            rx: self.broadcast_tx.subscribe(),
            cmd_tx: self.cmd_tx.clone(),
        }
    }

    /// Run the multiplexer, driving the underlying connection.
    ///
    /// This consumes the multiplexer. It runs until the connection closes
    /// or all handles are dropped.
    pub async fn run(mut self) {
        loop {
            tokio::select! {
                frame = self.conn.recv() => {
                    if let Some(cemi) = frame {
                        // Broadcast to all handles (ignore if no receivers)
                        let _ = self.broadcast_tx.send(cemi);
                    } else {
                        tracing::debug!("multiplexer: connection closed");
                        break;
                    }
                }

                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(MuxCmd::Send(frame, reply)) => {
                            let result = self.conn.send(frame).await;
                            let _ = reply.send(result);
                        }
                        None => {
                            // All handles dropped
                            tracing::debug!("multiplexer: all handles dropped");
                            self.conn.close().await;
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// An independent handle to a multiplexed connection.
///
/// Each handle receives all incoming CEMI frames and can send frames
/// through the shared connection.
pub struct MultiplexHandle {
    rx: broadcast::Receiver<CemiFrame>,
    cmd_tx: mpsc::Sender<MuxCmd>,
}

impl MultiplexHandle {
    /// Send a CEMI frame through the shared connection.
    ///
    /// # Errors
    ///
    /// Returns [`KnxIpError`] if the send fails or the multiplexer is closed.
    pub async fn send_frame(&self, frame: CemiFrame) -> Result<(), KnxIpError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(MuxCmd::Send(frame, tx))
            .await
            .map_err(|_| KnxIpError::Closed)?;
        rx.await.map_err(|_| KnxIpError::Closed)?
    }

    /// Receive the next CEMI frame.
    ///
    /// Returns `None` if the multiplexer is closed.
    /// Frames that arrive while this handle is not awaiting `recv` may be
    /// dropped if the broadcast buffer overflows.
    pub async fn recv(&mut self) -> Option<CemiFrame> {
        loop {
            match self.rx.recv().await {
                Ok(frame) => return Some(frame),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "multiplex handle lagged");
                    // Loop back to try again
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

impl KnxConnection for MultiplexHandle {
    async fn send(&self, frame: CemiFrame) -> Result<(), KnxIpError> {
        self.send_frame(frame).await
    }

    async fn recv(&mut self) -> Option<CemiFrame> {
        self.recv().await
    }

    async fn close(&mut self) {
        // MultiplexHandle closes when dropped
    }
}

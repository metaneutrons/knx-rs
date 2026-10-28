// SPDX-License-Identifier: GPL-3.0-only

//! A knxd container as a third-party KNXnet/IP stack for interoperability
//! tests. See `tests/knxd/README.md`.
//!
//! The harness drives the plain `docker` CLI, so it follows whatever Docker
//! context the developer or CI runner uses and adds no dependency. Tests skip
//! (loudly) only when Docker itself is unavailable. A container that fails to
//! start, or an image that fails to build, fails the test.
//!
//! # Reaching knxd from the host
//!
//! UDP published with `-p` does not reach the host on every Docker setup (it
//! does not with colima's default SSH port forwarder). Instead of publishing,
//! the host talks to knxd through a [`Relay`]: `docker exec -i` runs `relay.py`
//! in the container's network namespace, and the harness carries datagrams
//! over the stdio pipes. The relay also records a wire trace and can withhold
//! acknowledgements, so a test can play a client that has gone silent.
//!
//! The other direction (knxd as a tunnelling client of a server on the host)
//! uses the container's route to the host: `host.docker.internal`.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use knx_rs_core::knxip::{ConnectionHeader, KnxIpFrame, ServiceType};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::Instant;

const CONTEXT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/knxd");
const READY_TIMEOUT: Duration = Duration::from_secs(15);

/// Set to anything but `0` to fail instead of skip when Docker is missing.
const REQUIRE_ENV: &str = "KNX_RS_REQUIRE_DOCKER";

static COUNTER: AtomicU32 = AtomicU32::new(0);
static IMAGE: OnceLock<Result<String, String>> = OnceLock::new();

fn docker(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .output()
}

fn text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// `Ok` when a Docker daemon for Linux containers answers; the error says why
/// not. (Windows runners ship a daemon for Windows containers, which cannot
/// run the knxd image.)
fn docker_available() -> Result<(), String> {
    match docker(&["version", "--format", "{{.Server.Os}}"]) {
        Ok(out) if out.status.success() => {
            let os = text(&out).trim().to_owned();
            if os == "linux" {
                Ok(())
            } else {
                Err(format!("the Docker server runs {os} containers, not linux"))
            }
        }
        Ok(out) => Err(text(&out).trim().to_owned()),
        Err(e) => Err(format!("cannot run the docker CLI: {e}")),
    }
}

/// Content hash of the build context, so a changed recipe gets a new image tag.
fn context_hash() -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for name in ["Dockerfile", "entrypoint.sh", "relay.py"] {
        let bytes = std::fs::read(format!("{CONTEXT_DIR}/{name}")).unwrap();
        for byte in name.bytes().chain(bytes) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

/// Build the image once per process; returns its tag.
fn image() -> Result<String, String> {
    IMAGE
        .get_or_init(|| {
            let tag = format!("knx-rs-knxd-test:{}", context_hash());
            let present = docker(&["image", "inspect", &tag]).is_ok_and(|o| o.status.success());
            if !present {
                let out = docker(&["build", "-t", &tag, CONTEXT_DIR])
                    .map_err(|e| format!("docker build: {e}"))?;
                if !out.status.success() {
                    return Err(format!("docker build failed:\n{}", text(&out)));
                }
            }
            Ok(tag)
        })
        .clone()
}

/// How knxd is configured.
pub enum Mode {
    /// knxd is a tunnelling server; the clients under test attach to it.
    Server,
    /// knxd is a tunnelling client of the server at this port on the host and
    /// also runs a server of its own for an observer client.
    Ipt { host_port: u16, heartbeat_secs: u32 },
}

/// A running knxd container, removed on drop.
pub struct Knxd {
    pub name: String,
    pub version: String,
}

impl Knxd {
    /// Start knxd. `None` (after printing why) only if Docker is unavailable.
    ///
    /// # Panics
    ///
    /// If the image cannot be built or the container does not become ready.
    pub async fn start(mode: Mode) -> Option<Self> {
        if let Err(why) = docker_available() {
            assert!(
                !std::env::var(REQUIRE_ENV).is_ok_and(|v| v != "0"),
                "{REQUIRE_ENV} is set but Docker is unavailable: {why}"
            );
            eprintln!("SKIP knxd interoperability test: Docker is unavailable ({why})");
            return None;
        }
        let tag = image().unwrap_or_else(|e| panic!("{e}"));
        let name = format!(
            "knx-rs-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let (ini, ipt) = match mode {
            Mode::Server => ("server.ini", None),
            Mode::Ipt {
                host_port,
                heartbeat_secs,
            } => ("ipt.ini", Some((host_port, heartbeat_secs))),
        };
        let ini_text = std::fs::read_to_string(format!("{CONTEXT_DIR}/{ini}")).unwrap();

        let mut args: Vec<String> = ["run", "-d", "--rm", "--name", &name]
            .map(String::from)
            .to_vec();
        args.extend(["--label".into(), "knx-rs-test=1".into()]);
        args.extend(["-e".into(), format!("KNXD_INI={ini_text}")]);
        if let Some((port, heartbeat)) = ipt {
            args.extend(["-e".into(), format!("KNX_PORT={port}")]);
            args.extend(["-e".into(), format!("KNX_HEARTBEAT={heartbeat}")]);
            args.extend(["-e".into(), "KNX_HOST=host.docker.internal".into()]);
            // Docker Desktop and colima define the name; plain Linux needs it.
            if cfg!(target_os = "linux") {
                args.extend([
                    "--add-host".into(),
                    "host.docker.internal:host-gateway".into(),
                ]);
            }
        }
        args.push(tag);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = docker(&refs).expect("docker run");
        assert!(out.status.success(), "docker run failed:\n{}", text(&out));

        let mut knxd = Self {
            name,
            version: String::new(),
        };
        knxd.wait_ready().await;
        knxd.exec_root_free(&["knxd", "--version"])
            .trim()
            .clone_into(&mut knxd.version);
        eprintln!("knxd container {} runs {}", knxd.name, knxd.version);
        Some(knxd)
    }

    async fn wait_for(&self, what: &str, mut probe: impl FnMut() -> bool) {
        let deadline = Instant::now() + READY_TIMEOUT;
        while !probe() {
            assert!(
                Instant::now() < deadline,
                "knxd not ready ({what}):\n{}",
                self.logs()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn wait_ready(&self) {
        // UDP 3671 (0x0E57) is bound once the tunnelling server is up.
        self.wait_for("udp 3671", || {
            self.exec_status(&["grep", "-qi", ":0E57 ", "/proc/net/udp"])
        })
        .await;
    }

    fn exec_status(&self, cmd: &[&str]) -> bool {
        let mut args = vec!["exec", &self.name];
        args.extend_from_slice(cmd);
        docker(&args).is_ok_and(|o| o.status.success())
    }

    /// Run a command in the container as the unprivileged user; stdout and
    /// stderr combined.
    pub fn exec_root_free(&self, cmd: &[&str]) -> String {
        let mut args = vec!["exec", &self.name];
        args.extend_from_slice(cmd);
        text(&docker(&args).expect("docker exec"))
    }

    /// Inject a group write from inside knxd (`knxtool`, as a local client).
    pub fn knxtool_group_write(&self, group: &str, value: &str) {
        let out = self.exec_root_free(&[
            "knxtool",
            "groupswrite",
            "local:/run/knxd/knxd.sock",
            group,
            value,
        ]);
        eprintln!("knxtool groupswrite {group} {value}: {}", out.trim());
    }

    /// Freeze the container: knxd stops answering, but its socket still
    /// collects datagrams, like a client that has hung.
    pub fn pause(&self) {
        let out = docker(&["pause", &self.name]).expect("docker pause");
        assert!(out.status.success(), "docker pause: {}", text(&out));
    }

    /// Resume a frozen container.
    pub fn unpause(&self) {
        let out = docker(&["unpause", &self.name]).expect("docker unpause");
        assert!(out.status.success(), "docker unpause: {}", text(&out));
    }

    /// knxd's log so far.
    pub fn logs(&self) -> String {
        text(&docker(&["logs", &self.name]).expect("docker logs"))
    }

    /// Start a tcpdump of all UDP traffic inside the container; returns once
    /// it is listening.
    pub async fn capture_start(&self) {
        let out = docker(&[
            "exec",
            "-u",
            "root",
            "-d",
            &self.name,
            "tcpdump",
            "-nn",
            "-i",
            "any",
            "-U",
            "--immediate-mode",
            "-w",
            "/tmp/wire.pcap",
            "udp",
        ])
        .expect("docker exec tcpdump");
        assert!(out.status.success(), "tcpdump: {}", text(&out));
        // tcpdump writes the pcap header as soon as it is capturing.
        self.wait_for("tcpdump", || {
            self.exec_status(&["test", "-s", "/tmp/wire.pcap"])
        })
        .await;
    }

    /// Stop the capture and return it as a hex dump (`tcpdump -X`).
    pub async fn capture_stop(&self) -> String {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _ = docker(&["exec", "-u", "root", &self.name, "pkill", "tcpdump"]);
        tokio::time::sleep(Duration::from_millis(200)).await;
        text(
            &docker(&[
                "exec",
                "-u",
                "root",
                &self.name,
                "tcpdump",
                "-nn",
                "-X",
                "-tttt",
                "-r",
                "/tmp/wire.pcap",
            ])
            .expect("docker exec tcpdump -r"),
        )
    }

    /// A relay carrying one client's datagrams to the tunnelling server of
    /// this container.
    pub async fn relay(&self) -> Relay {
        Relay::start(&self.name).await
    }
}

impl Drop for Knxd {
    fn drop(&mut self) {
        let _ = docker(&["rm", "-f", &self.name]);
    }
}

// ── Datagram relay with wire trace ────────────────────────────

/// One datagram seen by the relay.
#[derive(Clone, Debug)]
pub struct Datagram {
    /// Time since the relay started.
    pub at: Duration,
    /// `true` for client to knxd.
    pub from_client: bool,
    pub bytes: Vec<u8>,
}

impl Datagram {
    /// The frame, when it parses as KNXnet/IP.
    pub fn frame(&self) -> Option<KnxIpFrame> {
        KnxIpFrame::parse(&self.bytes).ok()
    }

    /// Message code of the cEMI frame in a tunnelling request.
    pub fn cemi_code(&self) -> Option<u8> {
        let frame = self.frame()?;
        (frame.service_type == ServiceType::TunnelingRequest)
            .then(|| frame.body.get(usize::from(ConnectionHeader::LEN)).copied())
            .flatten()
    }

    /// One line for the wire trace.
    pub fn describe(&self) -> String {
        let arrow = if self.from_client {
            "client -> knxd"
        } else {
            "knxd -> client"
        };
        let hex: Vec<String> = self.bytes.iter().map(|b| format!("{b:02x}")).collect();
        let detail = match self.frame() {
            Some(frame) => {
                let extra = ConnectionHeader::parse(&frame.body)
                    .filter(|_| {
                        matches!(
                            frame.service_type,
                            ServiceType::TunnelingRequest | ServiceType::TunnelingAck
                        )
                    })
                    .map(|h| {
                        format!(
                            " ch={} seq={} status={}",
                            h.channel_id, h.sequence_counter, h.status
                        )
                    })
                    .unwrap_or_default();
                format!("{:?}{extra}", frame.service_type)
            }
            None => "unparseable".to_owned(),
        };
        format!(
            "+{:8.3}s {arrow}: {detail}  [{}]",
            self.at.as_secs_f64(),
            hex.join(" ")
        )
    }
}

struct Shared {
    started: Instant,
    trace: Mutex<Vec<Datagram>>,
    withhold_client_acks: AtomicBool,
}

impl Shared {
    fn record(&self, from_client: bool, bytes: &[u8]) {
        self.trace.lock().unwrap().push(Datagram {
            at: self.started.elapsed(),
            from_client,
            bytes: bytes.to_vec(),
        });
    }
}

/// Put the NAT-mode HPAI (`0.0.0.0:0`) into the endpoints of a client request,
/// so knxd answers to the source of the datagrams, which is the relay inside
/// the container, instead of the client's host address.
fn to_nat_mode(bytes: &[u8]) -> Vec<u8> {
    const NAT_HPAI: [u8; 8] = [0x08, 0x01, 0, 0, 0, 0, 0, 0];
    let mut out = bytes.to_vec();
    let Ok(frame) = KnxIpFrame::parse(bytes) else {
        return out;
    };
    let offsets: &[usize] = match frame.service_type {
        ServiceType::ConnectRequest => &[6, 14],
        ServiceType::ConnectionStateRequest | ServiceType::DisconnectRequest => &[8],
        _ => &[],
    };
    for &at in offsets {
        if let Some(slot) = out.get_mut(at..at + 8) {
            slot.copy_from_slice(&NAT_HPAI);
        }
    }
    out
}

/// A UDP endpoint on the host that forwards to the knxd container.
pub struct Relay {
    /// Where the client under test connects.
    pub addr: SocketAddr,
    shared: Arc<Shared>,
    task: tokio::task::JoinHandle<()>,
}

impl Relay {
    async fn start(container: &str) -> Self {
        let front = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = front.local_addr().unwrap();
        let mut child = tokio::process::Command::new("docker")
            .args([
                "exec",
                "-i",
                container,
                "python3",
                "/usr/local/bin/relay.py",
                "127.0.0.1",
                "3671",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("docker exec relay");
        let mut to_knxd = child.stdin.take().unwrap();
        let mut from_knxd = child.stdout.take().unwrap();
        let shared = Arc::new(Shared {
            started: Instant::now(),
            trace: Mutex::new(Vec::new()),
            withhold_client_acks: AtomicBool::new(false),
        });

        // Reader task: whole datagrams from the container's stdout.
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(async move {
            loop {
                let mut len = [0u8; 2];
                if from_knxd.read_exact(&mut len).await.is_err() {
                    return;
                }
                let mut payload = vec![0u8; usize::from(u16::from_be_bytes(len))];
                if from_knxd.read_exact(&mut payload).await.is_err() || tx.send(payload).is_err() {
                    return;
                }
            }
        });

        let state = Arc::clone(&shared);
        let task = tokio::spawn(async move {
            let _child = child; // killed when the task ends
            let mut client: Option<SocketAddr> = None;
            let mut buf = [0u8; 2048];
            loop {
                tokio::select! {
                    got = front.recv_from(&mut buf) => {
                        let Ok((n, from)) = got else { return };
                        client = Some(from);
                        state.record(true, &buf[..n]);
                        let is_ack = KnxIpFrame::parse(&buf[..n])
                            .is_ok_and(|f| f.service_type == ServiceType::TunnelingAck);
                        if is_ack && state.withhold_client_acks.load(Ordering::Relaxed) {
                            continue;
                        }
                        let bytes = to_nat_mode(&buf[..n]);
                        let mut framed = u16::try_from(bytes.len()).unwrap().to_be_bytes().to_vec();
                        framed.extend_from_slice(&bytes);
                        if to_knxd.write_all(&framed).await.is_err() || to_knxd.flush().await.is_err() {
                            return;
                        }
                    }
                    got = rx.recv() => {
                        let Some(bytes) = got else { return };
                        state.record(false, &bytes);
                        if let Some(client) = client {
                            let _ = front.send_to(&bytes, client).await;
                        }
                    }
                }
            }
        });
        Self { addr, shared, task }
    }

    /// Stop forwarding the client's tunnelling acknowledgements to knxd, as a
    /// client that has gone silent would.
    pub fn withhold_client_acks(&self, on: bool) {
        self.shared
            .withhold_client_acks
            .store(on, Ordering::Relaxed);
    }

    /// Everything seen so far.
    pub fn trace(&self) -> Vec<Datagram> {
        self.shared.trace.lock().unwrap().clone()
    }

    /// The trace as text, one datagram per line.
    pub fn trace_text(&self) -> String {
        self.trace()
            .iter()
            .map(Datagram::describe)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Wait until a datagram matching `pred` was seen; panics after `wait`.
    pub async fn wait_for(&self, what: &str, wait: Duration, pred: impl Fn(&Datagram) -> bool) {
        let deadline = Instant::now() + wait;
        loop {
            if self.trace().iter().any(&pred) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; wire trace:\n{}",
                self.trace_text()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

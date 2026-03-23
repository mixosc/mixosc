use crate::reference::ReferenceFiles;
use std::env;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const X32_DEFAULT_PORT: u16 = 10023;
pub const X32_BROADCAST_ADDR: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), X32_DEFAULT_PORT);
const INFO_REQUEST: &[u8] = b"/info\0\0\0,\0\0\0";
const STATUS_REQUEST: &[u8] = b"/status\0,\0\0\0";
const XINFO_REQUEST: &[u8] = b"/xinfo\0\0,\0\0\0";
const XINFO_RESPONSE: &str = "/xinfo";
const INFO_RESPONSE: &str = "/info";
const STATUS_RESPONSE: &str = "/status";

#[derive(Debug, Clone)]
pub struct ConnectionProbe {
    target: SocketAddr,
    timeout: Duration,
    bind_addr: SocketAddr,
}

#[derive(Debug, Clone)]
pub struct DiscoveryProbe {
    bind_addr: SocketAddr,
    broadcast_addr: SocketAddr,
    timeout: Duration,
}

impl DiscoveryProbe {
    pub fn new() -> Self {
        Self {
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            broadcast_addr: X32_BROADCAST_ADDR,
            timeout: Duration::from_millis(1200),
        }
    }

    pub fn with_bind_addr(mut self, bind_addr: SocketAddr) -> Self {
        self.bind_addr = bind_addr;
        self
    }

    pub fn with_broadcast_addr(mut self, broadcast_addr: SocketAddr) -> Self {
        self.broadcast_addr = broadcast_addr;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn discover(&self) -> Result<Vec<DiscoveredMixer>, ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket.set_broadcast(true).map_err(ProbeError::Configure)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        socket
            .send_to(XINFO_REQUEST, self.broadcast_addr)
            .map_err(ProbeError::Send)?;

        let start = Instant::now();
        let mut mixers: Vec<DiscoveredMixer> = Vec::new();
        let mut buffer = [0_u8; 2048];

        loop {
            match socket.recv_from(&mut buffer) {
                Ok((received, responder)) => {
                    if let Some(mixer) = parse_discovered_mixer(&buffer[..received], responder) {
                        if mixers.iter().all(|known| known.addr != mixer.addr) {
                            mixers.push(mixer);
                        }
                    }

                    if start.elapsed() >= self.timeout {
                        break;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(error) => return Err(ProbeError::Receive(error)),
            }
        }

        Ok(mixers)
    }
}

impl ConnectionProbe {
    pub fn new(target: SocketAddr) -> Self {
        Self {
            target,
            timeout: Duration::from_millis(750),
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 0)),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_bind_addr(mut self, bind_addr: SocketAddr) -> Self {
        self.bind_addr = bind_addr;
        self
    }

    pub fn target(&self) -> SocketAddr {
        self.target
    }

    pub fn probe(&self) -> Result<ProbeOutcome, ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        socket
            .send_to(INFO_REQUEST, self.target)
            .map_err(ProbeError::Send)?;

        let mut buffer = [0_u8; 2048];
        match socket.recv_from(&mut buffer) {
            Ok((received, responder)) => Ok(ProbeOutcome::Connected {
                responder,
                response: parse_response(&buffer[..received]),
            }),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                socket
                    .send_to(STATUS_REQUEST, self.target)
                    .map_err(ProbeError::Send)?;
                match socket.recv_from(&mut buffer) {
                    Ok((received, responder)) => Ok(ProbeOutcome::Connected {
                        responder,
                        response: parse_response(&buffer[..received]),
                    }),
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        Ok(ProbeOutcome::Disconnected)
                    }
                    Err(error) => Err(ProbeError::Receive(error)),
                }
            }
            Err(error) => Err(ProbeError::Receive(error)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredMixer {
    pub addr: SocketAddr,
    pub network_address: Option<String>,
    pub name: Option<String>,
    pub model: Option<String>,
    pub firmware: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    Connected {
        responder: SocketAddr,
        response: ProbeResponse,
    },
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeResponse {
    Info,
    Status,
    XInfo,
    Unknown,
}

#[derive(Debug)]
pub enum ProbeError {
    Bind(io::Error),
    Configure(io::Error),
    Send(io::Error),
    Receive(io::Error),
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bind(error) => write!(f, "failed to bind UDP socket: {error}"),
            Self::Configure(error) => write!(f, "failed to configure UDP socket: {error}"),
            Self::Send(error) => write!(f, "failed to send probe to mixer: {error}"),
            Self::Receive(error) => write!(f, "failed to receive mixer response: {error}"),
        }
    }
}

impl std::error::Error for ProbeError {}

pub fn parse_target(input: &str) -> Result<SocketAddr, ParseTargetError> {
    if let Ok(addr) = input.parse::<SocketAddr>() {
        return Ok(addr);
    }

    let candidate = format!("{input}:{X32_DEFAULT_PORT}");
    let mut resolved = candidate.to_socket_addrs()?;
    resolved.next().ok_or(ParseTargetError::NoResolvedAddress)
}

pub fn default_reference_dir() -> PathBuf {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"));

    home.join("Files").join("OSC")
}

pub fn default_reference_files() -> ReferenceFiles {
    ReferenceFiles::new(default_reference_dir())
}

#[derive(Debug)]
pub enum ParseTargetError {
    Resolve(io::Error),
    NoResolvedAddress,
}

impl std::fmt::Display for ParseTargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resolve(error) => write!(f, "failed to resolve mixer address: {error}"),
            Self::NoResolvedAddress => write!(f, "mixer address did not resolve to a socket"),
        }
    }
}

impl std::error::Error for ParseTargetError {}

impl From<io::Error> for ParseTargetError {
    fn from(value: io::Error) -> Self {
        Self::Resolve(value)
    }
}

fn parse_response(packet: &[u8]) -> ProbeResponse {
    match osc_address(packet) {
        Some(INFO_RESPONSE) => ProbeResponse::Info,
        Some(STATUS_RESPONSE) => ProbeResponse::Status,
        Some(XINFO_RESPONSE) => ProbeResponse::XInfo,
        _ => ProbeResponse::Unknown,
    }
}

fn parse_discovered_mixer(packet: &[u8], responder: SocketAddr) -> Option<DiscoveredMixer> {
    if !matches!(parse_response(packet), ProbeResponse::XInfo) {
        return None;
    }

    let strings = osc_strings(packet);

    Some(DiscoveredMixer {
        addr: responder,
        network_address: strings.first().cloned(),
        name: strings.get(1).cloned(),
        model: strings.get(2).cloned(),
        firmware: strings.get(3).cloned(),
    })
}

fn osc_address(packet: &[u8]) -> Option<&str> {
    let end = packet.iter().position(|byte| *byte == 0)?;
    std::str::from_utf8(&packet[..end]).ok()
}

fn osc_strings(packet: &[u8]) -> Vec<String> {
    let Some(mut offset) = osc_padded_len(packet) else {
        return Vec::new();
    };
    let Some(type_tag_len) = packet.get(offset..).and_then(osc_padded_len) else {
        return Vec::new();
    };
    offset += type_tag_len;

    let mut values = Vec::new();

    while offset < packet.len() {
        let bytes = &packet[offset..];
        let Some(end) = bytes.iter().position(|byte| *byte == 0) else {
            break;
        };

        let Some(value) = std::str::from_utf8(&bytes[..end]).ok() else {
            break;
        };
        let Some(padded_len) = osc_padded_len(bytes) else {
            break;
        };

        let value = value.to_owned();
        values.push(value);
        offset += padded_len;
    }

    values
}

fn osc_padded_len(bytes: &[u8]) -> Option<usize> {
    let end = bytes.iter().position(|byte| *byte == 0)?;
    let raw = end + 1;
    Some((raw + 3) & !3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_osc_address_from_packet() {
        assert_eq!(osc_address(b"/info\0\0\0,\0\0\0"), Some("/info"));
    }

    #[test]
    fn identifies_known_probe_responses() {
        assert_eq!(parse_response(b"/status\0,\0\0\0"), ProbeResponse::Status);
        assert_eq!(parse_response(b"/xinfo\0\0,\0\0\0"), ProbeResponse::XInfo);
    }

    #[test]
    fn applies_default_port_to_bare_host() {
        let target = parse_target("127.0.0.1").expect("should parse localhost");
        assert_eq!(target.port(), X32_DEFAULT_PORT);
    }

    #[test]
    fn parses_xinfo_discovery_payload() {
        let packet = concat!(
            "/xinfo\0\0,\0\0\0",
            "192.168.1.62\0\0\0\0",
            "X32-024A-53\0",
            "X32\0",
            "3.04\0\0\0"
        )
        .as_bytes();
        let responder = SocketAddr::from(([192, 168, 1, 62], X32_DEFAULT_PORT));

        let mixer = parse_discovered_mixer(packet, responder).expect("xinfo should parse");

        assert_eq!(mixer.addr, responder);
        assert_eq!(mixer.network_address.as_deref(), Some("192.168.1.62"));
        assert_eq!(mixer.name.as_deref(), Some("X32-024A-53"));
        assert_eq!(mixer.model.as_deref(), Some("X32"));
        assert_eq!(mixer.firmware.as_deref(), Some("3.04"));
    }
}

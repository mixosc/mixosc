use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

pub const X32_DEFAULT_PORT: u16 = 10023;
pub const X32_BROADCAST_ADDR: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), X32_DEFAULT_PORT);
const INFO_REQUEST: &[u8] = b"/info\0\0\0,\0\0\0";
const STATUS_REQUEST: &[u8] = b"/status\0,\0\0\0";
const XINFO_REQUEST: &[u8] = b"/xinfo\0\0,\0\0\0";
pub const XREMOTE_REQUEST: &[u8] = b"/xremote\0\0\0,\0\0\0";
const XINFO_RESPONSE: &str = "/xinfo";
const INFO_RESPONSE: &str = "/info";
const STATUS_RESPONSE: &str = "/status";
const FADER_RESPONSE_SUFFIX: &str = "/mix/fader";
const PAN_RESPONSE_SUFFIX: &str = "/mix/pan";
const GAIN_RESPONSE_SUFFIX: &str = "/preamp/trim";
const HEADAMP_GAIN_RESPONSE_SUFFIX: &str = "/gain";
const HEADAMP_INDEX_RESPONSE_SUFFIX: &str = "/index";
const MUTE_RESPONSE_SUFFIX: &str = "/mix/on";
const SOLO_RESPONSE_PREFIX: &str = "/-stat/solosw/";
const NAME_RESPONSE_SUFFIX: &str = "/config/name";
const COLOR_RESPONSE_SUFFIX: &str = "/config/color";
const INPUT_METERS_REQUEST: &str = "/meters/0";
const INPUT_METERS_ALIAS: &str = "meters/0";
const MAIN_METERS_REQUEST: &str = "/meters/2";
const MAIN_METERS_ALIAS: &str = "meters/2";

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

#[derive(Debug, Clone)]
pub struct FaderBankProbe {
    target: SocketAddr,
    bind_addr: SocketAddr,
    timeout: Duration,
}

impl FaderBankProbe {
    pub fn new(target: SocketAddr) -> Self {
        Self {
            target,
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            timeout: Duration::from_millis(400),
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

    pub fn load(&self, targets: &[FaderTarget]) -> Result<Vec<StripFader>, ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let mut faders = Vec::with_capacity(targets.len());

        for &target in targets {
            let path = fader_path(target);
            let request = osc_query(&path);
            socket
                .send_to(&request, self.target)
                .map_err(ProbeError::Send)?;

            let mut buffer = [0_u8; 2048];
            let (received, _) = socket.recv_from(&mut buffer).map_err(ProbeError::Receive)?;
            let packet = &buffer[..received];
            let Some((path, value)) = parse_fader_value(packet) else {
                return Err(ProbeError::Protocol(format!(
                    "unexpected OSC reply while reading {target}"
                )));
            };

            if path != fader_path(target) {
                return Err(ProbeError::Protocol(format!(
                    "received fader reply for '{path}' while reading {target}"
                )));
            }

            faders.push(StripFader { target, value });
        }

        Ok(faders)
    }

    pub fn set(&self, target: FaderTarget, value: f32) -> Result<(), ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let packet = osc_float_message(&fader_path(target), value.clamp(0.0, 1.0));
        socket.send_to(&packet, self.target).map_err(ProbeError::Send)?;
        Ok(())
    }
}

impl PanBankProbe {
    pub fn new(target: SocketAddr) -> Self {
        Self {
            target,
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            timeout: Duration::from_millis(400),
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

    pub fn load(&self, targets: &[FaderTarget]) -> Result<Vec<StripPan>, ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let mut pans = Vec::with_capacity(targets.len());

        for &target in targets {
            let path = pan_path(target);
            let request = osc_query(&path);
            socket
                .send_to(&request, self.target)
                .map_err(ProbeError::Send)?;

            let mut buffer = [0_u8; 2048];
            let (received, _) = socket.recv_from(&mut buffer).map_err(ProbeError::Receive)?;
            let packet = &buffer[..received];
            let Some((reply_path, value)) = parse_pan_value(packet) else {
                return Err(ProbeError::Protocol(format!(
                    "unexpected OSC reply while reading pan for {target}"
                )));
            };

            if reply_path != path {
                return Err(ProbeError::Protocol(format!(
                    "received pan reply for '{reply_path}' while reading {target}"
                )));
            }

            pans.push(StripPan { target, value });
        }

        Ok(pans)
    }

    pub fn set(&self, target: FaderTarget, value: f32) -> Result<(), ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let packet = osc_float_message(&pan_path(target), value.clamp(0.0, 1.0));
        socket.send_to(&packet, self.target).map_err(ProbeError::Send)?;
        Ok(())
    }
}

impl SendBankProbe {
    pub fn new(target: SocketAddr) -> Self {
        Self {
            target,
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            timeout: Duration::from_millis(400),
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

    pub fn load(&self, targets: &[FaderTarget], buses: &[u8]) -> Result<Vec<StripSend>, ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let mut sends = Vec::with_capacity(targets.len() * buses.len());

        for &target in targets {
            for &bus in buses {
                let path = send_level_path(target, bus);
                let request = osc_query(&path);
                socket
                    .send_to(&request, self.target)
                    .map_err(ProbeError::Send)?;

                let mut buffer = [0_u8; 2048];
                let (received, _) = socket.recv_from(&mut buffer).map_err(ProbeError::Receive)?;
                let packet = &buffer[..received];
                let Some((reply_path, value)) = parse_send_value(packet) else {
                    return Err(ProbeError::Protocol(format!(
                        "unexpected OSC reply while reading send {bus:02} for {target}"
                    )));
                };

                if reply_path != path {
                    return Err(ProbeError::Protocol(format!(
                        "received send reply for '{reply_path}' while reading bus {bus:02} for {target}"
                    )));
                }

                sends.push(StripSend { target, bus, value });
            }
        }

        Ok(sends)
    }

    pub fn set(&self, target: FaderTarget, bus: u8, value: f32) -> Result<(), ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let packet = osc_float_message(&send_level_path(target, bus), value.clamp(0.0, 1.0));
        socket.send_to(&packet, self.target).map_err(ProbeError::Send)?;
        Ok(())
    }
}

impl GainBankProbe {
    pub fn new(target: SocketAddr) -> Self {
        Self {
            target,
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            timeout: Duration::from_millis(400),
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

    pub fn load(&self, targets: &[FaderTarget]) -> Result<Vec<StripGain>, ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let mut gains = Vec::with_capacity(targets.len());

        for &target in targets {
            gains.push(self.read_gain(&socket, target)?);
        }

        Ok(gains)
    }

    pub fn set(
        &self,
        target: FaderTarget,
        source: GainSource,
        value: f32,
    ) -> Result<(), ProbeError> {
        if matches!(target, FaderTarget::Bus(_) | FaderTarget::FxRtn(_) | FaderTarget::Mtx(_) | FaderTarget::Dca(_)) {
            return Ok(());
        }

        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let packet = match source {
            GainSource::Headamp(index) => {
                osc_float_message(&headamp_gain_path(index), encode_headamp_gain(value))
            }
            GainSource::Trim => {
                osc_float_message(&gain_path(target), encode_trim_gain(value))
            }
        };
        socket.send_to(&packet, self.target).map_err(ProbeError::Send)?;
        Ok(())
    }

    fn read_gain(&self, socket: &UdpSocket, target: FaderTarget) -> Result<StripGain, ProbeError> {
        if matches!(target, FaderTarget::Bus(_) | FaderTarget::FxRtn(_) | FaderTarget::Mtx(_) | FaderTarget::Dca(_)) {
            return Ok(StripGain {
                target,
                value: 0.0,
                source: GainSource::Trim,
            });
        }

        if gain_uses_headamp(target) && let Some(index) = self.read_headamp_index(socket, target)? {
            let path = headamp_gain_path(index);
            let request = osc_query(&path);
            socket
                .send_to(&request, self.target)
                .map_err(ProbeError::Send)?;

            let mut buffer = [0_u8; 2048];
            let (received, _) = socket.recv_from(&mut buffer).map_err(ProbeError::Receive)?;
            let packet = &buffer[..received];
            let Some((reply_path, value)) = parse_headamp_gain_value(packet) else {
                return Err(ProbeError::Protocol(format!(
                    "unexpected OSC reply while reading headamp gain for {target}"
                )));
            };

            if reply_path != path {
                return Err(ProbeError::Protocol(format!(
                    "received headamp gain reply for '{reply_path}' while reading {target}"
                )));
            }

            Ok(StripGain {
                target,
                value: decode_headamp_gain(value),
                source: GainSource::Headamp(index),
            })
        } else {
            let path = gain_path(target);
            let request = osc_query(&path);
            socket
                .send_to(&request, self.target)
                .map_err(ProbeError::Send)?;

            let mut buffer = [0_u8; 2048];
            let (received, _) = socket.recv_from(&mut buffer).map_err(ProbeError::Receive)?;
            let packet = &buffer[..received];
            let Some((reply_path, value)) = parse_gain_value(packet) else {
                return Err(ProbeError::Protocol(format!(
                    "unexpected OSC reply while reading trim gain for {target}"
                )));
            };

            if reply_path != path {
                return Err(ProbeError::Protocol(format!(
                    "received trim gain reply for '{reply_path}' while reading {target}"
                )));
            }

            Ok(StripGain {
                target,
                value: decode_trim_gain(value),
                source: GainSource::Trim,
            })
        }
    }

    fn read_headamp_index(
        &self,
        socket: &UdpSocket,
        target: FaderTarget,
    ) -> Result<Option<u8>, ProbeError> {
        let path = headamp_index_path(target);
        let request = osc_query(&path);
        socket
            .send_to(&request, self.target)
            .map_err(ProbeError::Send)?;

        let mut buffer = [0_u8; 2048];
        let (received, _) = socket.recv_from(&mut buffer).map_err(ProbeError::Receive)?;
        let packet = &buffer[..received];
        let Some((reply_path, value)) = parse_headamp_index_value(packet) else {
            return Err(ProbeError::Protocol(format!(
                "unexpected OSC reply while reading headamp index for {target}"
            )));
        };

        if reply_path != path {
            return Err(ProbeError::Protocol(format!(
                "received headamp index reply for '{reply_path}' while reading {target}"
            )));
        }

        if value < 0 {
            Ok(None)
        } else {
            Ok(Some(value as u8))
        }
    }
}

fn gain_uses_headamp(target: FaderTarget) -> bool {
    !matches!(target, FaderTarget::Channel(17..=32))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaderTarget {
    Channel(u8),
    Aux(u8),
    Bus(u8),
    FxRtn(u8),
    Mtx(u8),
    Dca(u8),
    Main,
}

impl std::fmt::Display for FaderTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Channel(channel) => write!(f, "channel {channel:02}"),
            Self::Aux(aux) => write!(f, "aux {aux:02}"),
            Self::Bus(bus) => write!(f, "bus {bus:02}"),
            Self::FxRtn(fx) => write!(f, "fxrtn {fx:02}"),
            Self::Mtx(mtx) => write!(f, "mtx {mtx:02}"),
            Self::Dca(dca) => write!(f, "dca {dca}"),
            Self::Main => write!(f, "main lr"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripFader {
    pub target: FaderTarget,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripPan {
    pub target: FaderTarget,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripGain {
    pub target: FaderTarget,
    pub value: f32,
    pub source: GainSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GainSource {
    Headamp(u8),
    Trim,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripSend {
    pub target: FaderTarget,
    pub bus: u8,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripName {
    pub target: FaderTarget,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripColor {
    pub target: FaderTarget,
    pub value: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripMute {
    pub target: FaderTarget,
    pub on: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripSolo {
    pub target: FaderTarget,
    pub on: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripMeter {
    pub target: FaderTarget,
    pub level_linear: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConsoleUpdate {
    Gain(StripGain),
    HeadampGain { index: u8, value: f32 },
    Fader(StripFader),
    Pan(StripPan),
    Send(StripSend),
    Mute(StripMute),
    Solo(StripSolo),
    Name(StripName),
    Color(StripColor),
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
    Protocol(String),
}

#[derive(Debug, Clone)]
pub struct MuteBankProbe {
    target: SocketAddr,
    bind_addr: SocketAddr,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct PanBankProbe {
    target: SocketAddr,
    bind_addr: SocketAddr,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct GainBankProbe {
    target: SocketAddr,
    bind_addr: SocketAddr,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct SendBankProbe {
    target: SocketAddr,
    bind_addr: SocketAddr,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct NameBankProbe {
    target: SocketAddr,
    bind_addr: SocketAddr,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct ColorBankProbe {
    target: SocketAddr,
    bind_addr: SocketAddr,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct SoloBankProbe {
    target: SocketAddr,
    bind_addr: SocketAddr,
    timeout: Duration,
}

impl SoloBankProbe {
    pub fn new(target: SocketAddr) -> Self {
        Self {
            target,
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            timeout: Duration::from_millis(400),
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

    pub fn load(&self, targets: &[FaderTarget]) -> Result<Vec<StripSolo>, ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let mut solos = Vec::with_capacity(targets.len());

        for &target in targets {
            let path = solo_path(target);
            let request = osc_query(&path);
            socket
                .send_to(&request, self.target)
                .map_err(ProbeError::Send)?;

            let mut buffer = [0_u8; 2048];
            let (received, _) = socket.recv_from(&mut buffer).map_err(ProbeError::Receive)?;
            let packet = &buffer[..received];
            let Some((reply_path, on)) = parse_switch_value(packet) else {
                return Err(ProbeError::Protocol(format!(
                    "unexpected OSC reply while reading solo for {target}"
                )));
            };

            if reply_path != path {
                return Err(ProbeError::Protocol(format!(
                    "received solo reply for '{reply_path}' while reading {target}"
                )));
            }

            solos.push(StripSolo { target, on });
        }

        Ok(solos)
    }

    pub fn set(&self, target: FaderTarget, on: bool) -> Result<(), ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let packet = osc_int_message(&solo_path(target), i32::from(on));
        socket.send_to(&packet, self.target).map_err(ProbeError::Send)?;
        Ok(())
    }
}

impl MuteBankProbe {
    pub fn new(target: SocketAddr) -> Self {
        Self {
            target,
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            timeout: Duration::from_millis(400),
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

    pub fn load(&self, targets: &[FaderTarget]) -> Result<Vec<StripMute>, ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let mut mutes = Vec::with_capacity(targets.len());

        for &target in targets {
            let path = mute_path(target);
            let request = osc_query(&path);
            socket
                .send_to(&request, self.target)
                .map_err(ProbeError::Send)?;

            let mut buffer = [0_u8; 2048];
            let (received, _) = socket.recv_from(&mut buffer).map_err(ProbeError::Receive)?;
            let packet = &buffer[..received];
            let Some((reply_path, on)) = parse_switch_value(packet) else {
                return Err(ProbeError::Protocol(format!(
                    "unexpected OSC reply while reading mute for {target}"
                )));
            };

            if reply_path != path {
                return Err(ProbeError::Protocol(format!(
                    "received mute reply for '{reply_path}' while reading {target}"
                )));
            }

            mutes.push(StripMute { target, on });
        }

        Ok(mutes)
    }

    pub fn set(&self, target: FaderTarget, on: bool) -> Result<(), ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let packet = osc_int_message(&mute_path(target), i32::from(on));
        socket.send_to(&packet, self.target).map_err(ProbeError::Send)?;
        Ok(())
    }
}

impl NameBankProbe {
    pub fn new(target: SocketAddr) -> Self {
        Self {
            target,
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            timeout: Duration::from_millis(400),
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

    pub fn load(&self, targets: &[FaderTarget]) -> Result<Vec<StripName>, ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let mut names = Vec::with_capacity(targets.len());

        for &target in targets {
            let path = name_path(target);
            let request = osc_query(&path);
            socket
                .send_to(&request, self.target)
                .map_err(ProbeError::Send)?;

            let mut buffer = [0_u8; 2048];
            let (received, _) = socket.recv_from(&mut buffer).map_err(ProbeError::Receive)?;
            let packet = &buffer[..received];
            let Some((reply_path, value)) = parse_string_value(packet) else {
                return Err(ProbeError::Protocol(format!(
                    "unexpected OSC reply while reading name for {target}"
                )));
            };

            if reply_path != path {
                return Err(ProbeError::Protocol(format!(
                    "received name reply for '{reply_path}' while reading {target}"
                )));
            }

            names.push(StripName { target, value });
        }

        Ok(names)
    }
}

impl ColorBankProbe {
    pub fn new(target: SocketAddr) -> Self {
        Self {
            target,
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            timeout: Duration::from_millis(400),
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

    pub fn load(&self, targets: &[FaderTarget]) -> Result<Vec<StripColor>, ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;

        let mut colors = Vec::with_capacity(targets.len());

        for &target in targets {
            let path = color_path(target);
            let request = osc_query(&path);
            socket
                .send_to(&request, self.target)
                .map_err(ProbeError::Send)?;

            let mut buffer = [0_u8; 2048];
            let (received, _) = socket.recv_from(&mut buffer).map_err(ProbeError::Receive)?;
            let packet = &buffer[..received];
            let Some((reply_path, value)) = parse_color_value(packet) else {
                return Err(ProbeError::Protocol(format!(
                    "unexpected OSC reply while reading color for {target}"
                )));
            };

            if reply_path != path {
                return Err(ProbeError::Protocol(format!(
                    "received color reply for '{reply_path}' while reading {target}"
                )));
            }

            colors.push(StripColor { target, value });
        }

        Ok(colors)
    }
}

#[derive(Debug, Clone)]
pub struct MeterBankProbe {
    target: SocketAddr,
    bind_addr: SocketAddr,
    timeout: Duration,
}

impl MeterBankProbe {
    pub fn new(target: SocketAddr) -> Self {
        Self {
            target,
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            timeout: Duration::from_millis(400),
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

    pub fn load_inputs(&self) -> Result<Vec<StripMeter>, ProbeError> {
        let socket = self.bind_socket()?;
        let request = osc_meter_group_request(INPUT_METERS_REQUEST);
        socket
            .send_to(&request, self.target)
            .map_err(ProbeError::Send)?;
        let mut buffer = [0_u8; 4096];
        let (received, _) = socket.recv_from(&mut buffer).map_err(ProbeError::Receive)?;
        parse_input_meter_packet(&buffer[..received])
    }

    fn bind_socket(&self) -> Result<UdpSocket, ProbeError> {
        let socket = UdpSocket::bind(self.bind_addr).map_err(ProbeError::Bind)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(ProbeError::Configure)?;
        Ok(socket)
    }
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bind(error) => write!(f, "failed to bind UDP socket: {error}"),
            Self::Configure(error) => write!(f, "failed to configure UDP socket: {error}"),
            Self::Send(error) => write!(f, "failed to send probe to mixer: {error}"),
            Self::Receive(error) => write!(f, "failed to receive mixer response: {error}"),
            Self::Protocol(error) => write!(f, "invalid mixer protocol data: {error}"),
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

fn fader_path(target: FaderTarget) -> String {
    match target {
        FaderTarget::Channel(channel) => format!("/ch/{channel:02}/mix/fader"),
        FaderTarget::Aux(aux) => format!("/auxin/{aux:02}/mix/fader"),
        FaderTarget::Bus(bus) => format!("/bus/{bus:02}/mix/fader"),
        FaderTarget::FxRtn(fx) => format!("/fxrtn/{fx:02}/mix/fader"),
        FaderTarget::Mtx(mtx) => format!("/mtx/{mtx:02}/mix/fader"),
        FaderTarget::Dca(dca) => format!("/dca/{dca}/fader"),
        FaderTarget::Main => "/main/st/mix/fader".to_owned(),
    }
}

fn pan_path(target: FaderTarget) -> String {
    match target {
        FaderTarget::Channel(channel) => format!("/ch/{channel:02}/mix/pan"),
        FaderTarget::Aux(aux) => format!("/auxin/{aux:02}/mix/pan"),
        FaderTarget::Bus(bus) => format!("/bus/{bus:02}/mix/pan"),
        FaderTarget::FxRtn(fx) => format!("/fxrtn/{fx:02}/mix/pan"),
        FaderTarget::Mtx(mtx) => format!("/mtx/{mtx:02}/mix/pan"),
        FaderTarget::Dca(_) => String::new(),
        FaderTarget::Main => "/main/st/mix/pan".to_owned(),
    }
}

fn gain_path(target: FaderTarget) -> String {
    match target {
        FaderTarget::Channel(channel) => format!("/ch/{channel:02}/preamp/trim"),
        FaderTarget::Aux(aux) => format!("/auxin/{aux:02}/preamp/trim"),
        FaderTarget::Bus(_) => "/bus/01/preamp/trim".to_owned(),
        FaderTarget::FxRtn(_) => "/fxrtn/01/preamp/trim".to_owned(),
        FaderTarget::Mtx(_) | FaderTarget::Dca(_) => String::new(),
        FaderTarget::Main => "/main/st/preamp/trim".to_owned(),
    }
}

fn headamp_index_path(target: FaderTarget) -> String {
    let index = match target {
        FaderTarget::Channel(channel) => channel - 1,
        FaderTarget::Aux(aux) => 31 + aux,
        FaderTarget::Bus(_) => 255,
        FaderTarget::FxRtn(_) => 255,
        FaderTarget::Mtx(_) | FaderTarget::Dca(_) => 255,
        FaderTarget::Main => 255,
    };
    format!("/-ha/{index:02}/index")
}

fn headamp_gain_path(index: u8) -> String {
    format!("/headamp/{index:03}/gain")
}

fn headamp_index_from_gain_path(path: &str) -> Option<u8> {
    path.strip_prefix("/headamp/")
        .and_then(|rest| rest.strip_suffix("/gain"))
        .and_then(|index| index.parse::<u8>().ok())
}

fn send_level_path(target: FaderTarget, bus: u8) -> String {
    match target {
        FaderTarget::Channel(channel) => format!("/ch/{channel:02}/mix/{bus:02}/level"),
        FaderTarget::Aux(aux) => format!("/auxin/{aux:02}/mix/{bus:02}/level"),
        FaderTarget::Bus(bus_target) => format!("/bus/{bus_target:02}/mix/{bus:02}/level"),
        FaderTarget::FxRtn(fx) => format!("/fxrtn/{fx:02}/mix/{bus:02}/level"),
        FaderTarget::Mtx(_) | FaderTarget::Dca(_) => String::new(),
        FaderTarget::Main => format!("/main/st/mix/{bus:02}/level"),
    }
}

fn mute_path(target: FaderTarget) -> String {
    match target {
        FaderTarget::Channel(channel) => format!("/ch/{channel:02}/mix/on"),
        FaderTarget::Aux(aux) => format!("/auxin/{aux:02}/mix/on"),
        FaderTarget::Bus(bus) => format!("/bus/{bus:02}/mix/on"),
        FaderTarget::FxRtn(fx) => format!("/fxrtn/{fx:02}/mix/on"),
        FaderTarget::Mtx(mtx) => format!("/mtx/{mtx:02}/mix/on"),
        FaderTarget::Dca(dca) => format!("/dca/{dca}/on"),
        FaderTarget::Main => "/main/st/mix/on".to_owned(),
    }
}

fn solo_path(target: FaderTarget) -> String {
    let id = match target {
        FaderTarget::Channel(channel) => channel,
        FaderTarget::Aux(aux) => 32 + aux,
        FaderTarget::FxRtn(fx) => 40 + fx,
        FaderTarget::Bus(bus) => 48 + bus,
        FaderTarget::Mtx(_) | FaderTarget::Dca(_) | FaderTarget::Main => 0,
    };
    format!("/-stat/solosw/{id:02}")
}

fn name_path(target: FaderTarget) -> String {
    match target {
        FaderTarget::Channel(channel) => format!("/ch/{channel:02}/config/name"),
        FaderTarget::Aux(aux) => format!("/auxin/{aux:02}/config/name"),
        FaderTarget::Bus(bus) => format!("/bus/{bus:02}/config/name"),
        FaderTarget::FxRtn(fx) => format!("/fxrtn/{fx:02}/config/name"),
        FaderTarget::Mtx(mtx) => format!("/mtx/{mtx:02}/config/name"),
        FaderTarget::Dca(dca) => format!("/dca/{dca}/config/name"),
        FaderTarget::Main => "/main/st/config/name".to_owned(),
    }
}

fn color_path(target: FaderTarget) -> String {
    match target {
        FaderTarget::Channel(channel) => format!("/ch/{channel:02}/config/color"),
        FaderTarget::Aux(aux) => format!("/auxin/{aux:02}/config/color"),
        FaderTarget::Bus(bus) => format!("/bus/{bus:02}/config/color"),
        FaderTarget::FxRtn(fx) => format!("/fxrtn/{fx:02}/config/color"),
        FaderTarget::Mtx(mtx) => format!("/mtx/{mtx:02}/config/color"),
        FaderTarget::Dca(dca) => format!("/dca/{dca}/config/color"),
        FaderTarget::Main => "/main/st/config/color".to_owned(),
    }
}

fn osc_address(packet: &[u8]) -> Option<&str> {
    let end = packet.iter().position(|byte| *byte == 0)?;
    std::str::from_utf8(&packet[..end]).ok()
}

fn osc_meter_group_request(meter_id: &str) -> Vec<u8> {
    let mut packet = osc_string("/meters");
    packet.extend_from_slice(b",s\0\0");
    packet.extend_from_slice(&osc_string(meter_id));
    packet
}

pub fn batchsubscribe_meter_request(
    alias: &str,
    meter_id: &str,
    arg0: i32,
    arg1: i32,
    time_factor: i32,
) -> Vec<u8> {
    let mut packet = osc_string("/batchsubscribe");
    packet.extend_from_slice(b",ssiii\0\0");
    packet.extend_from_slice(&osc_string(alias));
    packet.extend_from_slice(&osc_string(meter_id));
    packet.extend_from_slice(&arg0.to_be_bytes());
    packet.extend_from_slice(&arg1.to_be_bytes());
    packet.extend_from_slice(&time_factor.to_be_bytes());
    packet
}

pub fn renew_request(alias: &str) -> Vec<u8> {
    let mut packet = osc_string("/renew");
    packet.extend_from_slice(b",s\0\0");
    packet.extend_from_slice(&osc_string(alias));
    packet
}

pub fn parse_console_update(packet: &[u8]) -> Option<ConsoleUpdate> {
    if let Some((path, value)) = parse_gain_value(packet)
        && let Some(target) = target_from_channel_path(&path, GAIN_RESPONSE_SUFFIX)
    {
        return Some(ConsoleUpdate::Gain(StripGain {
            target,
            value: decode_trim_gain(value),
            source: GainSource::Trim,
        }));
    }

    if let Some((path, value)) = parse_headamp_gain_value(packet)
        && let Some(index) = headamp_index_from_gain_path(&path)
    {
        return Some(ConsoleUpdate::HeadampGain {
            index,
            value: decode_headamp_gain(value),
        });
    }

    if let Some((path, value)) = parse_fader_value(packet)
        && let Some(target) = target_from_channel_path(&path, FADER_RESPONSE_SUFFIX)
    {
        return Some(ConsoleUpdate::Fader(StripFader { target, value }));
    }

    if let Some((path, value)) = parse_pan_value(packet)
        && let Some(target) = target_from_channel_path(&path, PAN_RESPONSE_SUFFIX)
    {
        return Some(ConsoleUpdate::Pan(StripPan { target, value }));
    }

    if let Some((target, bus, value)) = parse_send_update(packet) {
        return Some(ConsoleUpdate::Send(StripSend { target, bus, value }));
    }

    if let Some((path, on)) = parse_switch_value(packet) {
        if let Some(target) = target_from_channel_path(&path, MUTE_RESPONSE_SUFFIX) {
            return Some(ConsoleUpdate::Mute(StripMute { target, on }));
        }
        if let Some(target) = target_from_solo_path(&path) {
            return Some(ConsoleUpdate::Solo(StripSolo { target, on }));
        }
    }

    if let Some((path, value)) = parse_string_value(packet)
        && let Some(target) = target_from_channel_path(&path, NAME_RESPONSE_SUFFIX)
    {
        return Some(ConsoleUpdate::Name(StripName { target, value }));
    }

    if let Some((path, value)) = parse_color_value(packet)
        && let Some(target) = target_from_channel_path(&path, COLOR_RESPONSE_SUFFIX)
    {
        return Some(ConsoleUpdate::Color(StripColor { target, value }));
    }

    None
}

fn osc_query(address: &str) -> Vec<u8> {
    osc_string(address)
}

fn osc_float_message(address: &str, value: f32) -> Vec<u8> {
    let mut packet = osc_string(address);
    packet.extend_from_slice(b",f\0\0");
    packet.extend_from_slice(&value.to_bits().to_be_bytes());
    packet
}

fn osc_int_message(address: &str, value: i32) -> Vec<u8> {
    let mut packet = osc_string(address);
    packet.extend_from_slice(b",i\0\0");
    packet.extend_from_slice(&value.to_be_bytes());
    packet
}

fn osc_string(value: &str) -> Vec<u8> {
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(0);
    while bytes.len() % 4 != 0 {
        bytes.push(0);
    }
    bytes
}

fn target_from_channel_path(path: &str, suffix: &str) -> Option<FaderTarget> {
    if let Some(index) = path
        .strip_prefix("/ch/")
        .and_then(|rest| rest.strip_suffix(suffix))
    {
        return index.parse::<u8>().ok().map(FaderTarget::Channel);
    }

    if let Some(index) = path
        .strip_prefix("/auxin/")
        .and_then(|rest| rest.strip_suffix(suffix))
    {
        return index.parse::<u8>().ok().map(FaderTarget::Aux);
    }

    if let Some(index) = path
        .strip_prefix("/bus/")
        .and_then(|rest| rest.strip_suffix(suffix))
    {
        return index.parse::<u8>().ok().map(FaderTarget::Bus);
    }

    if let Some(index) = path
        .strip_prefix("/fxrtn/")
        .and_then(|rest| rest.strip_suffix(suffix))
    {
        return index.parse::<u8>().ok().map(FaderTarget::FxRtn);
    }

    if let Some(index) = path
        .strip_prefix("/mtx/")
        .and_then(|rest| rest.strip_suffix(suffix))
    {
        return index.parse::<u8>().ok().map(FaderTarget::Mtx);
    }

    // DCA paths have a different structure without /mix/ prefix
    if suffix == FADER_RESPONSE_SUFFIX {
        if let Some(index) = path.strip_prefix("/dca/").and_then(|rest| rest.strip_suffix("/fader")) {
            return index.parse::<u8>().ok().map(FaderTarget::Dca);
        }
    }
    if suffix == MUTE_RESPONSE_SUFFIX {
        if let Some(index) = path.strip_prefix("/dca/").and_then(|rest| rest.strip_suffix("/mix/on")) {
            return index.parse::<u8>().ok().map(FaderTarget::Dca);
        }
        if let Some(index) = path.strip_prefix("/dca/").and_then(|rest| rest.strip_suffix("/on")) {
            return index.parse::<u8>().ok().map(FaderTarget::Dca);
        }
    }
    if suffix == NAME_RESPONSE_SUFFIX {
        if let Some(index) = path.strip_prefix("/dca/").and_then(|rest| rest.strip_suffix("/config/name")) {
            return index.parse::<u8>().ok().map(FaderTarget::Dca);
        }
    }

    if path == format!("/main/st{suffix}") {
        return Some(FaderTarget::Main);
    }

    None
}

fn target_from_solo_path(path: &str) -> Option<FaderTarget> {
    let id = path.strip_prefix(SOLO_RESPONSE_PREFIX)?.parse::<u8>().ok()?;
    match id {
        1..=32 => Some(FaderTarget::Channel(id)),
        33..=40 => Some(FaderTarget::Aux(id - 32)),
        41..=48 => Some(FaderTarget::FxRtn(id - 40)),
        49..=64 => Some(FaderTarget::Bus(id - 48)),
        _ => None,
    }
}

fn target_and_bus_from_send_path(path: &str) -> Option<(FaderTarget, u8)> {
    let (target, rest) = if let Some(rest) = path.strip_prefix("/ch/") {
        let (channel, rest) = rest.split_once('/')?;
        (FaderTarget::Channel(channel.parse::<u8>().ok()?), rest)
    } else if let Some(rest) = path.strip_prefix("/auxin/") {
        let (aux, rest) = rest.split_once('/')?;
        (FaderTarget::Aux(aux.parse::<u8>().ok()?), rest)
    } else if let Some(rest) = path.strip_prefix("/bus/") {
        let (bus, rest) = rest.split_once('/')?;
        (FaderTarget::Bus(bus.parse::<u8>().ok()?), rest)
    } else if let Some(rest) = path.strip_prefix("/fxrtn/") {
        let (fx, rest) = rest.split_once('/')?;
        (FaderTarget::FxRtn(fx.parse::<u8>().ok()?), rest)
    } else {
        return None;
    };

    let rest = rest.strip_prefix("mix/")?;
    let (bus, tail) = rest.split_once('/')?;
    if tail != "level" {
        return None;
    }

    let bus = bus.parse::<u8>().ok()?;
    if !(1..=16).contains(&bus) {
        return None;
    }

    Some((target, bus))
}

fn parse_fader_value(packet: &[u8]) -> Option<(String, f32)> {
    if let Some(result) = parse_float_value(packet, FADER_RESPONSE_SUFFIX) {
        return Some(result);
    }
    let (path, value) = parse_float_value(packet, "/fader")?;
    if !path.starts_with("/dca/") {
        return None;
    }
    Some((path, value))
}

fn parse_pan_value(packet: &[u8]) -> Option<(String, f32)> {
    parse_float_value(packet, PAN_RESPONSE_SUFFIX)
}

fn parse_gain_value(packet: &[u8]) -> Option<(String, f32)> {
    parse_float_value(packet, GAIN_RESPONSE_SUFFIX)
}

fn parse_headamp_gain_value(packet: &[u8]) -> Option<(String, f32)> {
    let (path, value) = parse_float_value(packet, HEADAMP_GAIN_RESPONSE_SUFFIX)?;
    path.starts_with("/headamp/").then_some((path, value))
}

fn parse_headamp_index_value(packet: &[u8]) -> Option<(String, i32)> {
    let (path, value) = parse_int_value(packet)?;
    if path.starts_with("/-ha/") && path.ends_with(HEADAMP_INDEX_RESPONSE_SUFFIX) {
        Some((path, value))
    } else {
        None
    }
}

fn parse_send_value(packet: &[u8]) -> Option<(String, f32)> {
    let (path, value) = parse_float_value(packet, "/level")?;
    target_and_bus_from_send_path(&path)?;
    Some((path, value))
}

fn parse_send_update(packet: &[u8]) -> Option<(FaderTarget, u8, f32)> {
    let (path, value) = parse_send_value(packet)?;
    let (target, bus) = target_and_bus_from_send_path(&path)?;
    Some((target, bus, value))
}

fn parse_float_value(packet: &[u8], suffix: &str) -> Option<(String, f32)> {
    let path = osc_address(packet)?;
    if !path.ends_with(suffix) {
        return None;
    }

    let mut offset = osc_padded_len(packet)?;
    let type_tag_end = packet.get(offset..)?.iter().position(|byte| *byte == 0)?;
    let type_tag = std::str::from_utf8(packet.get(offset..offset + type_tag_end)?).ok()?;
    let type_tag_len = osc_padded_len(packet.get(offset..)?)?;
    offset += type_tag_len;

    if type_tag != ",f" {
        return None;
    }

    let value_bytes: [u8; 4] = packet.get(offset..offset + 4)?.try_into().ok()?;
    Some((path.to_owned(), f32::from_bits(u32::from_be_bytes(value_bytes))))
}

fn parse_int_value(packet: &[u8]) -> Option<(String, i32)> {
    let path = osc_address(packet)?;

    let mut offset = osc_padded_len(packet)?;
    let type_tag_end = packet.get(offset..)?.iter().position(|byte| *byte == 0)?;
    let type_tag = std::str::from_utf8(packet.get(offset..offset + type_tag_end)?).ok()?;
    let type_tag_len = osc_padded_len(packet.get(offset..)?)?;
    offset += type_tag_len;

    if type_tag != ",i" {
        return None;
    }

    let value_bytes: [u8; 4] = packet.get(offset..offset + 4)?.try_into().ok()?;
    Some((path.to_owned(), i32::from_be_bytes(value_bytes)))
}

fn decode_headamp_gain(raw: f32) -> f32 {
    quantize_gain_step(raw.clamp(0.0, 1.0) * 72.0 - 12.0, -12.0, 0.1)
}

fn encode_headamp_gain(db: f32) -> f32 {
    ((quantize_gain_step(db, -12.0, 0.1) + 12.0) / 72.0).clamp(0.0, 1.0)
}

fn decode_trim_gain(raw: f32) -> f32 {
    quantize_gain_step(raw.clamp(0.0, 1.0) * 36.0 - 18.0, -18.0, 0.25)
}

fn encode_trim_gain(db: f32) -> f32 {
    ((quantize_gain_step(db, -18.0, 0.25) + 18.0) / 36.0).clamp(0.0, 1.0)
}

fn quantize_gain_step(value: f32, min: f32, step: f32) -> f32 {
    let steps = ((value - min) / step).round();
    min + steps * step
}

fn parse_switch_value(packet: &[u8]) -> Option<(String, bool)> {
    let path = osc_address(packet)?;
    if !path.ends_with(MUTE_RESPONSE_SUFFIX)
        && !path.starts_with(SOLO_RESPONSE_PREFIX)
        && !is_dca_mute_path(&path)
    {
        return None;
    }

    let mut offset = osc_padded_len(packet)?;
    let type_tag_end = packet.get(offset..)?.iter().position(|byte| *byte == 0)?;
    let type_tag = std::str::from_utf8(packet.get(offset..offset + type_tag_end)?).ok()?;
    let type_tag_len = osc_padded_len(packet.get(offset..)?)?;
    offset += type_tag_len;

    match type_tag {
        ",i" => {
            let value_bytes: [u8; 4] = packet.get(offset..offset + 4)?.try_into().ok()?;
            Some((path.to_owned(), i32::from_be_bytes(value_bytes) != 0))
        }
        ",f" => {
            let value_bytes: [u8; 4] = packet.get(offset..offset + 4)?.try_into().ok()?;
            Some((path.to_owned(), f32::from_be_bytes(value_bytes) != 0.0))
        }
        _ => None,
    }
}

fn is_dca_mute_path(path: &str) -> bool {
    path.strip_prefix("/dca/")
        .and_then(|rest| rest.strip_suffix("/mix/on").or_else(|| rest.strip_suffix("/on")))
        .and_then(|index| index.parse::<u8>().ok())
        .is_some()
}

fn parse_string_value(packet: &[u8]) -> Option<(String, String)> {
    let path = osc_address(packet)?;
    if !path.ends_with(NAME_RESPONSE_SUFFIX) {
        return None;
    }

    let mut offset = osc_padded_len(packet)?;
    let type_tag_end = packet.get(offset..)?.iter().position(|byte| *byte == 0)?;
    let type_tag = std::str::from_utf8(packet.get(offset..offset + type_tag_end)?).ok()?;
    let type_tag_len = osc_padded_len(packet.get(offset..)?)?;
    offset += type_tag_len;

    if type_tag != ",s" {
        return None;
    }

    let value_bytes = packet.get(offset..)?;
    let value_end = value_bytes.iter().position(|byte| *byte == 0)?;
    let value = std::str::from_utf8(&value_bytes[..value_end]).ok()?;
    Some((path.to_owned(), value.to_owned()))
}

fn parse_color_value(packet: &[u8]) -> Option<(String, u8)> {
    let path = osc_address(packet)?;
    if !path.ends_with(COLOR_RESPONSE_SUFFIX) {
        return None;
    }

    let mut offset = osc_padded_len(packet)?;
    let type_tag_end = packet.get(offset..)?.iter().position(|byte| *byte == 0)?;
    let type_tag = std::str::from_utf8(packet.get(offset..offset + type_tag_end)?).ok()?;
    let type_tag_len = osc_padded_len(packet.get(offset..)?)?;
    offset += type_tag_len;

    if type_tag != ",i" {
        return None;
    }

    let value_bytes: [u8; 4] = packet.get(offset..offset + 4)?.try_into().ok()?;
    let value = i32::from_be_bytes(value_bytes).clamp(0, 15) as u8;
    Some((path.to_owned(), value))
}

pub fn parse_input_meter_packet(packet: &[u8]) -> Result<Vec<StripMeter>, ProbeError> {
    let floats = parse_meter_blob(packet, INPUT_METERS_REQUEST, INPUT_METERS_ALIAS)?;

    let mut strips = Vec::with_capacity(48);
    for index in 0..48 {
        let target = if index < 32 {
            FaderTarget::Channel((index + 1) as u8)
        } else if index < 40 {
            FaderTarget::Aux((index - 31) as u8)
        } else {
            FaderTarget::FxRtn((index - 39) as u8)
        };
        let start = index * 4;
        let bytes: [u8; 4] = floats[start..start + 4]
            .try_into()
            .map_err(|_| ProbeError::Protocol("meter float slice size mismatch".to_owned()))?;
        strips.push(StripMeter {
            target,
            level_linear: f32::from_le_bytes(bytes),
        });
    }
    Ok(strips)
}

#[derive(Debug, Clone, Copy)]
pub struct MainMeterLevels {
    pub main_lr: [f32; 2],
    pub matrices: [f32; 6],
}

pub fn parse_main_meter_packet(packet: &[u8]) -> Result<MainMeterLevels, ProbeError> {
    let floats = parse_meter_blob(packet, MAIN_METERS_REQUEST, MAIN_METERS_ALIAS)?;
    if floats.len() < 24 * 4 {
        return Err(ProbeError::Protocol(
            "main meter blob is shorter than expected".to_owned(),
        ));
    }

    let mut matrices = [0.0f32; 6];
    for i in 0..6 {
        matrices[i] = f32::from_le_bytes(floats[(16 + i) * 4..(16 + i) * 4 + 4].try_into().map_err(|_| {
            ProbeError::Protocol(format!("matrix meter {i} float slice size mismatch"))
        })?);
    }

    Ok(MainMeterLevels {
        main_lr: [
            f32::from_le_bytes(floats[22 * 4..22 * 4 + 4].try_into().map_err(|_| {
                ProbeError::Protocol("main L meter float slice size mismatch".to_owned())
            })?),
            f32::from_le_bytes(floats[23 * 4..23 * 4 + 4].try_into().map_err(|_| {
                ProbeError::Protocol("main R meter float slice size mismatch".to_owned())
            })?),
        ],
        matrices,
    })
}

fn parse_meter_blob<'a>(
    packet: &'a [u8],
    expected_path: &str,
    expected_alias: &str,
) -> Result<&'a [u8], ProbeError> {
    let path = osc_address(packet)
        .ok_or_else(|| ProbeError::Protocol("meter reply missing OSC address".to_owned()))?;
    if path != expected_path && path != expected_alias {
        return Err(ProbeError::Protocol(format!(
            "unexpected meter reply path '{path}'"
        )));
    }

    let mut offset = osc_padded_len(packet)
        .ok_or_else(|| ProbeError::Protocol("meter reply has invalid OSC address".to_owned()))?;
    let type_tag_end = packet[offset..]
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| ProbeError::Protocol("meter reply missing OSC type tag".to_owned()))?;
    let type_tag = std::str::from_utf8(&packet[offset..offset + type_tag_end])
        .map_err(|_| ProbeError::Protocol("meter reply type tag is not UTF-8".to_owned()))?;
    if type_tag != ",b" {
        return Err(ProbeError::Protocol(format!(
            "unexpected meter reply type tag '{type_tag}'"
        )));
    }
    offset += osc_padded_len(&packet[offset..])
        .ok_or_else(|| ProbeError::Protocol("meter reply has invalid type tag".to_owned()))?;

    let blob_len = read_be_u32(packet, offset)? as usize;
    offset += 4;
    let blob = packet
        .get(offset..offset + blob_len)
        .ok_or_else(|| ProbeError::Protocol("meter blob length exceeds packet size".to_owned()))?;
    if blob.len() < 4 {
        return Err(ProbeError::Protocol(
            "meter blob is missing float-count header".to_owned(),
        ));
    }

    let float_count = u32::from_le_bytes(
        blob[0..4]
            .try_into()
            .map_err(|_| ProbeError::Protocol("meter float-count size mismatch".to_owned()))?,
    ) as usize;
    let floats = &blob[4..];

    if floats.len() < float_count * 4 {
        return Err(ProbeError::Protocol(
            "meter blob is shorter than advertised float count".to_owned(),
        ));
    }

    Ok(floats)
}

fn read_be_u32(packet: &[u8], offset: usize) -> Result<u32, ProbeError> {
    let bytes: [u8; 4] = packet
        .get(offset..offset + 4)
        .ok_or_else(|| ProbeError::Protocol("packet truncated while reading u32".to_owned()))?
        .try_into()
        .map_err(|_| ProbeError::Protocol("u32 slice size mismatch".to_owned()))?;
    Ok(u32::from_be_bytes(bytes))
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

    #[test]
    fn builds_query_packet_for_channel_fader() {
        assert_eq!(
            osc_query(&fader_path(FaderTarget::Channel(1))),
            b"/ch/01/mix/fader\0\0\0\0".to_vec()
        );
    }

    #[test]
    fn parses_float_fader_reply() {
        let packet = [
            b"/ch/01/mix/fader\0\0\0\0".as_slice(),
            b",f\0\0".as_slice(),
            0.75_f32.to_bits().to_be_bytes().as_slice(),
        ]
        .concat();

        let (path, value) = parse_fader_value(&packet).expect("should parse fader reply");
        assert_eq!(path, "/ch/01/mix/fader");
        assert!((value - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_float_pan_reply() {
        let packet = [
            osc_string("/auxin/05/mix/pan").as_slice(),
            b",f\0\0".as_slice(),
            0.25_f32.to_bits().to_be_bytes().as_slice(),
        ]
        .concat();

        let (path, value) = parse_pan_value(&packet).expect("should parse pan reply");
        assert_eq!(path, "/auxin/05/mix/pan");
        assert!((value - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_float_gain_reply() {
        let packet = [
            osc_string("/ch/02/preamp/trim").as_slice(),
            b",f\0\0".as_slice(),
            (-6.0_f32).to_bits().to_be_bytes().as_slice(),
        ]
        .concat();

        let (path, value) = parse_gain_value(&packet).expect("should parse gain reply");
        assert_eq!(path, "/ch/02/preamp/trim");
        assert!((value + 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_float_send_reply() {
        let packet = [
            osc_string("/ch/02/mix/16/level").as_slice(),
            b",f\0\0".as_slice(),
            0.5_f32.to_bits().to_be_bytes().as_slice(),
        ]
        .concat();

        let (path, value) = parse_send_value(&packet).expect("should parse send reply");
        assert_eq!(path, "/ch/02/mix/16/level");
        assert!((value - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_int_mute_reply() {
        let packet = osc_int_message("/auxin/05/mix/on", 0);

        let (path, on) = parse_switch_value(&packet).expect("should parse mute reply");
        assert_eq!(path, "/auxin/05/mix/on");
        assert!(!on);
    }

    #[test]
    fn parses_int_solo_reply() {
        let packet = osc_int_message("/-stat/solosw/37", 1);

        let (path, on) = parse_switch_value(&packet).expect("should parse solo reply");
        assert_eq!(path, "/-stat/solosw/37");
        assert!(on);
    }

    #[test]
    fn parses_string_name_reply() {
        let packet = [
            osc_string("/auxin/05/config/name").as_slice(),
            b",s\0\0".as_slice(),
            osc_string("Lead Vox").as_slice(),
        ]
        .concat();

        let (path, value) = parse_string_value(&packet).expect("should parse name reply");
        assert_eq!(path, "/auxin/05/config/name");
        assert_eq!(value, "Lead Vox");
    }

    #[test]
    fn builds_meter_request_packet() {
        let packet = osc_meter_group_request(INPUT_METERS_REQUEST);
        assert_eq!(&packet[..8], b"/meters\0");
        assert_eq!(&packet[8..12], b",s\0\0");
        assert_eq!(&packet[12..24], b"/meters/0\0\0\0");
    }

    #[test]
    fn parses_input_meter_blob() {
        let mut floats = Vec::new();
        for i in 0..82 {
            floats.extend_from_slice(&((i as f32) / 10.0).to_le_bytes());
        }
        let mut blob = Vec::new();
        blob.extend_from_slice(&(82_u32).to_le_bytes());
        blob.extend_from_slice(&floats);

        let mut packet = osc_string(INPUT_METERS_ALIAS);
        packet.extend_from_slice(b",b\0\0");
        packet.extend_from_slice(&(blob.len() as u32).to_be_bytes());
        packet.extend_from_slice(&blob);

        let meters = parse_input_meter_packet(&packet).expect("should parse input meter blob");
        assert_eq!(meters.len(), 48);
        assert_eq!(meters[0].target, FaderTarget::Channel(1));
        assert_eq!(meters[31].target, FaderTarget::Channel(32));
        assert_eq!(meters[32].target, FaderTarget::Aux(1));
        assert_eq!(meters[39].target, FaderTarget::Aux(8));
        assert_eq!(meters[40].target, FaderTarget::FxRtn(1));
        assert_eq!(meters[47].target, FaderTarget::FxRtn(8));
        assert!((meters[5].level_linear - 0.5).abs() < f32::EPSILON);
        assert!((meters[35].level_linear - 3.5).abs() < f32::EPSILON);
    }

    #[test]
    fn builds_batchsubscribe_meter_request_packet() {
        let packet = batchsubscribe_meter_request("meters/0", "/meters/0", 0, 0, 1);
        assert_eq!(&packet[..16], b"/batchsubscribe\0");
        assert_eq!(&packet[16..24], b",ssiii\0\0");
    }

    #[test]
    fn builds_renew_request_packet() {
        let packet = renew_request("meters/0");
        assert_eq!(&packet[..8], b"/renew\0\0");
    }

    #[test]
    fn builds_query_packet_for_bus_fader() {
        assert_eq!(
            osc_query(&fader_path(FaderTarget::Bus(1))),
            b"/bus/01/mix/fader\0\0\0".to_vec()
        );
    }

    #[test]
    fn parses_bus_fader_reply() {
        let packet = [
            osc_string("/bus/05/mix/fader").as_slice(),
            b",f\0\0".as_slice(),
            0.75_f32.to_bits().to_be_bytes().as_slice(),
        ]
        .concat();

        let (path, value) = parse_fader_value(&packet).expect("should parse bus fader reply");
        assert_eq!(path, "/bus/05/mix/fader");
        assert!((value - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_bus_pan_reply() {
        let packet = [
            osc_string("/bus/03/mix/pan").as_slice(),
            b",f\0\0".as_slice(),
            0.25_f32.to_bits().to_be_bytes().as_slice(),
        ]
        .concat();

        let (path, value) = parse_pan_value(&packet).expect("should parse bus pan reply");
        assert_eq!(path, "/bus/03/mix/pan");
        assert!((value - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_bus_send_reply() {
        let packet = [
            osc_string("/bus/02/mix/06/level").as_slice(),
            b",f\0\0".as_slice(),
            0.5_f32.to_bits().to_be_bytes().as_slice(),
        ]
        .concat();

        let (path, value) = parse_send_value(&packet).expect("should parse bus send reply");
        assert_eq!(path, "/bus/02/mix/06/level");
        assert!((value - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_bus_mute_reply() {
        let packet = osc_int_message("/bus/07/mix/on", 0);

        let (path, on) = parse_switch_value(&packet).expect("should parse bus mute reply");
        assert_eq!(path, "/bus/07/mix/on");
        assert!(!on);
    }

    #[test]
    fn parses_bus_solo_reply() {
        let packet = osc_int_message("/-stat/solosw/52", 1);

        let (path, on) = parse_switch_value(&packet).expect("should parse bus solo reply");
        assert_eq!(path, "/-stat/solosw/52");
        assert!(on);
    }

    #[test]
    fn parses_bus_name_reply() {
        let packet = [
            osc_string("/bus/08/config/name").as_slice(),
            b",s\0\0".as_slice(),
            osc_string("Drums").as_slice(),
        ]
        .concat();

        let (path, value) = parse_string_value(&packet).expect("should parse bus name reply");
        assert_eq!(path, "/bus/08/config/name");
        assert_eq!(value, "Drums");
    }

    #[test]
    fn parses_dca_mute_reply_with_on_suffix() {
        let packet = osc_int_message("/dca/3/on", 0);

        let (path, on) = parse_switch_value(&packet).expect("should parse DCA mute reply");
        assert_eq!(path, "/dca/3/on");
        assert!(!on);
    }

    #[test]
    fn parses_dca_mute_reply_with_mix_on_suffix() {
        let packet = osc_int_message("/dca/5/mix/on", 1);

        let (path, on) = parse_switch_value(&packet).expect("should parse DCA /mix/on mute reply");
        assert_eq!(path, "/dca/5/mix/on");
        assert!(on);
    }

    #[test]
    fn parses_dca_mute_reply_as_float() {
        let packet = [
            osc_string("/dca/2/on").as_slice(),
            b",f\0\0".as_slice(),
            1.0_f32.to_bits().to_be_bytes().as_slice(),
        ]
        .concat();

        let (path, on) = parse_switch_value(&packet).expect("should parse DCA float mute reply");
        assert_eq!(path, "/dca/2/on");
        assert!(on);
    }

    #[test]
    fn parses_fxrtn_mute_reply_as_float() {
        let packet = [
            osc_string("/fxrtn/03/mix/on").as_slice(),
            b",f\0\0".as_slice(),
            0.0_f32.to_bits().to_be_bytes().as_slice(),
        ]
        .concat();

        let (path, on) = parse_switch_value(&packet).expect("should parse FX return float mute reply");
        assert_eq!(path, "/fxrtn/03/mix/on");
        assert!(!on);
    }

    #[test]
    fn parses_dca_mute_console_update() {
        let packet = osc_int_message("/dca/4/on", 1);
        let update = parse_console_update(&packet).expect("should parse DCA mute update");
        assert_eq!(
            update,
            ConsoleUpdate::Mute(StripMute {
                target: FaderTarget::Dca(4),
                on: true,
            })
        );
    }

    #[test]
    fn parses_dca_mix_on_mute_console_update() {
        let packet = osc_int_message("/dca/6/mix/on", 0);
        let update = parse_console_update(&packet).expect("should parse DCA /mix/on mute update");
        assert_eq!(
            update,
            ConsoleUpdate::Mute(StripMute {
                target: FaderTarget::Dca(6),
                on: false,
            })
        );
    }
}

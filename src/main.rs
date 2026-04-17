use iced::futures::sink::SinkExt;
use iced::futures::{channel::mpsc, stream::BoxStream, StreamExt};
use iced::stream;
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Background, Border, Color, Element, Fill, Length, Subscription, Task, Theme, time};
use maolan_widgets::horizontal_slider::horizontal_slider;
use maolan_widgets::meters::meters;
use maolan_widgets::slider::slider as vertical_slider;
use maolan_widgets::ticks::meter_ticks;
use mixosc::{
    ConnectionProbe, ConsoleUpdate, DiscoveredMixer, DiscoveryProbe, FaderBankProbe, FaderTarget,
    GainBankProbe, GainSource, MuteBankProbe, NameBankProbe, PanBankProbe, ProbeOutcome,
    ProbeResponse, SendBankProbe, SoloBankProbe, StripFader, StripGain, StripMeter, StripMute,
    StripName, StripPan, StripSend, StripSolo, XREMOTE_REQUEST, batchsubscribe_meter_request,
    parse_console_update, parse_input_meter_packet, parse_target, renew_request,
};
use std::env;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::{Instant, sleep};

const STRIP_COUNT: usize = 40;
const SEND_BUS_COUNT: usize = 16;
const STRIP_METER_HEIGHT: f32 = 260.0;
const SEND_BUSES: [u8; SEND_BUS_COUNT] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
const VISIBLE_STRIPS: [FaderTarget; STRIP_COUNT] = [
    FaderTarget::Channel(1),
    FaderTarget::Channel(2),
    FaderTarget::Channel(3),
    FaderTarget::Channel(4),
    FaderTarget::Channel(5),
    FaderTarget::Channel(6),
    FaderTarget::Channel(7),
    FaderTarget::Channel(8),
    FaderTarget::Channel(9),
    FaderTarget::Channel(10),
    FaderTarget::Channel(11),
    FaderTarget::Channel(12),
    FaderTarget::Channel(13),
    FaderTarget::Channel(14),
    FaderTarget::Channel(15),
    FaderTarget::Channel(16),
    FaderTarget::Channel(17),
    FaderTarget::Channel(18),
    FaderTarget::Channel(19),
    FaderTarget::Channel(20),
    FaderTarget::Channel(21),
    FaderTarget::Channel(22),
    FaderTarget::Channel(23),
    FaderTarget::Channel(24),
    FaderTarget::Channel(25),
    FaderTarget::Channel(26),
    FaderTarget::Channel(27),
    FaderTarget::Channel(28),
    FaderTarget::Channel(29),
    FaderTarget::Channel(30),
    FaderTarget::Channel(31),
    FaderTarget::Channel(32),
    FaderTarget::Aux(1),
    FaderTarget::Aux(2),
    FaderTarget::Aux(3),
    FaderTarget::Aux(4),
    FaderTarget::Aux(5),
    FaderTarget::Aux(6),
    FaderTarget::Aux(7),
    FaderTarget::Aux(8),
];

fn main() -> iced::Result {
    iced::application(new, update, view)
        .subscription(subscription)
        .theme(theme)
        .window_size(iced::Size::new(720.0, 360.0))
        .run()
}

#[derive(Debug)]
struct StatusApp {
    mixer_addr: Option<SocketAddr>,
    discovered_mixer: Option<DiscoveredMixer>,
    manual_target: bool,
    probe_in_flight: bool,
    names: [Option<String>; STRIP_COUNT],
    gains: [Option<f32>; STRIP_COUNT],
    gain_sources: [GainSource; STRIP_COUNT],
    sends: [[Option<f32>; SEND_BUS_COUNT]; STRIP_COUNT],
    pans: [Option<f32>; STRIP_COUNT],
    faders: [Option<f32>; STRIP_COUNT],
    meters_db: [f32; STRIP_COUNT],
    muted: [Option<bool>; STRIP_COUNT],
    soloed: [Option<bool>; STRIP_COUNT],
    status: ConnectionStatus,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionStatus {
    Checking,
    Connected(ProbeResponse),
    Disconnected,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    ConsoleUpdateReceived(Result<ConsoleUpdate, String>),
    GainChanged(usize, f32),
    SendChanged(usize, usize, f32),
    PanChanged(usize, f32),
    FaderChanged(usize, f32),
    NamesLoaded(Result<Vec<StripName>, String>),
    GainsLoaded(Result<Vec<StripGain>, String>),
    SendsLoaded(Result<Vec<StripSend>, String>),
    PansLoaded(Result<Vec<StripPan>, String>),
    FadersLoaded(Result<Vec<StripFader>, String>),
    SendSetFinished(Result<(), String>),
    GainSetFinished(Result<(), String>),
    PanSetFinished(Result<(), String>),
    FaderSetFinished(Result<(), String>),
    MetersLoaded(Result<Vec<StripMeter>, String>),
    MutePressed(usize),
    MutesLoaded(Result<Vec<StripMute>, String>),
    MuteSetFinished(Result<(), String>),
    SoloPressed(usize),
    SolosLoaded(Result<Vec<StripSolo>, String>),
    SoloSetFinished(Result<(), String>),
    DiscoveryFinished(Result<Vec<DiscoveredMixer>, String>),
    ProbeFinished(Result<ProbeOutcome, String>),
}

fn new() -> (StatusApp, Task<Message>) {
    let maybe_target = mixer_addr_from_args_or_env();
    let app = StatusApp {
        mixer_addr: maybe_target,
        discovered_mixer: None,
        manual_target: maybe_target.is_some(),
        probe_in_flight: true,
        names: std::array::from_fn(|_| None),
        gains: [None; STRIP_COUNT],
        gain_sources: [GainSource::Trim; STRIP_COUNT],
        sends: [[None; SEND_BUS_COUNT]; STRIP_COUNT],
        pans: [None; STRIP_COUNT],
        faders: [None; STRIP_COUNT],
        meters_db: [-90.0; STRIP_COUNT],
        muted: [None; STRIP_COUNT],
        soloed: [None; STRIP_COUNT],
        status: ConnectionStatus::Checking,
        last_error: None,
    };

    let task = match maybe_target {
        Some(mixer_addr) => spawn_probe(mixer_addr),
        None => spawn_discovery(),
    };

    (app, task)
}

fn update(app: &mut StatusApp, message: Message) -> Task<Message> {
    match message {
        Message::Tick if app.probe_in_flight => Task::none(),
        Message::Tick => {
            app.probe_in_flight = true;
            match app.mixer_addr {
                Some(mixer_addr) => spawn_probe(mixer_addr),
                None => spawn_discovery(),
            }
        }
        Message::ConsoleUpdateReceived(result) => {
            match result {
                Ok(ConsoleUpdate::Gain(strip)) => {
                    if let Some(index) = VISIBLE_STRIPS
                        .iter()
                        .position(|target| *target == strip.target)
                    {
                        match (app.gain_sources[index], strip.source) {
                            (GainSource::Headamp(_), GainSource::Trim) => {}
                            _ => {
                                app.gains[index] = Some(strip.value);
                                app.gain_sources[index] = strip.source;
                            }
                        }
                    }
                }
                Ok(ConsoleUpdate::HeadampGain { index: headamp_index, value }) => {
                    for (strip_index, source) in app.gain_sources.iter().enumerate() {
                        if *source == GainSource::Headamp(headamp_index) {
                            app.gains[strip_index] = Some(value);
                        }
                    }
                }
                Ok(ConsoleUpdate::Fader(strip)) => {
                    if let Some(index) = VISIBLE_STRIPS
                        .iter()
                        .position(|target| *target == strip.target)
                    {
                        app.faders[index] = Some(strip.value);
                    }
                }
                Ok(ConsoleUpdate::Pan(strip)) => {
                    if let Some(index) = VISIBLE_STRIPS
                        .iter()
                        .position(|target| *target == strip.target)
                    {
                        app.pans[index] = Some(strip.value);
                    }
                }
                Ok(ConsoleUpdate::Send(strip)) => {
                    if let Some(strip_index) = VISIBLE_STRIPS
                        .iter()
                        .position(|target| *target == strip.target)
                    {
                        let bus_index = usize::from(strip.bus.saturating_sub(1));
                        if let Some(send) = app.sends[strip_index].get_mut(bus_index) {
                            *send = Some(strip.value);
                        }
                    }
                }
                Ok(ConsoleUpdate::Mute(strip)) => {
                    if let Some(index) = VISIBLE_STRIPS
                        .iter()
                        .position(|target| *target == strip.target)
                    {
                        app.muted[index] = Some(!strip.on);
                    }
                }
                Ok(ConsoleUpdate::Solo(strip)) => {
                    if let Some(index) = VISIBLE_STRIPS
                        .iter()
                        .position(|target| *target == strip.target)
                    {
                        app.soloed[index] = Some(strip.on);
                    }
                }
                Ok(ConsoleUpdate::Name(strip)) => {
                    if let Some(index) = VISIBLE_STRIPS
                        .iter()
                        .position(|target| *target == strip.target)
                    {
                        app.names[index] = if strip.value.trim().is_empty() {
                            None
                        } else {
                            Some(strip.value)
                        };
                    }
                }
                Err(error) => app.last_error = Some(error),
            }

            Task::none()
        }
        Message::GainChanged(index, value) => {
            let source = app.gain_sources[index];
            let value = quantize_gain_value(value, source);
            if let Some(gain) = app.gains.get_mut(index) {
                *gain = Some(value);
            }

            let Some(mixer_addr) = app.mixer_addr else {
                return Task::none();
            };
            let target = VISIBLE_STRIPS[index];
            spawn_set_gain(mixer_addr, target, source, value)
        }
        Message::SendChanged(strip_index, bus_index, value) => {
            if let Some(send) = app.sends[strip_index].get_mut(bus_index) {
                *send = Some(value);
            }

            let Some(mixer_addr) = app.mixer_addr else {
                return Task::none();
            };
            let target = VISIBLE_STRIPS[strip_index];
            let bus = SEND_BUSES[bus_index];
            spawn_set_send(mixer_addr, target, bus, value)
        }
        Message::PanChanged(index, value) => {
            if let Some(pan) = app.pans.get_mut(index) {
                *pan = Some(value);
            }

            let Some(mixer_addr) = app.mixer_addr else {
                return Task::none();
            };
            let target = VISIBLE_STRIPS[index];
            spawn_set_pan(mixer_addr, target, value)
        }
        Message::FaderChanged(index, value) => {
            if let Some(fader) = app.faders.get_mut(index) {
                *fader = Some(value);
            }

            let Some(mixer_addr) = app.mixer_addr else {
                return Task::none();
            };
            let target = VISIBLE_STRIPS[index];
            spawn_set_fader(mixer_addr, target, value)
        }
        Message::NamesLoaded(result) => {
            match result {
                Ok(names) => {
                    for strip in names {
                        if let Some(index) = VISIBLE_STRIPS
                            .iter()
                            .position(|target| *target == strip.target)
                        {
                            app.names[index] = if strip.value.is_empty() {
                                None
                            } else {
                                Some(strip.value)
                            };
                        }
                    }
                }
                Err(error) => app.last_error = Some(error),
            }

            Task::none()
        }
        Message::GainsLoaded(result) => {
            match result {
                Ok(gains) => {
                    for strip in gains {
                        if let Some(index) = VISIBLE_STRIPS
                            .iter()
                            .position(|target| *target == strip.target)
                        {
                            app.gains[index] = Some(strip.value);
                            app.gain_sources[index] = strip.source;
                        }
                    }
                }
                Err(error) => app.last_error = Some(error),
            }

            Task::none()
        }
        Message::SendsLoaded(result) => {
            match result {
                Ok(sends) => {
                    for strip in sends {
                        if let Some(strip_index) = VISIBLE_STRIPS
                            .iter()
                            .position(|target| *target == strip.target)
                        {
                            let bus_index = usize::from(strip.bus.saturating_sub(1));
                            if let Some(send) = app.sends[strip_index].get_mut(bus_index) {
                                *send = Some(strip.value);
                            }
                        }
                    }
                }
                Err(error) => app.last_error = Some(error),
            }

            Task::none()
        }
        Message::PansLoaded(result) => {
            match result {
                Ok(pans) => {
                    for strip in pans {
                        if let Some(index) = VISIBLE_STRIPS
                            .iter()
                            .position(|target| *target == strip.target)
                        {
                            app.pans[index] = Some(strip.value);
                        }
                    }
                }
                Err(error) => app.last_error = Some(error),
            }

            Task::none()
        }
        Message::FadersLoaded(result) => {
            match result {
                Ok(faders) => {
                    for fader in faders {
                        if let Some(index) = VISIBLE_STRIPS
                            .iter()
                            .position(|target| *target == fader.target)
                        {
                            app.faders[index] = Some(fader.value);
                        }
                    }
                }
                Err(error) => app.last_error = Some(error),
            }

            Task::none()
        }
        Message::FaderSetFinished(result) => {
            if let Err(error) = result {
                app.last_error = Some(error);
            }

            Task::none()
        }
        Message::PanSetFinished(result) => {
            if let Err(error) = result {
                app.last_error = Some(error);
            }

            Task::none()
        }
        Message::SendSetFinished(result) => {
            if let Err(error) = result {
                app.last_error = Some(error);
            }

            Task::none()
        }
        Message::GainSetFinished(result) => {
            if let Err(error) = result {
                app.last_error = Some(error);
            }

            Task::none()
        }
        Message::MetersLoaded(result) => {
            match result {
                Ok(meters) => {
                    for meter in meters {
                        if let Some(index) = VISIBLE_STRIPS
                            .iter()
                            .position(|target| *target == meter.target)
                        {
                            app.meters_db[index] = linear_meter_to_db(meter.level_linear);
                        }
                    }
                }
                Err(error) => app.last_error = Some(error),
            }

            Task::none()
        }
        Message::MutePressed(index) => {
            let Some(mixer_addr) = app.mixer_addr else {
                return Task::none();
            };
            let target = VISIBLE_STRIPS[index];
            let currently_muted = app.muted.get(index).and_then(|state| *state).unwrap_or(false);
            let next_on = currently_muted;
            if let Some(muted) = app.muted.get_mut(index) {
                *muted = Some(!next_on);
            }
            spawn_set_mute(mixer_addr, target, next_on)
        }
        Message::MutesLoaded(result) => {
            match result {
                Ok(mutes) => {
                    for strip in mutes {
                        if let Some(index) =
                            VISIBLE_STRIPS.iter().position(|target| *target == strip.target)
                        {
                            app.muted[index] = Some(!strip.on);
                        }
                    }
                }
                Err(error) => app.last_error = Some(error),
            }

            Task::none()
        }
        Message::MuteSetFinished(result) => {
            if let Err(error) = result {
                app.last_error = Some(error);
            }

            Task::none()
        }
        Message::SoloPressed(index) => {
            let Some(mixer_addr) = app.mixer_addr else {
                return Task::none();
            };
            let target = VISIBLE_STRIPS[index];
            let next_on = !app.soloed.get(index).and_then(|state| *state).unwrap_or(false);
            if let Some(soloed) = app.soloed.get_mut(index) {
                *soloed = Some(next_on);
            }
            spawn_set_solo(mixer_addr, target, next_on)
        }
        Message::SolosLoaded(result) => {
            match result {
                Ok(solos) => {
                    for strip in solos {
                        if let Some(index) =
                            VISIBLE_STRIPS.iter().position(|target| *target == strip.target)
                        {
                            app.soloed[index] = Some(strip.on);
                        }
                    }
                }
                Err(error) => app.last_error = Some(error),
            }

            Task::none()
        }
        Message::SoloSetFinished(result) => {
            if let Err(error) = result {
                app.last_error = Some(error);
            }

            Task::none()
        }
        Message::DiscoveryFinished(result) => {
            app.probe_in_flight = false;

            match result {
                Ok(mut mixers) => {
                    if let Some(mixer) = mixers.drain(..).next() {
                        app.mixer_addr = Some(mixer.addr);
                        app.discovered_mixer = Some(mixer.clone());
                        app.last_error = None;
                        app.probe_in_flight = true;
                        refresh_mixer(mixer.addr)
                    } else {
                        app.mixer_addr = None;
                        app.discovered_mixer = None;
                        app.names = std::array::from_fn(|_| None);
                        app.gains = [None; STRIP_COUNT];
                        app.gain_sources = [GainSource::Trim; STRIP_COUNT];
                        app.sends = [[None; SEND_BUS_COUNT]; STRIP_COUNT];
                        app.pans = [None; STRIP_COUNT];
                        app.faders = [None; STRIP_COUNT];
                        app.meters_db = [-90.0; STRIP_COUNT];
                        app.muted = [None; STRIP_COUNT];
                        app.soloed = [None; STRIP_COUNT];
                        app.status = ConnectionStatus::Disconnected;
                        app.last_error =
                            Some("no X32 mixer discovered on the local network".to_owned());
                        Task::none()
                    }
                }
                Err(error) => {
                    app.mixer_addr = None;
                    app.discovered_mixer = None;
                    app.names = std::array::from_fn(|_| None);
                    app.gains = [None; STRIP_COUNT];
                    app.gain_sources = [GainSource::Trim; STRIP_COUNT];
                    app.sends = [[None; SEND_BUS_COUNT]; STRIP_COUNT];
                    app.pans = [None; STRIP_COUNT];
                    app.faders = [None; STRIP_COUNT];
                    app.meters_db = [-90.0; STRIP_COUNT];
                    app.muted = [None; STRIP_COUNT];
                    app.soloed = [None; STRIP_COUNT];
                    app.status = ConnectionStatus::Disconnected;
                    app.last_error = Some(error);
                    Task::none()
                }
            }
        }
        Message::ProbeFinished(result) => {
            app.probe_in_flight = false;
            let was_connected = matches!(app.status, ConnectionStatus::Connected(_));

            match result {
                Ok(ProbeOutcome::Connected { response, .. }) => {
                    app.status = ConnectionStatus::Connected(response);
                    if !was_connected {
                        if let Some(mixer_addr) = app.mixer_addr {
                            return Task::batch([
                                spawn_load_names(mixer_addr),
                                spawn_load_gains(mixer_addr),
                                spawn_load_sends(mixer_addr),
                                spawn_load_pans(mixer_addr),
                                spawn_load_faders(mixer_addr),
                                spawn_load_mutes(mixer_addr),
                                spawn_load_solos(mixer_addr),
                            ]);
                        }
                    }
                }
                Ok(ProbeOutcome::Disconnected) => {
                    app.status = ConnectionStatus::Disconnected;
                    app.last_error = None;
                    app.names = std::array::from_fn(|_| None);
                    app.gains = [None; STRIP_COUNT];
                    app.gain_sources = [GainSource::Trim; STRIP_COUNT];
                    app.sends = [[None; SEND_BUS_COUNT]; STRIP_COUNT];
                    app.pans = [None; STRIP_COUNT];
                    app.faders = [None; STRIP_COUNT];
                    app.meters_db = [-90.0; STRIP_COUNT];
                    app.muted = [None; STRIP_COUNT];
                    app.soloed = [None; STRIP_COUNT];
                    if !app.manual_target {
                        app.mixer_addr = None;
                        app.discovered_mixer = None;
                    }
                }
                Err(error) => {
                    app.status = ConnectionStatus::Disconnected;
                    app.last_error = Some(error);
                    app.names = std::array::from_fn(|_| None);
                    app.gains = [None; STRIP_COUNT];
                    app.gain_sources = [GainSource::Trim; STRIP_COUNT];
                    app.sends = [[None; SEND_BUS_COUNT]; STRIP_COUNT];
                    app.pans = [None; STRIP_COUNT];
                    app.faders = [None; STRIP_COUNT];
                    app.meters_db = [-90.0; STRIP_COUNT];
                    app.muted = [None; STRIP_COUNT];
                    app.soloed = [None; STRIP_COUNT];
                    if !app.manual_target {
                        app.mixer_addr = None;
                        app.discovered_mixer = None;
                    }
                }
            }

            Task::none()
        }
    }
}

fn subscription(_app: &StatusApp) -> Subscription<Message> {
    let ticker = time::every(Duration::from_secs(3)).map(|_| Message::Tick);

    if let Some(mixer_addr) = _app.mixer_addr {
        Subscription::batch([
            ticker,
            state_subscription(mixer_addr),
            meter_subscription(mixer_addr),
        ])
    } else {
        ticker
    }
}

fn theme(_app: &StatusApp) -> Theme {
    Theme::TokyoNight
}

fn view(app: &StatusApp) -> Element<'_, Message> {
    if matches!(app.status, ConnectionStatus::Connected(_)) {
        return container(mixer_strips(app))
            .padding(24)
            .center_x(Fill)
            .height(Length::Fill)
            .into();
    }

    let (label, color) = match app.status {
        ConnectionStatus::Checking => ("checking", Color::from_rgb8(0xE0, 0xB6, 0x4A)),
        ConnectionStatus::Connected(_) => ("connected", Color::from_rgb8(0x7D, 0xD3, 0xA7)),
        ConnectionStatus::Disconnected => ("disconnected", Color::from_rgb8(0xF0, 0x7C, 0x82)),
    };

    let address_line = app
        .mixer_addr
        .map(|addr| addr.to_string())
        .unwrap_or_else(|| "discovering on UDP broadcast".to_owned());

    let identity_line = app.discovered_mixer.as_ref().map_or_else(
        || "".to_owned(),
        |mixer| match (&mixer.name, &mixer.model, &mixer.firmware) {
            (Some(name), Some(model), Some(firmware)) => {
                format!("device: {name} ({model}, fw {firmware})")
            }
            (Some(name), Some(model), None) => format!("device: {name} ({model})"),
            (Some(name), None, None) => format!("device: {name}"),
            _ => "".to_owned(),
        },
    );

    let response_line = match app.status {
        ConnectionStatus::Connected(response) => format!("reply: {}", response_name(response)),
        ConnectionStatus::Checking => "reply: waiting".to_owned(),
        ConnectionStatus::Disconnected => "reply: none".to_owned(),
    };

    let error_line = app
        .last_error
        .as_deref()
        .map_or_else(|| "".to_owned(), |error| format!("error: {error}"));

    let status_panel = column![
        text("X32 mixer status").size(28),
        text(address_line).size(16),
        text(label).size(44).color(color),
        text(identity_line).size(16),
        text(response_line).size(16),
        text(error_line)
            .size(14)
            .color(Color::from_rgb8(0xC7, 0xC9, 0xD3)),
    ]
    .spacing(8)
    .width(Length::FillPortion(2));

    let content = row![status_panel];

    container(content)
        .padding(24)
        .center_x(Fill)
        .center_y(Fill)
        .into()
}

fn spawn_probe(mixer_addr: SocketAddr) -> Task<Message> {
    Task::perform(
        async move {
            ConnectionProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(400))
                .probe()
                .map_err(|error| error.to_string())
        },
        Message::ProbeFinished,
    )
}

fn spawn_load_faders(mixer_addr: SocketAddr) -> Task<Message> {
    Task::perform(
        async move {
            FaderBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .load(&VISIBLE_STRIPS)
                .map_err(|error| error.to_string())
        },
        Message::FadersLoaded,
    )
}

fn spawn_load_names(mixer_addr: SocketAddr) -> Task<Message> {
    Task::perform(
        async move {
            NameBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .load(&VISIBLE_STRIPS)
                .map_err(|error| error.to_string())
        },
        Message::NamesLoaded,
    )
}

fn spawn_load_gains(mixer_addr: SocketAddr) -> Task<Message> {
    Task::perform(
        async move {
            GainBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .load(&VISIBLE_STRIPS)
                .map_err(|error| error.to_string())
        },
        Message::GainsLoaded,
    )
}

fn spawn_load_sends(mixer_addr: SocketAddr) -> Task<Message> {
    Task::perform(
        async move {
            SendBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .load(&VISIBLE_STRIPS, &SEND_BUSES)
                .map_err(|error| error.to_string())
        },
        Message::SendsLoaded,
    )
}

fn spawn_load_pans(mixer_addr: SocketAddr) -> Task<Message> {
    Task::perform(
        async move {
            PanBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .load(&VISIBLE_STRIPS)
                .map_err(|error| error.to_string())
        },
        Message::PansLoaded,
    )
}

fn spawn_load_mutes(mixer_addr: SocketAddr) -> Task<Message> {
    Task::perform(
        async move {
            MuteBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .load(&VISIBLE_STRIPS)
                .map_err(|error| error.to_string())
        },
        Message::MutesLoaded,
    )
}

fn spawn_load_solos(mixer_addr: SocketAddr) -> Task<Message> {
    Task::perform(
        async move {
            SoloBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .load(&VISIBLE_STRIPS)
                .map_err(|error| error.to_string())
        },
        Message::SolosLoaded,
    )
}

fn spawn_set_fader(mixer_addr: SocketAddr, target: FaderTarget, value: f32) -> Task<Message> {
    Task::perform(
        async move {
            FaderBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .set(target, value)
                .map_err(|error| error.to_string())
        },
        Message::FaderSetFinished,
    )
}

fn spawn_set_pan(mixer_addr: SocketAddr, target: FaderTarget, value: f32) -> Task<Message> {
    Task::perform(
        async move {
            PanBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .set(target, value)
                .map_err(|error| error.to_string())
        },
        Message::PanSetFinished,
    )
}

fn spawn_set_send(
    mixer_addr: SocketAddr,
    target: FaderTarget,
    bus: u8,
    value: f32,
) -> Task<Message> {
    Task::perform(
        async move {
            SendBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .set(target, bus, value)
                .map_err(|error| error.to_string())
        },
        Message::SendSetFinished,
    )
}

fn spawn_set_gain(
    mixer_addr: SocketAddr,
    target: FaderTarget,
    source: GainSource,
    value: f32,
) -> Task<Message> {
    Task::perform(
        async move {
            GainBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .set(target, source, value)
                .map_err(|error| error.to_string())
        },
        Message::GainSetFinished,
    )
}

fn spawn_set_mute(mixer_addr: SocketAddr, target: FaderTarget, on: bool) -> Task<Message> {
    Task::perform(
        async move {
            MuteBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .set(target, on)
                .map_err(|error| error.to_string())
        },
        Message::MuteSetFinished,
    )
}

fn spawn_set_solo(mixer_addr: SocketAddr, target: FaderTarget, on: bool) -> Task<Message> {
    Task::perform(
        async move {
            SoloBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .set(target, on)
                .map_err(|error| error.to_string())
        },
        Message::SoloSetFinished,
    )
}

fn spawn_discovery() -> Task<Message> {
    Task::perform(
        async move {
            DiscoveryProbe::new()
                .with_timeout(Duration::from_millis(900))
                .discover()
                .map_err(|error| error.to_string())
        },
        Message::DiscoveryFinished,
    )
}

fn refresh_mixer(mixer_addr: SocketAddr) -> Task<Message> {
    Task::batch([
        spawn_probe(mixer_addr),
        spawn_load_names(mixer_addr),
        spawn_load_gains(mixer_addr),
        spawn_load_sends(mixer_addr),
        spawn_load_pans(mixer_addr),
        spawn_load_faders(mixer_addr),
        spawn_load_mutes(mixer_addr),
        spawn_load_solos(mixer_addr),
    ])
}

fn state_subscription(mixer_addr: SocketAddr) -> Subscription<Message> {
    Subscription::run_with(mixer_addr, state_worker).map(Message::ConsoleUpdateReceived)
}

fn mixer_addr_from_args_or_env() -> Option<SocketAddr> {
    let candidate = env::args()
        .nth(1)
        .or_else(|| env::var("MIXOSC_MIXER_ADDR").ok());

    candidate.map(|candidate| {
        parse_target(&candidate).unwrap_or_else(|error| {
            panic!(
                "invalid mixer address '{candidate}'. pass host[:port] as argv[1] or MIXOSC_MIXER_ADDR: {error}"
            )
        })
    })
}

fn response_name(response: ProbeResponse) -> &'static str {
    match response {
        ProbeResponse::Info => "/info",
        ProbeResponse::Status => "/status",
        ProbeResponse::XInfo => "/xinfo",
        ProbeResponse::Unknown => "unknown",
    }
}

fn mixer_strips(app: &StatusApp) -> Element<'_, Message> {
    let strips = app
        .faders
        .iter()
        .enumerate()
        .fold(
            row!().spacing(14).align_y(iced::Alignment::End),
            |strips, (index, value)| {
                let gain_value = app.gains[index].unwrap_or(0.0);
                let gain_source = app.gain_sources[index];
                let fader_value = value.unwrap_or(0.0);
                let pan_value = app.pans[index].unwrap_or(0.5);
                let gain_label = format_gain_label(gain_value, gain_source);
                let value_label = value
                    .map(format_fader_label)
                    .unwrap_or_else(|| "--".to_owned());
                let pan_label = format_pan_label(pan_value);
                let target = VISIBLE_STRIPS[index];
                let is_muted = app.muted[index].unwrap_or(false);
                let is_soloed = app.soloed[index].unwrap_or(false);
                let meter = container(
                    meters(1, &[app.meters_db[index]], STRIP_METER_HEIGHT)
                        .map(|()| unreachable!("meter widget does not emit messages")),
                )
                .height(Length::Fill);
                let scale = container(
                    meter_ticks(STRIP_METER_HEIGHT)
                        .map(|()| unreachable!("tick widget does not emit messages")),
                )
                .height(Length::Fill)
                .align_y(iced::alignment::Vertical::Bottom);
                let sends = SEND_BUSES.iter().enumerate().fold(
                    column!().spacing(2).align_x(iced::Alignment::Center),
                    |column, (bus_index, _bus)| {
                        let send_value = app.sends[index][bus_index].unwrap_or(0.0);
                        column.push(
                            horizontal_slider(
                                0.0..=1.0,
                                send_value,
                                move |next| Message::SendChanged(index, bus_index, next)
                            )
                            .fill_from_start()
                            .step(0.01)
                            .double_click_reset(0.0)
                            .width(Length::Fixed(72.0))
                            .height(Length::Fixed(10.0)),
                        )
                    },
                );

                strips.push(
                    column![
                        column![
                            text(gain_label).size(12),
                            horizontal_slider(
                                gain_range(gain_source),
                                gain_value,
                                move |next| Message::GainChanged(index, next)
                            )
                            .fill_from_start()
                            .filled_color(Color::from_rgb8(0xD9, 0x7A, 0x2B))
                            .handle_color(Color::from_rgb8(0xF3, 0xB3, 0x6A))
                            .step(gain_step(gain_source))
                            .double_click_reset(0.0)
                            .width(Length::Fixed(72.0))
                            .height(Length::Fixed(10.0)),
                        ]
                        .spacing(4)
                        .align_x(iced::Alignment::Center),
                        sends,
                        column![
                            text(pan_label).size(12),
                            horizontal_slider(
                                0.0..=1.0,
                                pan_value,
                                move |next| Message::PanChanged(index, next)
                            )
                            .step(0.01)
                            .double_click_reset(0.5)
                            .width(Length::Fixed(72.0))
                            .height(Length::Fixed(12.0)),
                        ]
                        .spacing(4)
                        .align_x(iced::Alignment::Center),
                        text(strip_name(app, index, target)).size(14),
                        button(text("SOLO").size(12))
                            .padding([6, 8])
                            .style(move |_theme: &Theme, _status| toggle_button_style(is_soloed))
                            .on_press(Message::SoloPressed(index)),
                        text(value_label).size(14),
                        row![
                            vertical_slider(
                                0.0..=1.0,
                                fader_value,
                                move |next| Message::FaderChanged(index, next)
                            )
                            .height(Length::Fill)
                            .width(Length::Fixed(20.0))
                            .double_click_reset(0.75)
                            .step(0.01),
                            scale,
                            meter,
                        ]
                        .spacing(6)
                        .height(Length::Fill)
                        .align_y(iced::Alignment::End),
                        button(text("MUTE").size(12))
                            .padding([6, 8])
                            .style(move |_theme: &Theme, _status| toggle_button_style(is_muted))
                            .on_press(Message::MutePressed(index)),
                        text(strip_label(target)).size(14),
                    ]
                    .spacing(10)
                    .align_x(iced::Alignment::Center),
                )
            },
        );

    container(
        scrollable(
            column![strips.height(Length::Fill), Space::new().height(Length::Fixed(18.0))]
                .height(Length::Fill),
        )
        .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new()))
        .height(Length::Fill),
    )
    .width(Length::FillPortion(3))
    .height(Length::Fill)
    .into()
}

fn strip_label(target: FaderTarget) -> String {
    match target {
        FaderTarget::Channel(channel) => format!("CH {channel:02}"),
        FaderTarget::Aux(aux) => format!("AUX {aux:02}"),
    }
}

fn strip_name(app: &StatusApp, index: usize, target: FaderTarget) -> String {
    app.names[index]
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| strip_label(target))
}

fn format_fader_label(value: f32) -> String {
    if value <= 0.0 {
        return "-oo".to_owned();
    }

    format!("{:.1} dB", x32_fader_db(value))
}

fn format_pan_label(value: f32) -> String {
    let offset = ((value.clamp(0.0, 1.0) - 0.5) * 200.0).round() as i32;

    if offset == 0 {
        "C".to_owned()
    } else if offset < 0 {
        format!("L{}", -offset)
    } else {
        format!("R{offset}")
    }
}

fn gain_range(source: GainSource) -> std::ops::RangeInclusive<f32> {
    match source {
        GainSource::Headamp(_) => -12.0..=60.0,
        GainSource::Trim => -18.0..=18.0,
    }
}

fn gain_step(source: GainSource) -> f32 {
    match source {
        GainSource::Headamp(_) => 0.1,
        GainSource::Trim => 0.25,
    }
}

fn quantize_gain_value(value: f32, source: GainSource) -> f32 {
    let range = gain_range(source);
    let min = *range.start();
    let max = *range.end();
    let step = gain_step(source);
    let steps = ((value.clamp(min, max) - min) / step).round();
    (min + steps * step).clamp(min, max)
}

fn format_gain_label(value: f32, source: GainSource) -> String {
    match source {
        GainSource::Headamp(_) => format!("{value:+.1} dB"),
        GainSource::Trim => format!("T {value:+.1} dB"),
    }
}

fn x32_fader_db(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);

    if value >= 0.5 {
        value * 40.0 - 30.0
    } else if value >= 0.25 {
        value * 80.0 - 50.0
    } else if value >= 0.0625 {
        value * 160.0 - 70.0
    } else {
        value * 480.0 - 90.0
    }
}

fn linear_meter_to_db(value: f32) -> f32 {
    let value = value.max(0.000_031_622_78);
    (20.0 * value.log10()).clamp(-90.0, 20.0)
}

fn toggle_button_style(active: bool) -> button::Style {
    let border_color = Color::from_rgb8(0x7D, 0xD3, 0xA7);

    if active {
        button::Style {
            background: Some(Background::Color(border_color)),
            text_color: Color::from_rgb8(0x14, 0x18, 0x20),
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color: border_color,
            },
            ..Default::default()
        }
    } else {
        button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: border_color,
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color: border_color,
            },
            ..Default::default()
        }
    }
}

fn meter_subscription(mixer_addr: SocketAddr) -> Subscription<Message> {
    Subscription::run_with(mixer_addr, meter_worker).map(Message::MetersLoaded)
}

fn state_worker(mixer_addr: &SocketAddr) -> BoxStream<'static, Result<ConsoleUpdate, String>> {
    let mixer_addr = *mixer_addr;
    stream::channel(
        64,
        move |mut output: mpsc::Sender<Result<ConsoleUpdate, String>>| async move {
            let socket = match bind_meter_socket().await {
                Ok(socket) => socket,
                Err(error) => {
                    let _ = output.send(Err(error.to_string())).await;
                    return;
                }
            };

            if let Err(error) = socket.send_to(XREMOTE_REQUEST, mixer_addr).await {
                let _ = output.send(Err(format!("failed to send /xremote: {error}"))).await;
                return;
            }

            let mut last_xremote = Instant::now();
            let mut buffer = [0_u8; 4096];

            loop {
                if last_xremote.elapsed() >= Duration::from_secs(5) {
                    if let Err(error) = socket.send_to(XREMOTE_REQUEST, mixer_addr).await {
                        let _ = output.send(Err(format!("failed to renew /xremote: {error}"))).await;
                        return;
                    }
                    last_xremote = Instant::now();
                }

                match tokio::time::timeout(Duration::from_millis(250), socket.recv_from(&mut buffer))
                    .await
                {
                    Ok(Ok((received, _))) => {
                        if let Some(update) = parse_console_update(&buffer[..received]) {
                            let _ = output.send(Ok(update)).await;
                        }
                    }
                    Ok(Err(error)) => {
                        let _ = output
                            .send(Err(format!("failed while receiving state stream: {error}")))
                            .await;
                        return;
                    }
                    Err(_) => {}
                }

                sleep(Duration::from_millis(10)).await;
            }
        },
    )
    .boxed()
}

fn meter_worker(mixer_addr: &SocketAddr) -> BoxStream<'static, Result<Vec<StripMeter>, String>> {
    let mixer_addr = *mixer_addr;
    stream::channel(32, move |mut output: mpsc::Sender<Result<Vec<StripMeter>, String>>| async move {
        let socket = match bind_meter_socket().await {
            Ok(socket) => socket,
            Err(error) => {
                let _ = output.send(Err(error.to_string())).await;
                return;
            }
        };

        let subscribe = batchsubscribe_meter_request("meters/0", "/meters/0", 0, 0, 1);
        if let Err(error) = socket.send_to(XREMOTE_REQUEST, mixer_addr).await {
            let _ = output.send(Err(format!("failed to send /xremote: {error}"))).await;
            return;
        }
        if let Err(error) = socket.send_to(&subscribe, mixer_addr).await {
            let _ = output
                .send(Err(format!("failed to send /batchsubscribe for meters/0: {error}")))
                .await;
            return;
        }

        let renew = renew_request("meters/0");
        let mut last_xremote = Instant::now();
        let mut last_renew = Instant::now();
        let mut buffer = [0_u8; 4096];

        loop {
            if last_xremote.elapsed() >= Duration::from_secs(5) {
                if let Err(error) = socket.send_to(XREMOTE_REQUEST, mixer_addr).await {
                    let _ = output.send(Err(format!("failed to renew /xremote: {error}"))).await;
                    return;
                }
                last_xremote = Instant::now();
            }

            if last_renew.elapsed() >= Duration::from_secs(5) {
                if let Err(error) = socket.send_to(&renew, mixer_addr).await {
                    let _ = output
                        .send(Err(format!("failed to renew meter subscription: {error}")))
                        .await;
                    return;
                }
                last_renew = Instant::now();
            }

            match tokio::time::timeout(Duration::from_millis(250), socket.recv_from(&mut buffer))
                .await
            {
                Ok(Ok((received, _))) => {
                    if let Ok(meters) = parse_input_meter_packet(&buffer[..received]) {
                        let _ = output.send(Ok(meters)).await;
                    }
                }
                Ok(Err(error)) => {
                    let _ = output
                        .send(Err(format!("failed while receiving meter stream: {error}")))
                        .await;
                    return;
                }
                Err(_) => {}
            }

            sleep(Duration::from_millis(10)).await;
        }
    })
    .boxed()
}

async fn bind_meter_socket() -> std::io::Result<UdpSocket> {
    let socket = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
    Ok(socket)
}

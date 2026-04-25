use crate::{
    ColorBankProbe, ConnectionProbe, ConsoleUpdate, DiscoveredMixer, DiscoveryProbe,
    FaderBankProbe, FaderTarget, GainBankProbe, GainSource, MainMeterLevels, MuteBankProbe,
    NameBankProbe, PanBankProbe, ProbeOutcome, ProbeResponse, SendBankProbe, SoloBankProbe,
    StripColor, StripFader, StripGain, StripMeter, StripMute, StripName, StripPan, StripSend,
    StripSolo, XREMOTE_REQUEST, batchsubscribe_meter_request, parse_console_update,
    parse_input_meter_packet, parse_main_meter_packet, parse_target, renew_request,
};
use iced::futures::sink::SinkExt;
use iced::futures::{StreamExt, channel::mpsc, stream::BoxStream};
use iced::stream;
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Background, Border, Color, Element, Fill, Length, Subscription, Task, Theme, time};
use iced_fonts::lucide::{
    audio_lines, audio_waveform, equal, file_input, panel_left, send, shield, sliders_vertical,
    toggle_left,
};
use maolan_widgets::horizontal_slider::horizontal_slider;
use maolan_widgets::meters::meters;
use maolan_widgets::slider::slider as vertical_slider;
use maolan_widgets::ticks::meter_ticks;
use std::env;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::{Instant, sleep};

const STRIP_COUNT: usize = 74;
const SEND_BUS_COUNT: usize = 16;
const STRIP_METER_HEIGHT: f32 = 260.0;
const SEND_BUSES: [u8; SEND_BUS_COUNT] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
const MATRIX_SENDS: [u8; 6] = [1, 2, 3, 4, 5, 6];
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
    FaderTarget::Bus(1),
    FaderTarget::Bus(2),
    FaderTarget::Bus(3),
    FaderTarget::Bus(4),
    FaderTarget::Bus(5),
    FaderTarget::Bus(6),
    FaderTarget::Bus(7),
    FaderTarget::Bus(8),
    FaderTarget::Bus(9),
    FaderTarget::Bus(10),
    FaderTarget::Bus(11),
    FaderTarget::Bus(12),
    FaderTarget::FxRtn(1),
    FaderTarget::FxRtn(2),
    FaderTarget::FxRtn(3),
    FaderTarget::FxRtn(4),
    FaderTarget::FxRtn(5),
    FaderTarget::FxRtn(6),
    FaderTarget::FxRtn(7),
    FaderTarget::FxRtn(8),
    FaderTarget::Mtx(1),
    FaderTarget::Mtx(2),
    FaderTarget::Mtx(3),
    FaderTarget::Mtx(4),
    FaderTarget::Mtx(5),
    FaderTarget::Mtx(6),
    FaderTarget::Dca(1),
    FaderTarget::Dca(2),
    FaderTarget::Dca(3),
    FaderTarget::Dca(4),
    FaderTarget::Dca(5),
    FaderTarget::Dca(6),
    FaderTarget::Dca(7),
    FaderTarget::Dca(8),
];

#[derive(Debug)]
pub struct StatusApp {
    mixer_addr: Option<SocketAddr>,
    discovered_mixer: Option<DiscoveredMixer>,
    manual_target: bool,
    probe_in_flight: bool,
    names: [Option<String>; STRIP_COUNT],
    colors: [Option<u8>; STRIP_COUNT],
    gains: [Option<f32>; STRIP_COUNT],
    gain_sources: [GainSource; STRIP_COUNT],
    gain_drag_values: [Option<f32>; STRIP_COUNT],
    sends: [[Option<f32>; SEND_BUS_COUNT]; STRIP_COUNT],
    pans: [Option<f32>; STRIP_COUNT],
    faders: [Option<f32>; STRIP_COUNT],
    meters_db: [f32; STRIP_COUNT],
    master_meters_db: [f32; 2],
    muted: [Option<bool>; STRIP_COUNT],
    soloed: [Option<bool>; STRIP_COUNT],
    master_fader: Option<f32>,
    master_muted: Option<bool>,
    master_soloed: Option<bool>,
    master_color: Option<u8>,
    active_view: AppView,
    selected_strip: Option<SelectedStrip>,
    status: ConnectionStatus,
    last_error: Option<String>,
}

impl Default for StatusApp {
    fn default() -> Self {
        Self {
            mixer_addr: None,
            discovered_mixer: None,
            manual_target: false,
            probe_in_flight: false,
            names: std::array::from_fn(|_| None),
            colors: [None; STRIP_COUNT],
            gains: [None; STRIP_COUNT],
            gain_sources: [GainSource::Trim; STRIP_COUNT],
            gain_drag_values: [None; STRIP_COUNT],
            sends: [[None; SEND_BUS_COUNT]; STRIP_COUNT],
            pans: [None; STRIP_COUNT],
            faders: [None; STRIP_COUNT],
            meters_db: [-90.0; STRIP_COUNT],
            master_meters_db: [-90.0, -90.0],
            muted: [None; STRIP_COUNT],
            soloed: [None; STRIP_COUNT],
            master_fader: None,
            master_muted: None,
            master_soloed: None,
            master_color: None,
            active_view: AppView::Mixer,
            selected_strip: Some(SelectedStrip::Strip(0)),
            status: ConnectionStatus::Disconnected,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppView {
    Mixer,
    Channel,
    Config,
    Gate,
    Dyn,
    Eq,
    Sends,
    Main,
    Fx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedStrip {
    Strip(usize),
    Master,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Checking,
    Connected(ProbeResponse),
    Disconnected,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    ConsoleUpdateReceived(Result<ConsoleUpdate, String>),
    GainChanged(usize, f32),
    GainReleased(usize),
    SendChanged(usize, usize, f32),
    PanChanged(usize, f32),
    FaderChanged(usize, f32),
    MasterFaderChanged(f32),
    NamesLoaded(Result<Vec<StripName>, String>),
    ColorsLoaded(Result<Vec<StripColor>, String>),
    GainsLoaded(Result<Vec<StripGain>, String>),
    SendsLoaded(Result<Vec<StripSend>, String>),
    PansLoaded(Result<Vec<StripPan>, String>),
    FadersLoaded(Result<Vec<StripFader>, String>),
    SendSetFinished(Result<(), String>),
    GainSetFinished(Result<(), String>),
    PanSetFinished(Result<(), String>),
    FaderSetFinished(Result<(), String>),
    MetersLoaded(Result<Vec<StripMeter>, String>),
    MasterMetersLoaded(Result<MainMeterLevels, String>),
    MutePressed(usize),
    MasterMutePressed,
    MasterSoloPressed,
    NavSelected(AppView),
    StripSelected(SelectedStrip),
    MutesLoaded(Result<Vec<StripMute>, String>),
    MuteSetFinished(Result<(), String>),
    SoloPressed(usize),
    SolosLoaded(Result<Vec<StripSolo>, String>),
    SoloSetFinished(Result<(), String>),
    DiscoveryFinished(Result<Vec<DiscoveredMixer>, String>),
    ProbeFinished(Result<ProbeOutcome, String>),
}

pub fn new() -> (StatusApp, Task<Message>) {
    let maybe_target = mixer_addr_from_args_or_env();
    let app = StatusApp {
        mixer_addr: maybe_target,
        discovered_mixer: None,
        manual_target: maybe_target.is_some(),
        probe_in_flight: true,
        names: std::array::from_fn(|_| None),
        colors: [None; STRIP_COUNT],
        gains: [None; STRIP_COUNT],
        gain_sources: [GainSource::Trim; STRIP_COUNT],
        gain_drag_values: [None; STRIP_COUNT],
        sends: [[None; SEND_BUS_COUNT]; STRIP_COUNT],
        pans: [None; STRIP_COUNT],
        faders: [None; STRIP_COUNT],
        meters_db: [-90.0; STRIP_COUNT],
        master_meters_db: [-90.0, -90.0],
        muted: [None; STRIP_COUNT],
        soloed: [None; STRIP_COUNT],
        master_fader: None,
        master_muted: None,
        master_soloed: None,
        master_color: None,
        active_view: AppView::Mixer,
        selected_strip: Some(SelectedStrip::Strip(0)),
        status: ConnectionStatus::Checking,
        last_error: None,
    };

    let task = match maybe_target {
        Some(mixer_addr) => spawn_probe(mixer_addr),
        None => spawn_discovery(),
    };

    (app, task)
}

pub fn update(app: &mut StatusApp, message: Message) -> Task<Message> {
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
                        let keep_headamp_source = matches!(
                            (VISIBLE_STRIPS[index], app.gain_sources[index], strip.source),
                            (
                                FaderTarget::Channel(1..=16),
                                GainSource::Headamp(_),
                                GainSource::Trim
                            )
                        );

                        if !keep_headamp_source {
                            app.gains[index] = Some(strip.value);
                            app.gain_sources[index] = strip.source;
                        }
                    }
                }
                Ok(ConsoleUpdate::HeadampGain {
                    index: headamp_index,
                    value,
                }) => {
                    for strip_index in 0..STRIP_COUNT {
                        if app.gain_sources[strip_index] == GainSource::Headamp(headamp_index) {
                            app.gains[strip_index] = Some(value);
                        }
                    }
                }
                Ok(ConsoleUpdate::Fader(strip)) => {
                    if strip.target == FaderTarget::Main {
                        app.master_fader = Some(strip.value);
                        return Task::none();
                    }
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
                    if strip.target == FaderTarget::Main {
                        app.master_muted = Some(!strip.on);
                        return Task::none();
                    }
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
                Ok(ConsoleUpdate::Color(strip)) => {
                    if strip.target == FaderTarget::Main {
                        app.master_color = Some(strip.value);
                        return Task::none();
                    }
                    if let Some(index) = VISIBLE_STRIPS
                        .iter()
                        .position(|target| *target == strip.target)
                    {
                        app.colors[index] = Some(strip.value);
                    }
                }
                Err(error) => app.last_error = Some(error),
            }

            Task::none()
        }
        Message::GainChanged(index, value) => {
            let source = app.gain_sources[index];
            let value = quantize_gain_value(value, source);
            if let Some(drag_value) = app.gain_drag_values.get_mut(index) {
                *drag_value = Some(value);
            }
            if let Some(gain) = app.gains.get_mut(index) {
                *gain = Some(value);
            }

            let Some(mixer_addr) = app.mixer_addr else {
                return Task::none();
            };
            let target = VISIBLE_STRIPS[index];
            spawn_set_gain(mixer_addr, target, source, value)
        }
        Message::GainReleased(index) => {
            if let Some(Some(value)) = app.gain_drag_values.get(index).copied()
                && let Some(gain) = app.gains.get_mut(index)
            {
                *gain = Some(value);
            }
            if let Some(drag_value) = app.gain_drag_values.get_mut(index) {
                *drag_value = None;
            }
            Task::none()
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
        Message::MasterFaderChanged(value) => {
            app.master_fader = Some(value);

            let Some(mixer_addr) = app.mixer_addr else {
                return Task::none();
            };
            spawn_set_fader(mixer_addr, FaderTarget::Main, value)
        }
        Message::NavSelected(view) => {
            app.active_view = view;
            Task::none()
        }
        Message::StripSelected(selected) => {
            app.selected_strip = Some(selected);
            Task::none()
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
        Message::ColorsLoaded(result) => {
            match result {
                Ok(colors) => {
                    for strip in colors {
                        if strip.target == FaderTarget::Main {
                            app.master_color = Some(strip.value);
                            continue;
                        }
                        if let Some(index) = VISIBLE_STRIPS
                            .iter()
                            .position(|target| *target == strip.target)
                        {
                            app.colors[index] = Some(strip.value);
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
                        if fader.target == FaderTarget::Main {
                            app.master_fader = Some(fader.value);
                            continue;
                        }
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
        Message::MasterMetersLoaded(result) => {
            match result {
                Ok(levels) => {
                    app.master_meters_db = [
                        linear_meter_to_db(levels.main_lr[0]),
                        linear_meter_to_db(levels.main_lr[1]),
                    ];
                    for (matrix_index, level) in levels.matrices.iter().enumerate() {
                        if let Some(strip_index) = VISIBLE_STRIPS.iter().position(|target| {
                            *target == FaderTarget::Mtx((matrix_index + 1) as u8)
                        }) {
                            app.meters_db[strip_index] = linear_meter_to_db(*level);
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
            let currently_muted = app
                .muted
                .get(index)
                .and_then(|state| *state)
                .unwrap_or(false);
            let next_on = currently_muted;
            if let Some(muted) = app.muted.get_mut(index) {
                *muted = Some(!next_on);
            }
            spawn_set_mute(mixer_addr, target, next_on)
        }
        Message::MasterMutePressed => {
            let Some(mixer_addr) = app.mixer_addr else {
                return Task::none();
            };
            let currently_muted = app.master_muted.unwrap_or(false);
            let next_on = currently_muted;
            app.master_muted = Some(!next_on);
            spawn_set_mute(mixer_addr, FaderTarget::Main, next_on)
        }
        Message::MutesLoaded(result) => {
            match result {
                Ok(mutes) => {
                    for strip in mutes {
                        if strip.target == FaderTarget::Main {
                            app.master_muted = Some(!strip.on);
                            continue;
                        }
                        if let Some(index) = VISIBLE_STRIPS
                            .iter()
                            .position(|target| *target == strip.target)
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
            let target = VISIBLE_STRIPS[index];
            let next_on = !app
                .soloed
                .get(index)
                .and_then(|state| *state)
                .unwrap_or(false);
            if let Some(soloed) = app.soloed.get_mut(index) {
                *soloed = Some(next_on);
            }
            if matches!(target, FaderTarget::Mtx(_) | FaderTarget::Dca(_)) {
                return Task::none();
            }
            let Some(mixer_addr) = app.mixer_addr else {
                return Task::none();
            };
            spawn_set_solo(mixer_addr, target, next_on)
        }
        Message::MasterSoloPressed => {
            let next_on = !app.master_soloed.unwrap_or(false);
            app.master_soloed = Some(next_on);
            Task::none()
        }
        Message::SolosLoaded(result) => {
            match result {
                Ok(solos) => {
                    for strip in solos {
                        if let Some(index) = VISIBLE_STRIPS
                            .iter()
                            .position(|target| *target == strip.target)
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
                        app.colors = [None; STRIP_COUNT];
                        app.gains = [None; STRIP_COUNT];
                        app.gain_sources = [GainSource::Trim; STRIP_COUNT];
                        app.sends = [[None; SEND_BUS_COUNT]; STRIP_COUNT];
                        app.pans = [None; STRIP_COUNT];
                        app.faders = [None; STRIP_COUNT];
                        app.meters_db = [-90.0; STRIP_COUNT];
                        app.master_meters_db = [-90.0, -90.0];
                        app.muted = [None; STRIP_COUNT];
                        app.soloed = [None; STRIP_COUNT];
                        app.master_fader = None;
                        app.master_muted = None;
                        app.master_soloed = None;
                        app.master_color = None;
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
                    app.master_meters_db = [-90.0, -90.0];
                    app.muted = [None; STRIP_COUNT];
                    app.soloed = [None; STRIP_COUNT];
                    app.master_fader = None;
                    app.master_muted = None;
                    app.master_soloed = None;
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
                    if !was_connected && let Some(mixer_addr) = app.mixer_addr {
                        return Task::batch([
                            spawn_load_names(mixer_addr),
                            spawn_load_colors(mixer_addr),
                            spawn_load_gains(mixer_addr),
                            spawn_load_sends(mixer_addr),
                            spawn_load_pans(mixer_addr),
                            spawn_load_faders(mixer_addr),
                            spawn_load_mutes(mixer_addr),
                            spawn_load_solos(mixer_addr),
                        ]);
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
                    app.master_meters_db = [-90.0, -90.0];
                    app.muted = [None; STRIP_COUNT];
                    app.soloed = [None; STRIP_COUNT];
                    app.master_fader = None;
                    app.master_muted = None;
                    app.master_soloed = None;
                    app.master_color = None;
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
                    app.master_meters_db = [-90.0, -90.0];
                    app.muted = [None; STRIP_COUNT];
                    app.soloed = [None; STRIP_COUNT];
                    app.master_fader = None;
                    app.master_muted = None;
                    app.master_soloed = None;
                    app.master_color = None;
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

pub fn subscription(_app: &StatusApp) -> Subscription<Message> {
    let ticker = time::every(Duration::from_secs(3)).map(|_| Message::Tick);

    if let Some(mixer_addr) = _app.mixer_addr {
        Subscription::batch([
            ticker,
            state_subscription(mixer_addr),
            meter_subscription(mixer_addr),
            master_meter_subscription(mixer_addr),
        ])
    } else {
        ticker
    }
}

pub fn theme(_app: &StatusApp) -> Theme {
    Theme::TokyoNight
}

pub fn view(app: &StatusApp) -> Element<'_, Message> {
    let content: Element<'_, Message> = if matches!(app.status, ConnectionStatus::Connected(_)) {
        let mixer_view: Element<'_, Message> = if let Some(panel) = top_detail_panel(app) {
            column![panel, mixer_strips(app)]
                .spacing(0)
                .height(Length::Fill)
                .into()
        } else {
            mixer_strips(app)
        };

        container(mixer_view)
            .padding([0, 16])
            .height(Length::Fill)
            .into()
    } else {
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

        container(row![status_panel])
            .padding([24, 16])
            .center_x(Fill)
            .center_y(Fill)
            .into()
    };

    let body = if matches!(app.status, ConnectionStatus::Connected(_)) {
        column![
            container(top_nav_bar(app))
                .padding([0, 16])
                .width(Length::Shrink),
            content
        ]
        .spacing(0)
        .into()
    } else {
        content
    };

    container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn top_detail_panel(app: &StatusApp) -> Option<Element<'_, Message>> {
    match app.active_view {
        AppView::Mixer => None,
        AppView::Channel => Some(channel_detail_panel(app)),
        AppView::Config => Some(config_detail_panel()),
        AppView::Gate => Some(gate_detail_panel()),
        AppView::Dyn => Some(dyn_detail_panel()),
        AppView::Eq => Some(eq_detail_panel()),
        AppView::Sends => Some(sends_detail_panel()),
        AppView::Main => Some(main_detail_panel()),
        AppView::Fx => Some(fx_detail_panel()),
    }
}

#[derive(Clone, Copy)]
pub struct NavTab {
    icon: fn() -> iced::widget::Text<'static, Theme>,
    label: &'static str,
    view: AppView,
}

fn top_nav_bar(app: &StatusApp) -> Element<'static, Message> {
    const TABS: [NavTab; 9] = [
        NavTab {
            icon: sliders_vertical,
            label: "Mixer",
            view: AppView::Mixer,
        },
        NavTab {
            icon: panel_left,
            label: "Channel",
            view: AppView::Channel,
        },
        NavTab {
            icon: file_input,
            label: "Config",
            view: AppView::Config,
        },
        NavTab {
            icon: toggle_left,
            label: "Gate",
            view: AppView::Gate,
        },
        NavTab {
            icon: audio_waveform,
            label: "Dyn",
            view: AppView::Dyn,
        },
        NavTab {
            icon: equal,
            label: "EQ",
            view: AppView::Eq,
        },
        NavTab {
            icon: send,
            label: "Sends",
            view: AppView::Sends,
        },
        NavTab {
            icon: audio_lines,
            label: "Main",
            view: AppView::Main,
        },
        NavTab {
            icon: shield,
            label: "FX1 - 8",
            view: AppView::Fx,
        },
    ];

    let tabs = TABS.into_iter().fold(
        row!()
            .spacing(4)
            .padding([3, 3])
            .align_y(iced::Alignment::Center),
        |row, tab| row.push(nav_button(tab, app.active_view == tab.view)),
    );

    container(tabs)
        .height(Length::Shrink)
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb8(0x1C, 0x1C, 0x1C))),
            border: Border {
                color: Color::from_rgb8(0x2A, 0x2A, 0x2A),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn nav_button(tab: NavTab, selected: bool) -> Element<'static, Message> {
    let accent = mixer_accent_color();
    let active_text = accent;
    let inactive_text = Color::from_rgb8(0xA9, 0xAC, 0xB3);

    let icon =
        container(
            (tab.icon)()
                .size(17)
                .color(if selected { active_text } else { inactive_text }),
        )
        .width(Length::Fixed(24.0))
        .height(Length::Fixed(24.0))
        .padding(0)
        .center_x(Fill)
        .center_y(Fill)
        .style(move |_theme: &Theme| container::Style {
            border: Border {
                color: if selected {
                    accent
                } else {
                    Color::from_rgb8(0x6B, 0x6F, 0x76)
                },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        });

    button(
        row![
            icon,
            text(tab.label)
                .size(14)
                .color(if selected { active_text } else { inactive_text }),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .padding([4, 10])
    .width(Length::Fixed(108.0))
    .height(Length::Fixed(36.0))
    .style(move |_theme: &Theme, _status| button::Style {
        background: Some(Background::Color(if selected {
            Color::from_rgb8(0x2A, 0x2A, 0x2A)
        } else {
            Color::from_rgb8(0x24, 0x24, 0x24)
        })),
        border: Border {
            color: if selected {
                Color::from_rgb8(0x4B, 0x4B, 0x4B)
            } else {
                Color::from_rgb8(0x3A, 0x3A, 0x3A)
            },
            width: 1.0,
            radius: 0.0.into(),
        },
        text_color: if selected { active_text } else { inactive_text },
        ..Default::default()
    })
    .on_press(Message::NavSelected(tab.view))
    .into()
}

fn mixer_accent_color() -> Color {
    Color::from_rgb8(0x29, 0xE6, 0xF2)
}

fn strip_module_item(label: &'static str) -> Element<'static, Message> {
    text(label)
        .size(12)
        .color(Color::from_rgb8(0xC7, 0xC9, 0xD3))
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
                .load(&[VISIBLE_STRIPS.as_slice(), &[FaderTarget::Main]].concat())
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

fn spawn_load_colors(mixer_addr: SocketAddr) -> Task<Message> {
    Task::perform(
        async move {
            ColorBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .load(&[VISIBLE_STRIPS.as_slice(), &[FaderTarget::Main]].concat())
                .map_err(|error| error.to_string())
        },
        Message::ColorsLoaded,
    )
}

fn spawn_load_gains(mixer_addr: SocketAddr) -> Task<Message> {
    let targets: Vec<FaderTarget> = VISIBLE_STRIPS
        .iter()
        .filter(|t| {
            !matches!(
                t,
                FaderTarget::Bus(_)
                    | FaderTarget::FxRtn(_)
                    | FaderTarget::Mtx(_)
                    | FaderTarget::Dca(_)
            )
        })
        .cloned()
        .collect();
    Task::perform(
        async move {
            GainBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .load(&targets)
                .map_err(|error| error.to_string())
        },
        Message::GainsLoaded,
    )
}

fn spawn_load_sends(mixer_addr: SocketAddr) -> Task<Message> {
    let channel_aux_targets: Vec<FaderTarget> = VISIBLE_STRIPS
        .iter()
        .filter(|t| {
            matches!(
                t,
                FaderTarget::Channel(_) | FaderTarget::Aux(_) | FaderTarget::FxRtn(_)
            )
        })
        .cloned()
        .collect();
    let bus_targets: Vec<FaderTarget> = VISIBLE_STRIPS
        .iter()
        .filter(|t| matches!(t, FaderTarget::Bus(_)))
        .cloned()
        .collect();
    Task::batch([
        Task::perform(
            async move {
                SendBankProbe::new(mixer_addr)
                    .with_timeout(Duration::from_millis(250))
                    .load(&channel_aux_targets, &SEND_BUSES)
                    .map_err(|error| error.to_string())
            },
            Message::SendsLoaded,
        ),
        Task::perform(
            async move {
                SendBankProbe::new(mixer_addr)
                    .with_timeout(Duration::from_millis(250))
                    .load(&bus_targets, &MATRIX_SENDS)
                    .map_err(|error| error.to_string())
            },
            Message::SendsLoaded,
        ),
    ])
}

fn spawn_load_pans(mixer_addr: SocketAddr) -> Task<Message> {
    let targets: Vec<FaderTarget> = VISIBLE_STRIPS
        .iter()
        .filter(|t| !matches!(t, FaderTarget::Dca(_) | FaderTarget::Mtx(_)))
        .cloned()
        .collect();
    Task::perform(
        async move {
            PanBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .load(&targets)
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
                .load(&[VISIBLE_STRIPS.as_slice(), &[FaderTarget::Main]].concat())
                .map_err(|error| error.to_string())
        },
        Message::MutesLoaded,
    )
}

fn spawn_load_solos(mixer_addr: SocketAddr) -> Task<Message> {
    let targets: Vec<FaderTarget> = VISIBLE_STRIPS
        .iter()
        .filter(|t| !matches!(t, FaderTarget::Mtx(_) | FaderTarget::Dca(_)))
        .cloned()
        .collect();
    Task::perform(
        async move {
            SoloBankProbe::new(mixer_addr)
                .with_timeout(Duration::from_millis(250))
                .load(&targets)
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
        spawn_load_colors(mixer_addr),
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

    candidate.and_then(|candidate| parse_target(&candidate).ok())
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
    let strips = app.faders.iter().enumerate().fold(
        row!().spacing(0).align_y(iced::Alignment::End),
        |strips, (index, value)| {
            let gain_value = app.gain_drag_values[index]
                .or(app.gains[index])
                .unwrap_or(0.0);
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
            let is_selected = app.selected_strip == Some(SelectedStrip::Strip(index));
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
            let sends: Element<'_, Message> = match target {
                FaderTarget::Channel(_) | FaderTarget::Aux(_) | FaderTarget::FxRtn(_) => SEND_BUSES
                    .iter()
                    .enumerate()
                    .fold(
                        column!().spacing(2).align_x(iced::Alignment::Center),
                        |column, (bus_index, _bus)| {
                            let send_value = app.sends[index][bus_index].unwrap_or(0.0);
                            column.push(
                                horizontal_slider(0.0..=1.0, send_value, move |next| {
                                    Message::SendChanged(index, bus_index, next)
                                })
                                .fill_from_start()
                                .step(0.01)
                                .double_click_reset(0.0)
                                .width(Length::Fixed(72.0))
                                .height(Length::Fixed(10.0)),
                            )
                        },
                    )
                    .into(),
                FaderTarget::Bus(_) | FaderTarget::Main => MATRIX_SENDS
                    .iter()
                    .enumerate()
                    .fold(
                        column!().spacing(2).align_x(iced::Alignment::Center),
                        |column, (bus_index, _bus)| {
                            let send_value = app.sends[index][bus_index].unwrap_or(0.0);
                            column.push(
                                horizontal_slider(0.0..=1.0, send_value, move |next| {
                                    Message::SendChanged(index, bus_index, next)
                                })
                                .fill_from_start()
                                .step(0.01)
                                .double_click_reset(0.0)
                                .width(Length::Fixed(72.0))
                                .height(Length::Fixed(10.0)),
                            )
                        },
                    )
                    .into(),
                FaderTarget::Mtx(_) | FaderTarget::Dca(_) => {
                    Space::new().height(Length::Fixed(0.0)).into()
                }
            };
            let hide_strip_top_controls = app.active_view != AppView::Mixer;
            let top_sends: Element<'_, Message> = if hide_strip_top_controls {
                Space::new().height(Length::Fixed(0.0)).into()
            } else {
                sends
            };
            let top_gain_label = if hide_strip_top_controls {
                String::new()
            } else {
                gain_label
            };
            let top_controls = strip_mixer_top(
                index,
                target,
                gain_value,
                gain_source,
                top_gain_label,
                pan_value,
                if hide_strip_top_controls {
                    String::new()
                } else {
                    pan_label
                },
                top_sends,
            );

            let solo_button: Element<'_, Message> = if matches!(target, FaderTarget::Mtx(_)) {
                Space::new().height(Length::Fixed(0.0)).into()
            } else {
                button(text("SOLO").size(12))
                    .padding([6, 8])
                    .style(move |_theme: &Theme, _status| {
                        toggle_button_style(is_soloed, Color::from_rgb8(0xF0, 0xC0, 0x30))
                    })
                    .on_press(Message::SoloPressed(index))
                    .into()
            };

            let mut strip = column![top_controls]
                .spacing(10)
                .align_x(iced::Alignment::Center);
            let strip_color = app.colors[index].unwrap_or(0);
            let color_rgb = x32_color_to_rgb(strip_color);
            let is_inverted = (9..=15).contains(&strip_color);
            let text_color = if is_inverted { Color::BLACK } else { color_rgb };
            let bg = if is_inverted {
                Some(Background::Color(color_rgb))
            } else {
                None
            };
            strip = strip.push(
                button(
                    container(
                        text(strip_name(app, index, target))
                            .size(14)
                            .color(text_color),
                    )
                    .style(move |_theme: &Theme| container::Style {
                        border: Border {
                            color: color_rgb,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        background: bg,
                        ..Default::default()
                    })
                    .padding([2, 6]),
                )
                .style(button::text)
                .on_press(Message::StripSelected(SelectedStrip::Strip(index))),
            );
            if !matches!(target, FaderTarget::Mtx(_)) {
                strip = strip.push(solo_button);
            }
            strip = strip.push(text(value_label).size(14));
            strip = strip.push(
                row![
                    vertical_slider(0.0..=1.0, fader_value, move |next| Message::FaderChanged(
                        index, next
                    ))
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
            );
            strip = strip.push(
                button(text("MUTE").size(12))
                    .padding([6, 8])
                    .style(move |_theme: &Theme, _status| {
                        toggle_button_style(is_muted, Color::from_rgb8(0xE0, 0x50, 0x50))
                    })
                    .on_press(Message::MutePressed(index)),
            );
            strip = strip.push(text(strip_label(target)).size(14));
            strips.push(
                container(strip)
                    .style(move |_theme: &Theme| container::Style {
                        border: Border {
                            color: if is_selected {
                                mixer_accent_color()
                            } else {
                                Color::from_rgb8(0x3B, 0x42, 0x52)
                            },
                            width: if is_selected { 2.0 } else { 1.0 },
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    })
                    .padding([0, 7]),
            )
        },
    );

    let master_selected = app.selected_strip == Some(SelectedStrip::Master);
    let master_strip = {
        let value = app.master_fader.unwrap_or(0.0);
        let value_label = app
            .master_fader
            .map(format_fader_label)
            .unwrap_or_else(|| "--".to_owned());
        let is_muted = app.master_muted.unwrap_or(false);
        let is_soloed = app.master_soloed.unwrap_or(false);
        let meter = container(
            meters(2, &app.master_meters_db, STRIP_METER_HEIGHT)
                .map(|()| unreachable!("meter widget does not emit messages")),
        )
        .height(Length::Fill);
        let scale = container(
            meter_ticks(STRIP_METER_HEIGHT)
                .map(|()| unreachable!("tick widget does not emit messages")),
        )
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Bottom);

        column![
            Space::new().height(Length::Fixed(26.0)),
            Space::new().height(Length::Fixed(0.0)),
            {
                let master_color_val = app.master_color.unwrap_or(0);
                let color_rgb = x32_color_to_rgb(master_color_val);
                let is_inverted = (9..=15).contains(&master_color_val);
                let text_color = if is_inverted { Color::BLACK } else { color_rgb };
                let bg = if is_inverted {
                    Some(Background::Color(color_rgb))
                } else {
                    None
                };
                button(
                    container(text("LR").size(14).color(text_color))
                        .style(move |_theme: &Theme| container::Style {
                            border: Border {
                                color: color_rgb,
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            background: bg,
                            ..Default::default()
                        })
                        .padding([2, 6]),
                )
                .style(button::text)
                .on_press(Message::StripSelected(SelectedStrip::Master))
            },
            button(text("SOLO").size(12))
                .padding([6, 8])
                .style(move |_theme: &Theme, _status| toggle_button_style(
                    is_soloed,
                    Color::from_rgb8(0xF0, 0xC0, 0x30)
                ))
                .on_press(Message::MasterSoloPressed),
            text(value_label).size(14),
            row![
                vertical_slider(0.0..=1.0, value, Message::MasterFaderChanged)
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
                .style(move |_theme: &Theme, _status| toggle_button_style(
                    is_muted,
                    Color::from_rgb8(0xE0, 0x50, 0x50)
                ))
                .on_press(Message::MasterMutePressed),
            text("LR").size(14),
        ]
        .spacing(10)
        .align_x(iced::Alignment::Center)
    };

    let master_strip = container(master_strip)
        .style(move |_theme: &Theme| container::Style {
            border: Border {
                color: if master_selected {
                    mixer_accent_color()
                } else {
                    Color::from_rgb8(0x3B, 0x42, 0x52)
                },
                width: if master_selected { 2.0 } else { 1.0 },
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .padding([0, 7]);

    container(
        row![
            scrollable(
                column![
                    strips.height(Length::Fill),
                    Space::new().height(Length::Fixed(18.0))
                ]
                .height(Length::Fill),
            )
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new()
            ))
            .width(Length::Fill)
            .height(Length::Fill),
            master_strip,
        ]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::Alignment::End),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[allow(clippy::too_many_arguments)]
fn strip_mixer_top(
    index: usize,
    target: FaderTarget,
    gain_value: f32,
    gain_source: GainSource,
    gain_label: String,
    pan_value: f32,
    pan_label: String,
    sends: Element<'_, Message>,
) -> Element<'_, Message> {
    let hide_upper_controls = gain_label.is_empty();
    let hide_balance = pan_label.is_empty();
    let gain_block: Element<'static, Message> = if hide_upper_controls
        || matches!(
            target,
            FaderTarget::Bus(_) | FaderTarget::FxRtn(_) | FaderTarget::Mtx(_) | FaderTarget::Dca(_)
        ) {
        Space::new().height(Length::Fixed(26.0)).into()
    } else {
        column![
            text(gain_label).size(12),
            horizontal_slider(gain_range(gain_source), gain_value, move |next| {
                Message::GainChanged(index, next)
            })
            .fill_from_start()
            .filled_color(Color::from_rgb8(0xD9, 0x7A, 0x2B))
            .handle_color(Color::from_rgb8(0xF3, 0xB3, 0x6A))
            .step(gain_step(gain_source))
            .double_click_reset(0.0)
            .on_release(Message::GainReleased(index))
            .width(Length::Fixed(72.0))
            .height(Length::Fixed(10.0)),
        ]
        .spacing(4)
        .align_x(iced::Alignment::Center)
        .into()
    };

    let pan_block: Element<'static, Message> =
        if hide_balance || matches!(target, FaderTarget::Dca(_) | FaderTarget::Mtx(_)) {
            Space::new().height(Length::Fixed(0.0)).into()
        } else {
            column![
                text(pan_label).size(12),
                horizontal_slider(0.0..=1.0, pan_value, move |next| Message::PanChanged(
                    index, next
                ))
                .step(0.01)
                .double_click_reset(0.5)
                .width(Length::Fixed(72.0))
                .height(Length::Fixed(12.0)),
            ]
            .spacing(4)
            .align_x(iced::Alignment::Center)
            .into()
        };

    let mut top = column![gain_block]
        .spacing(10)
        .align_x(iced::Alignment::Center);
    if !hide_upper_controls && !matches!(target, FaderTarget::Mtx(_) | FaderTarget::Dca(_)) {
        top = top.push(sends);
        top = top.push(
            column![
                strip_module_item("Gate"),
                strip_module_item("EQ"),
                strip_module_item("Dyn"),
            ]
            .spacing(4)
            .align_x(iced::Alignment::Center),
        );
    }
    top.push(pan_block).into()
}

fn channel_detail_panel(app: &StatusApp) -> Element<'_, Message> {
    let selected = app.selected_strip.unwrap_or(SelectedStrip::Strip(0));
    let index = match selected {
        SelectedStrip::Strip(index) => index,
        SelectedStrip::Master => 0,
    };
    let target = VISIBLE_STRIPS[index];
    let pan_value = app.pans[index].unwrap_or(0.5);
    let sends: Element<'_, Message> = match target {
        FaderTarget::Channel(_) | FaderTarget::Aux(_) | FaderTarget::FxRtn(_) => SEND_BUSES
            .iter()
            .enumerate()
            .fold(column!().spacing(4), |column, (bus_index, bus)| {
                let send_value = app.sends[index][bus_index].unwrap_or(0.0);
                column.push(channel_send_row(index, bus_index, *bus, send_value))
            })
            .into(),
        FaderTarget::Bus(_) => MATRIX_SENDS
            .iter()
            .enumerate()
            .fold(column!().spacing(4), |column, (bus_index, bus)| {
                let send_value = app.sends[index][bus_index].unwrap_or(0.0);
                column.push(channel_send_row(index, bus_index, *bus, send_value))
            })
            .into(),
        FaderTarget::Mtx(_) | FaderTarget::Dca(_) | FaderTarget::Main => text("No sends")
            .size(14)
            .color(Color::from_rgb8(0x8E, 0x94, 0x9D))
            .into(),
    };

    let gate_panel = detail_panel("Noise Gate", module_detail_placeholder("Gate"));
    let eq_panel = detail_panel("Equalizer", module_detail_placeholder("EQ"));
    let dyn_panel = detail_panel("Dynamics", module_detail_placeholder("Comp"));
    let sends_panel = detail_panel("Bus Sends", sends);
    let balance_panel = detail_panel(
        "Balance",
        column![
            text(format_pan_label(pan_value))
                .size(18)
                .color(Color::from_rgb8(0xE6, 0xE8, 0xEE)),
            horizontal_slider(0.0..=1.0, pan_value, move |next| Message::PanChanged(
                index, next
            ))
            .step(0.01)
            .double_click_reset(0.5)
            .width(Length::Fixed(150.0))
            .height(Length::Fixed(14.0)),
        ]
        .spacing(10)
        .align_x(iced::Alignment::Center),
    );

    container(row![gate_panel, eq_panel, dyn_panel, sends_panel, balance_panel,].spacing(2))
        .height(Length::Shrink)
        .width(Length::Fill)
        .into()
}

fn config_detail_panel() -> Element<'static, Message> {
    top_panel_shell(row![
        detail_panel(
            "Source",
            column![
                placeholder_text("Stereo Link"),
                placeholder_text("Phantom"),
                placeholder_text("Polarity"),
                placeholder_select("01 : In01"),
                tiny_vertical_meter("Gain"),
            ]
            .spacing(8)
        ),
        detail_panel(
            "Low Cut",
            column![
                module_detail_placeholder("LC"),
                tiny_vertical_meter("Frequency"),
            ]
            .spacing(8)
        ),
        detail_panel(
            "Delay",
            column![
                module_detail_placeholder("Dt"),
                placeholder_text("0.3 ft"),
                placeholder_text("0.10 m"),
                tiny_vertical_meter("Delay"),
            ]
            .spacing(8)
        ),
        detail_panel(
            "Insert Position",
            column![
                row![
                    module_chip("IN"),
                    module_chip("GT"),
                    module_chip("EQ"),
                    module_chip("DY"),
                    module_chip("FX"),
                ]
                .spacing(4),
                placeholder_select("OFF"),
            ]
            .spacing(10)
        ),
    ])
}

fn gate_detail_panel() -> Element<'static, Message> {
    top_panel_shell(row![
        detail_panel(
            "Active",
            column![
                placeholder_button("Active"),
                module_detail_placeholder("Gain"),
            ]
            .spacing(10)
        ),
        detail_panel(
            "Mode",
            column![
                placeholder_radio_list(&["Exp 2:1", "Exp 3:1", "Exp 4:1", "Gate 1:1", "Ducker"]),
                row![
                    tiny_vertical_meter("Threshold"),
                    tiny_vertical_meter("Range"),
                ]
                .spacing(14),
            ]
            .spacing(8)
        ),
        detail_panel(
            "Gain Envelope",
            row![
                tiny_vertical_meter("Attack"),
                tiny_vertical_meter("Hold"),
                tiny_vertical_meter("Release"),
            ]
            .spacing(14)
        ),
        detail_panel(
            "Side Chain Filter",
            column![
                module_detail_placeholder("SC"),
                placeholder_select("Self"),
                row![tiny_vertical_meter("Type"), tiny_vertical_meter("Freq"),].spacing(14),
            ]
            .spacing(8)
        ),
    ])
}

fn dyn_detail_panel() -> Element<'static, Message> {
    top_panel_shell(row![
        detail_panel(
            "Active",
            column![
                placeholder_button("Active"),
                module_detail_placeholder("Gain"),
            ]
            .spacing(10)
        ),
        detail_panel(
            "Mode",
            column![
                placeholder_radio_list(&["1", "2", "3", "4", "5", "Peak", "RMS"]),
                row![
                    tiny_vertical_meter("Thresh"),
                    tiny_vertical_meter("Ratio"),
                    tiny_vertical_meter("Mix"),
                    tiny_vertical_meter("Gain"),
                ]
                .spacing(10),
            ]
            .spacing(8)
        ),
        detail_panel(
            "Gain Envelope",
            row![
                tiny_vertical_meter("Attack"),
                tiny_vertical_meter("Hold"),
                tiny_vertical_meter("Release"),
            ]
            .spacing(14)
        ),
        detail_panel(
            "Side Chain Filter",
            column![
                module_detail_placeholder("SC"),
                placeholder_select("Self"),
                row![tiny_vertical_meter("Type"), tiny_vertical_meter("Freq"),].spacing(14),
            ]
            .spacing(8)
        ),
    ])
}

fn eq_detail_panel() -> Element<'static, Message> {
    top_panel_shell(row![
        detail_panel(
            "Equalizer",
            column![
                eq_graph_placeholder(),
                row![
                    eq_band_box("Low", "124.7 Hz"),
                    eq_band_box("LoMid", "496.6 Hz"),
                    eq_band_box("HiMid", "1.97 kHz"),
                    eq_band_box("High", "12.02 kHz"),
                ]
                .spacing(4),
            ]
            .spacing(8),
        ),
        detail_panel(
            "Selected Band",
            column![
                placeholder_button("Pre"),
                placeholder_button("Spec"),
                tiny_horizontal_meter("Gain"),
                tiny_horizontal_meter("Freq"),
                tiny_horizontal_meter("Q"),
            ]
            .spacing(8)
        ),
    ])
}

fn sends_detail_panel() -> Element<'static, Message> {
    top_panel_shell(row![
        detail_panel(
            "Tap",
            column![
                placeholder_radio_list(&[
                    "Input",
                    "Pre EQ",
                    "Post EQ",
                    "Pre Fader",
                    "Post Fader",
                    "Sub Group"
                ]),
                row![
                    module_chip("Bus 1"),
                    module_chip("Bus 2"),
                    module_chip("Bus 3"),
                    module_chip("Bus 4"),
                ]
                .spacing(4),
            ]
            .spacing(8)
        ),
        detail_panel(
            "Bus Sends",
            column![
                row![
                    channel_send_row_placeholder("01"),
                    channel_send_row_placeholder("02"),
                    channel_send_row_placeholder("03"),
                    channel_send_row_placeholder("04"),
                ]
                .spacing(8),
                row![
                    channel_send_row_placeholder("05"),
                    channel_send_row_placeholder("06"),
                    channel_send_row_placeholder("07"),
                    channel_send_row_placeholder("08"),
                ]
                .spacing(8),
                row![
                    channel_send_row_placeholder("09"),
                    channel_send_row_placeholder("10"),
                    channel_send_row_placeholder("11"),
                    channel_send_row_placeholder("12"),
                ]
                .spacing(8),
                row![
                    channel_send_row_placeholder("13"),
                    channel_send_row_placeholder("14"),
                    channel_send_row_placeholder("15"),
                    channel_send_row_placeholder("16"),
                ]
                .spacing(8),
            ]
            .spacing(6)
        ),
    ])
}

fn main_detail_panel() -> Element<'static, Message> {
    top_panel_shell(row![
        detail_panel(
            "Main Output",
            column![
                row![module_chip("LR"), module_chip("MC")].spacing(8),
                module_detail_placeholder("Pan"),
                placeholder_radio_list(&["L/R + Mono", "LCR"]),
            ]
            .spacing(10)
        ),
        detail_panel(
            "Panning Mode",
            row![
                tiny_horizontal_meter("Left"),
                tiny_horizontal_meter("Right")
            ]
            .spacing(10)
        ),
        detail_panel(
            "Group Assignments",
            column![
                row![module_chip("X"), module_chip("Y")].spacing(8),
                placeholder_dot_row(8),
                row![
                    module_chip("1"),
                    module_chip("2"),
                    module_chip("3"),
                    module_chip("4"),
                    module_chip("5"),
                    module_chip("6"),
                ]
                .spacing(6),
            ]
            .spacing(12)
        ),
    ])
}

fn fx_detail_panel() -> Element<'static, Message> {
    top_panel_shell(row![
        fx_slot_panel("FX 1", "Stereo Guitar Amp"),
        fx_slot_panel("FX 2", "Hall Reverb"),
        fx_slot_panel("FX 3", "Stereo Delay"),
        fx_slot_panel("FX 4", "Stereo Chorus"),
    ])
}

fn detail_panel<'a>(
    title: &'static str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(
        column![
            text(title)
                .size(14)
                .color(Color::from_rgb8(0xC7, 0xC9, 0xD3)),
            content.into(),
        ]
        .spacing(10)
        .align_x(iced::Alignment::Center),
    )
    .style(|_theme: &Theme| container::Style {
        background: Some(Background::Color(Color::from_rgb8(0x1A, 0x1A, 0x1C))),
        border: Border {
            color: Color::from_rgb8(0x4B, 0x4B, 0x4B),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .padding([10, 10])
    .height(Length::Fixed(220.0))
    .width(Length::Fixed(160.0))
    .into()
}

fn top_panel_shell<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content.into())
        .padding([0, 0])
        .height(Length::Shrink)
        .width(Length::Fill)
        .into()
}

fn module_detail_placeholder<'a>(label: &'static str) -> Element<'a, Message> {
    column![
        container(
            text(label)
                .size(14)
                .color(Color::from_rgb8(0xE6, 0xE8, 0xEE))
        )
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb8(0x4B, 0x4B, 0x4B))),
            border: Border {
                color: Color::from_rgb8(0x7A, 0x7D, 0x82),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .padding([4, 14]),
        container(
            Space::new()
                .width(Length::Fixed(110.0))
                .height(Length::Fixed(72.0))
        )
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb8(0x06, 0x07, 0x09))),
            border: Border {
                color: Color::from_rgb8(0x5A, 0x5D, 0x63),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        }),
    ]
    .spacing(10)
    .align_x(iced::Alignment::Center)
    .into()
}

fn placeholder_text(label: &'static str) -> Element<'static, Message> {
    text(label)
        .size(13)
        .color(Color::from_rgb8(0xB9, 0xBC, 0xC2))
        .into()
}

fn placeholder_button(label: &'static str) -> Element<'static, Message> {
    container(
        text(label)
            .size(13)
            .color(Color::from_rgb8(0xE1, 0xE4, 0xEA)),
    )
    .padding([4, 10])
    .style(|_theme: &Theme| container::Style {
        background: Some(Background::Color(Color::from_rgb8(0x4B, 0x4B, 0x4B))),
        border: Border {
            color: Color::from_rgb8(0x6A, 0x6D, 0x73),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn placeholder_select(label: &'static str) -> Element<'static, Message> {
    container(
        row![
            text(label)
                .size(13)
                .color(Color::from_rgb8(0xE1, 0xE4, 0xEA)),
            text("▾").size(13).color(Color::from_rgb8(0xB9, 0xBC, 0xC2)),
        ]
        .spacing(18),
    )
    .padding([4, 8])
    .style(|_theme: &Theme| container::Style {
        background: Some(Background::Color(Color::from_rgb8(0x12, 0x12, 0x14))),
        border: Border {
            color: Color::from_rgb8(0x4A, 0x4D, 0x52),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn module_chip(label: &'static str) -> Element<'static, Message> {
    container(
        text(label)
            .size(12)
            .color(Color::from_rgb8(0xBF, 0xC3, 0xCB)),
    )
    .padding([3, 6])
    .style(|_theme: &Theme| container::Style {
        background: Some(Background::Color(Color::from_rgb8(0x1A, 0x1B, 0x1F))),
        border: Border {
            color: Color::from_rgb8(0x5A, 0x5D, 0x63),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn placeholder_radio_list(items: &[&'static str]) -> Element<'static, Message> {
    items
        .iter()
        .fold(column!().spacing(4), |column, item| {
            column.push(
                row![
                    text("○").size(12).color(Color::from_rgb8(0x8E, 0x94, 0x9D)),
                    text(*item)
                        .size(12)
                        .color(Color::from_rgb8(0xB9, 0xBC, 0xC2)),
                ]
                .spacing(6),
            )
        })
        .into()
}

fn placeholder_dot_row(count: usize) -> Element<'static, Message> {
    (0..count)
        .fold(row!().spacing(6), |row, n| {
            row.push(module_chip(Box::leak((n + 1).to_string().into_boxed_str())))
        })
        .into()
}

fn tiny_vertical_meter(label: &'static str) -> Element<'static, Message> {
    column![
        container(
            Space::new()
                .width(Length::Fixed(16.0))
                .height(Length::Fixed(76.0))
        )
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb8(0x20, 0x22, 0x26))),
            border: Border {
                color: Color::from_rgb8(0x3E, 0x42, 0x48),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        }),
        text(label)
            .size(11)
            .color(Color::from_rgb8(0x9D, 0xA3, 0xAC)),
    ]
    .spacing(4)
    .align_x(iced::Alignment::Center)
    .into()
}

fn tiny_horizontal_meter(label: &'static str) -> Element<'static, Message> {
    column![
        text(label)
            .size(11)
            .color(Color::from_rgb8(0x9D, 0xA3, 0xAC)),
        container(
            Space::new()
                .width(Length::Fixed(70.0))
                .height(Length::Fixed(12.0))
        )
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb8(0x20, 0x22, 0x26))),
            border: Border {
                color: Color::from_rgb8(0x3E, 0x42, 0x48),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        }),
    ]
    .spacing(4)
    .into()
}

fn eq_graph_placeholder() -> Element<'static, Message> {
    container(
        Space::new()
            .width(Length::Fixed(520.0))
            .height(Length::Fixed(92.0)),
    )
    .style(|_theme: &Theme| container::Style {
        background: Some(Background::Color(Color::from_rgb8(0x0B, 0x0C, 0x10))),
        border: Border {
            color: Color::from_rgb8(0x4A, 0x4D, 0x52),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn eq_band_box(label: &'static str, freq: &'static str) -> Element<'static, Message> {
    container(
        column![
            text(label)
                .size(12)
                .color(Color::from_rgb8(0xD7, 0xDA, 0xE0)),
            text("PEQ").size(12).color(mixer_accent_color()),
            text(freq)
                .size(11)
                .color(Color::from_rgb8(0xA9, 0xAC, 0xB3)),
        ]
        .spacing(2),
    )
    .padding([6, 8])
    .style(|_theme: &Theme| container::Style {
        background: Some(Background::Color(Color::from_rgb8(0x20, 0x22, 0x26))),
        border: Border {
            color: Color::from_rgb8(0x4A, 0x4D, 0x52),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn channel_send_row_placeholder(bus: &'static str) -> Element<'static, Message> {
    column![
        text(bus).size(12).color(mixer_accent_color()),
        container(
            Space::new()
                .width(Length::Fixed(56.0))
                .height(Length::Fixed(10.0))
        )
        .style(|_theme: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgb8(0x20, 0x22, 0x26))),
            border: Border {
                color: Color::from_rgb8(0x3E, 0x42, 0x48),
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        }),
    ]
    .spacing(4)
    .align_x(iced::Alignment::Center)
    .into()
}

fn fx_slot_panel(title: &'static str, effect: &'static str) -> Element<'static, Message> {
    detail_panel(
        title,
        column![
            placeholder_select(effect),
            module_detail_placeholder("FX"),
            row![placeholder_select("Bus 13"), tiny_vertical_meter(""),].spacing(8),
            row![placeholder_select("Bus 13"), tiny_vertical_meter(""),].spacing(8),
            placeholder_button("Tap"),
        ]
        .spacing(8),
    )
}

fn channel_send_row<'a>(
    strip_index: usize,
    bus_index: usize,
    bus: u8,
    send_value: f32,
) -> Element<'a, Message> {
    row![
        text(format!("{bus:02}"))
            .size(13)
            .width(Length::Fixed(22.0))
            .color(Color::from_rgb8(0x29, 0xE6, 0xF2)),
        horizontal_slider(0.0..=1.0, send_value, move |next| {
            Message::SendChanged(strip_index, bus_index, next)
        })
        .fill_from_start()
        .step(0.01)
        .double_click_reset(0.0)
        .width(Length::Fixed(110.0))
        .height(Length::Fixed(10.0)),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}

fn x32_color_to_rgb(value: u8) -> Color {
    match value {
        1 => Color::from_rgb8(0xFF, 0x45, 0x45),  // RD
        2 => Color::from_rgb8(0x32, 0xCD, 0x32),  // GN
        3 => Color::from_rgb8(0xFF, 0xD7, 0x00),  // YE
        4 => Color::from_rgb8(0x41, 0x69, 0xE1),  // BL
        5 => Color::from_rgb8(0xFF, 0x00, 0xFF),  // MG
        6 => Color::from_rgb8(0x00, 0xFF, 0xFF),  // CY
        7 => Color::from_rgb8(0xFF, 0xFF, 0xFF),  // WH
        9 => Color::from_rgb8(0xCC, 0x33, 0x33),  // RDi
        10 => Color::from_rgb8(0x28, 0xA4, 0x28), // GNi
        11 => Color::from_rgb8(0xCC, 0xAC, 0x00), // YEi
        12 => Color::from_rgb8(0x33, 0x55, 0xB4), // BLi
        13 => Color::from_rgb8(0xCC, 0x00, 0xCC), // MGi
        14 => Color::from_rgb8(0x00, 0xCC, 0xCC), // CYi
        15 => Color::from_rgb8(0xDD, 0xDD, 0xDD), // WHi
        _ => Color::from_rgb8(0x3B, 0x42, 0x52),  // OFF / default
    }
}

fn strip_label(target: FaderTarget) -> String {
    match target {
        FaderTarget::Channel(channel) => format!("CH {channel:02}"),
        FaderTarget::Aux(aux) => format!("AUX {aux:02}"),
        FaderTarget::Bus(bus) => format!("BUS {bus:02}"),
        FaderTarget::FxRtn(fx) => format!("FX {fx:02}"),
        FaderTarget::Mtx(mtx) => format!("MTX {mtx:02}"),
        FaderTarget::Dca(dca) => format!("DCA {dca}"),
        FaderTarget::Main => "LR".to_owned(),
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

fn toggle_button_style(active: bool, color: Color) -> button::Style {
    if active {
        button::Style {
            background: Some(Background::Color(color)),
            text_color: Color::from_rgb8(0x14, 0x18, 0x20),
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color,
            },
            ..Default::default()
        }
    } else {
        button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: color,
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color,
            },
            ..Default::default()
        }
    }
}

fn meter_subscription(mixer_addr: SocketAddr) -> Subscription<Message> {
    Subscription::run_with(mixer_addr, meter_worker).map(Message::MetersLoaded)
}

fn master_meter_subscription(mixer_addr: SocketAddr) -> Subscription<Message> {
    Subscription::run_with(mixer_addr, master_meter_worker).map(Message::MasterMetersLoaded)
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
                let _ = output
                    .send(Err(format!("failed to send /xremote: {error}")))
                    .await;
                return;
            }

            let mut last_xremote = Instant::now();
            let mut buffer = [0_u8; 4096];

            loop {
                if last_xremote.elapsed() >= Duration::from_secs(5) {
                    if let Err(error) = socket.send_to(XREMOTE_REQUEST, mixer_addr).await {
                        let _ = output
                            .send(Err(format!("failed to renew /xremote: {error}")))
                            .await;
                        return;
                    }
                    last_xremote = Instant::now();
                }

                match tokio::time::timeout(
                    Duration::from_millis(250),
                    socket.recv_from(&mut buffer),
                )
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
            }
        },
    )
    .boxed()
}

fn meter_worker(mixer_addr: &SocketAddr) -> BoxStream<'static, Result<Vec<StripMeter>, String>> {
    let mixer_addr = *mixer_addr;
    stream::channel(
        32,
        move |mut output: mpsc::Sender<Result<Vec<StripMeter>, String>>| async move {
            let socket = match bind_meter_socket().await {
                Ok(socket) => socket,
                Err(error) => {
                    let _ = output.send(Err(error.to_string())).await;
                    return;
                }
            };

            let subscribe = batchsubscribe_meter_request("meters/0", "/meters/0", 0, 0, 1);
            if let Err(error) = socket.send_to(XREMOTE_REQUEST, mixer_addr).await {
                let _ = output
                    .send(Err(format!("failed to send /xremote: {error}")))
                    .await;
                return;
            }
            if let Err(error) = socket.send_to(&subscribe, mixer_addr).await {
                let _ = output
                    .send(Err(format!(
                        "failed to send /batchsubscribe for meters/0: {error}"
                    )))
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
                        let _ = output
                            .send(Err(format!("failed to renew /xremote: {error}")))
                            .await;
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

                match tokio::time::timeout(
                    Duration::from_millis(250),
                    socket.recv_from(&mut buffer),
                )
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
        },
    )
    .boxed()
}

fn master_meter_worker(
    mixer_addr: &SocketAddr,
) -> BoxStream<'static, Result<MainMeterLevels, String>> {
    let mixer_addr = *mixer_addr;
    stream::channel(
        32,
        move |mut output: mpsc::Sender<Result<MainMeterLevels, String>>| async move {
            let socket = match bind_meter_socket().await {
                Ok(socket) => socket,
                Err(error) => {
                    let _ = output.send(Err(error.to_string())).await;
                    return;
                }
            };

            let subscribe = batchsubscribe_meter_request("meters/2", "/meters/2", 0, 0, 1);
            if let Err(error) = socket.send_to(XREMOTE_REQUEST, mixer_addr).await {
                let _ = output
                    .send(Err(format!("failed to send /xremote: {error}")))
                    .await;
                return;
            }
            if let Err(error) = socket.send_to(&subscribe, mixer_addr).await {
                let _ = output
                    .send(Err(format!(
                        "failed to send /batchsubscribe for meters/2: {error}"
                    )))
                    .await;
                return;
            }

            let mut last_renew = Instant::now();
            let mut buffer = [0_u8; 4096];

            loop {
                if last_renew.elapsed() >= Duration::from_secs(5) {
                    let renew = renew_request("meters/2");
                    if let Err(error) = socket.send_to(&renew, mixer_addr).await {
                        let _ = output
                            .send(Err(format!(
                                "failed to renew meter stream meters/2: {error}"
                            )))
                            .await;
                        return;
                    }
                    last_renew = Instant::now();
                }

                match tokio::time::timeout(
                    Duration::from_millis(250),
                    socket.recv_from(&mut buffer),
                )
                .await
                {
                    Ok(Ok((received, _))) => {
                        if let Ok(levels) = parse_main_meter_packet(&buffer[..received]) {
                            let _ = output.send(Ok(levels)).await;
                        }
                    }
                    Ok(Err(error)) => {
                        let _ = output
                            .send(Err(format!(
                                "failed while receiving main meter stream: {error}"
                            )))
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

async fn bind_meter_socket() -> std::io::Result<UdpSocket> {
    let socket = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
    Ok(socket)
}

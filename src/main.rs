use iced::widget::{column, container, text};
use iced::{Color, Element, Fill, Subscription, Task, Theme, time};
use mixosc::{
    ConnectionProbe, DiscoveredMixer, DiscoveryProbe, ProbeOutcome, ProbeResponse, parse_target,
};
use std::env;
use std::net::SocketAddr;
use std::time::Duration;

fn main() -> iced::Result {
    iced::application(new, update, view)
        .subscription(subscription)
        .theme(theme)
        .window_size(iced::Size::new(360.0, 180.0))
        .run()
}

#[derive(Debug)]
struct StatusApp {
    mixer_addr: Option<SocketAddr>,
    discovered_mixer: Option<DiscoveredMixer>,
    manual_target: bool,
    probe_in_flight: bool,
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
            app.status = ConnectionStatus::Checking;
            match app.mixer_addr {
                Some(mixer_addr) => spawn_probe(mixer_addr),
                None => spawn_discovery(),
            }
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
                        spawn_probe(mixer.addr)
                    } else {
                        app.mixer_addr = None;
                        app.discovered_mixer = None;
                        app.status = ConnectionStatus::Disconnected;
                        app.last_error =
                            Some("no X32 mixer discovered on the local network".to_owned());
                        Task::none()
                    }
                }
                Err(error) => {
                    app.mixer_addr = None;
                    app.discovered_mixer = None;
                    app.status = ConnectionStatus::Disconnected;
                    app.last_error = Some(error);
                    Task::none()
                }
            }
        }
        Message::ProbeFinished(result) => {
            app.probe_in_flight = false;

            match result {
                Ok(ProbeOutcome::Connected { response, .. }) => {
                    app.status = ConnectionStatus::Connected(response);
                    app.last_error = None;
                }
                Ok(ProbeOutcome::Disconnected) => {
                    app.status = ConnectionStatus::Disconnected;
                    app.last_error = None;
                    if !app.manual_target {
                        app.mixer_addr = None;
                        app.discovered_mixer = None;
                    }
                }
                Err(error) => {
                    app.status = ConnectionStatus::Disconnected;
                    app.last_error = Some(error);
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
    time::every(Duration::from_secs(1)).map(|_| Message::Tick)
}

fn theme(_app: &StatusApp) -> Theme {
    Theme::TokyoNight
}

fn view(app: &StatusApp) -> Element<'_, Message> {
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

    let content = column![
        text("X32 mixer status").size(28),
        text(address_line).size(16),
        text(label).size(44).color(color),
        text(identity_line).size(16),
        text(response_line).size(16),
        text(error_line)
            .size(14)
            .color(Color::from_rgb8(0xC7, 0xC9, 0xD3)),
    ]
    .spacing(8);

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

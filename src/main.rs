use iced::application;
use iced_fonts::LUCIDE_FONT_BYTES;
use mixosc::app::{new, subscription, theme, update, view};

fn main() -> iced::Result {
    application(new, update, view)
        .subscription(subscription)
        .theme(theme)
        .font(LUCIDE_FONT_BYTES)
        .window_size(iced::Size::new(720.0, 360.0))
        .run()
}

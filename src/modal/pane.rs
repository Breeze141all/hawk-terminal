use iced::{
    Alignment, Element, Length, padding,
    widget::{container, mouse_area, opaque},
};

pub mod alerts;
pub mod indicators;
pub mod mini_tickers_list;
pub mod replay_calendar;
pub mod settings;
pub mod stream;

#[derive(Debug, Clone, PartialEq)]
pub enum Modal {
    StreamModifier(super::stream::Modifier),
    MiniTickersList(mini_tickers_list::MiniPanel),
    Settings,
    Indicators,
    LinkGroup,
    Controls,
    Alerts,
    ReplayDatePicker(replay_calendar::ReplayDatePickerState),
}

pub fn stack_modal<'a, Message>(
    base: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    on_blur: Message,
    padding: padding::Padding,
    alignment: Alignment,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    stack_modal_positioned(base, content, on_blur, padding, alignment, Alignment::Start)
}

pub fn stack_modal_positioned<'a, Message>(
    base: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    on_blur: Message,
    padding: padding::Padding,
    align_x: Alignment,
    align_y: Alignment,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    iced::widget::stack![
        base.into(),
        mouse_area(
            container(opaque(content))
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(padding)
                .align_x(align_x)
                .align_y(align_y)
        )
        .on_press(on_blur)
    ]
    .into()
}

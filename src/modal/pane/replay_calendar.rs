use crate::screen::dashboard::pane::{Event, Message};
use crate::style::{self, Icon, icon_text};
use chrono::{Datelike, NaiveDate, Timelike};
use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{button, column, container, pane_grid, row, rule, space, text};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

#[derive(Debug, Clone, Copy)]
pub struct CalendarIconProgram {
    pub color: Option<Color>,
    pub is_active: bool,
}

impl<Message> canvas::Program<Message> for CalendarIconProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let stroke_color = if let Some(c) = self.color {
            c
        } else if self.is_active {
            palette.primary.weak.text
        } else {
            palette.background.base.text
        };

        let stroke = Stroke {
            style: canvas::Style::Solid(stroke_color),
            width: 1.1,
            line_cap: canvas::LineCap::Round,
            line_join: canvas::LineJoin::Round,
            ..Default::default()
        };

        let w = bounds.width;
        let h = bounds.height;
        let pad_x = 1.2;
        let pad_top = 2.2;
        let body_w = w - 2.0 * pad_x;
        let body_h = h - pad_top - 1.2;

        let body = Path::rounded_rectangle(
            Point::new(pad_x, pad_top),
            Size::new(body_w, body_h),
            1.5.into(),
        );
        frame.stroke(&body, stroke);

        let header_y = pad_top + body_h * 0.28;
        frame.stroke(
            &Path::line(
                Point::new(pad_x, header_y),
                Point::new(pad_x + body_w, header_y),
            ),
            Stroke {
                width: 0.9,
                ..stroke
            },
        );

        let pin_l = pad_x + body_w * 0.25;
        let pin_r = pad_x + body_w * 0.75;
        frame.stroke(
            &Path::line(Point::new(pin_l, 0.8), Point::new(pin_l, pad_top + 0.8)),
            stroke,
        );
        frame.stroke(
            &Path::line(Point::new(pin_r, 0.8), Point::new(pin_r, pad_top + 0.8)),
            stroke,
        );

        let dot_r = (w * 0.06).clamp(0.6, 0.9);
        let col1 = pad_x + body_w * 0.26;
        let col2 = pad_x + body_w * 0.50;
        let col3 = pad_x + body_w * 0.74;
        let row1 = header_y + (pad_top + body_h - header_y) * 0.35;
        let row2 = header_y + (pad_top + body_h - header_y) * 0.70;

        frame.fill(&Path::circle(Point::new(col1, row1), dot_r), stroke_color);
        frame.fill(&Path::circle(Point::new(col2, row1), dot_r), stroke_color);
        frame.fill(&Path::circle(Point::new(col3, row1), dot_r), stroke_color);
        frame.fill(&Path::circle(Point::new(col1, row2), dot_r), stroke_color);
        frame.fill(&Path::circle(Point::new(col2, row2), dot_r), stroke_color);
        frame.fill(&Path::circle(Point::new(col3, row2), dot_r), stroke_color);

        vec![frame.into_geometry()]
    }
}

pub fn calendar_icon<'a, Message: 'a>(
    color: Option<Color>,
    is_active: bool,
    size: f32,
) -> Element<'a, Message> {
    Canvas::new(CalendarIconProgram { color, is_active })
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

#[derive(Debug, Clone, Copy)]
pub struct RandomIconProgram {
    pub color: Option<Color>,
}

impl<Message> canvas::Program<Message> for RandomIconProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let stroke_color = self.color.unwrap_or(palette.background.base.text);

        let stroke = Stroke {
            style: canvas::Style::Solid(stroke_color),
            width: 1.1,
            line_cap: canvas::LineCap::Round,
            line_join: canvas::LineJoin::Round,
            ..Default::default()
        };

        let w = bounds.width;
        let h = bounds.height;
        let pad = 1.2;
        let die_w = w - 2.0 * pad;
        let die_h = h - 2.0 * pad;

        let die_path =
            Path::rounded_rectangle(Point::new(pad, pad), Size::new(die_w, die_h), 1.8.into());
        frame.stroke(&die_path, stroke);

        let pip_r = (w * 0.075).clamp(0.7, 1.1);
        let left = pad + die_w * 0.26;
        let right = pad + die_w * 0.74;
        let top = pad + die_h * 0.26;
        let bot = pad + die_h * 0.74;
        let mid_x = pad + die_w * 0.5;
        let mid_y = pad + die_h * 0.5;

        frame.fill(&Path::circle(Point::new(left, top), pip_r), stroke_color);
        frame.fill(&Path::circle(Point::new(right, top), pip_r), stroke_color);
        frame.fill(&Path::circle(Point::new(mid_x, mid_y), pip_r), stroke_color);
        frame.fill(&Path::circle(Point::new(left, bot), pip_r), stroke_color);
        frame.fill(&Path::circle(Point::new(right, bot), pip_r), stroke_color);

        vec![frame.into_geometry()]
    }
}

pub fn random_icon<'a, Message: 'a>(color: Option<Color>, size: f32) -> Element<'a, Message> {
    Canvas::new(RandomIconProgram { color })
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

#[derive(Debug, Clone, Copy)]
pub struct StepBackwardIconProgram {
    pub color: Option<Color>,
}

impl<Message> canvas::Program<Message> for StepBackwardIconProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let fill_color = self.color.unwrap_or(palette.background.base.text);

        let w = bounds.width;
        let h = bounds.height;
        let top = h * 0.15;
        let bot = h * 0.85;
        let mid_y = h * 0.5;

        // Left triangle pointing left
        let tri1 = Path::new(|b| {
            b.move_to(Point::new(w * 0.08, mid_y));
            b.line_to(Point::new(w * 0.48, top));
            b.line_to(Point::new(w * 0.48, bot));
            b.close();
        });
        frame.fill(&tri1, fill_color);

        // Right triangle pointing left
        let tri2 = Path::new(|b| {
            b.move_to(Point::new(w * 0.52, mid_y));
            b.line_to(Point::new(w * 0.92, top));
            b.line_to(Point::new(w * 0.92, bot));
            b.close();
        });
        frame.fill(&tri2, fill_color);

        vec![frame.into_geometry()]
    }
}

pub fn step_backward_icon<'a, Message: 'a>(
    color: Option<Color>,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    Canvas::new(StepBackwardIconProgram { color })
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .into()
}

#[derive(Debug, Clone, Copy)]
pub struct StepForwardIconProgram {
    pub color: Option<Color>,
}

impl<Message> canvas::Program<Message> for StepForwardIconProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let fill_color = self.color.unwrap_or(palette.background.base.text);

        let w = bounds.width;
        let h = bounds.height;
        let top = h * 0.15;
        let bot = h * 0.85;
        let mid_y = h * 0.5;

        // Left triangle pointing right
        let tri1 = Path::new(|b| {
            b.move_to(Point::new(w * 0.08, top));
            b.line_to(Point::new(w * 0.48, mid_y));
            b.line_to(Point::new(w * 0.08, bot));
            b.close();
        });
        frame.fill(&tri1, fill_color);

        // Right triangle pointing right
        let tri2 = Path::new(|b| {
            b.move_to(Point::new(w * 0.52, top));
            b.line_to(Point::new(w * 0.92, mid_y));
            b.line_to(Point::new(w * 0.52, bot));
            b.close();
        });
        frame.fill(&tri2, fill_color);

        vec![frame.into_geometry()]
    }
}

pub fn step_forward_icon<'a, Message: 'a>(
    color: Option<Color>,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    Canvas::new(StepForwardIconProgram { color })
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .into()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayDatePickerState {
    pub view_year: i32,
    pub view_month: u32,
    pub selected_date: NaiveDate,
    pub hour: u32,
    pub minute: u32,
}

impl ReplayDatePickerState {
    pub fn new(cutoff_time: u64) -> Self {
        let dt = if cutoff_time > 0 {
            chrono::DateTime::from_timestamp_millis(cutoff_time as i64)
                .unwrap_or_else(chrono::Utc::now)
        } else {
            chrono::Utc::now()
        };

        Self {
            view_year: dt.year(),
            view_month: dt.month(),
            selected_date: dt.date_naive(),
            hour: dt.hour(),
            minute: dt.minute(),
        }
    }

    pub fn to_timestamp_millis(&self) -> Option<u64> {
        let naive_dt = self
            .selected_date
            .and_hms_opt(self.hour.min(23), self.minute.min(59), 0)?;
        let utc_dt = naive_dt.and_utc();
        Some(utc_dt.timestamp_millis() as u64)
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    PrevMonth,
    NextMonth,
    PrevYear,
    NextYear,
    SelectDate(NaiveDate),
    AdjustHour(i32),
    AdjustMinute(i32),
    SetTime(u32, u32),
    RandomBar,
    JumpToToday,
    Close,
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "",
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };

    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(30)
}

pub fn view<'a>(pane: pane_grid::Pane, state: &ReplayDatePickerState) -> Element<'a, Message> {
    let to_msg = move |act: Action| Message::PaneEvent(pane, Event::ReplayDatePickerAction(act));

    let header = row![
        button(text("«").size(11).font(style::AZERET_MONO))
            .style(|theme, status| style::button::transparent(theme, status, false))
            .on_press(to_msg(Action::PrevYear))
            .padding([2, 5]),
        button(text("‹").size(12).font(style::AZERET_MONO))
            .style(|theme, status| style::button::transparent(theme, status, false))
            .on_press(to_msg(Action::PrevMonth))
            .padding([2, 5]),
        text(format!(
            "{} {}",
            month_name(state.view_month),
            state.view_year
        ))
        .size(12)
        .font(style::AZERET_MONO),
        button(text("›").size(12).font(style::AZERET_MONO))
            .style(|theme, status| style::button::transparent(theme, status, false))
            .on_press(to_msg(Action::NextMonth))
            .padding([2, 5]),
        button(text("»").size(11).font(style::AZERET_MONO))
            .style(|theme, status| style::button::transparent(theme, status, false))
            .on_press(to_msg(Action::NextYear))
            .padding([2, 5]),
        space(),
        button(icon_text(Icon::Close, 11))
            .style(|theme, status| style::button::transparent(theme, status, false))
            .on_press(to_msg(Action::Close))
            .padding([2, 4]),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let weekdays = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
    let weekday_header = row(weekdays.iter().map(|&d| {
        container(
            text(d)
                .size(10)
                .font(style::AZERET_MONO)
                .style(|theme: &Theme| {
                    let p = theme.extended_palette();
                    text::Style {
                        color: Some(p.background.weak.text),
                    }
                }),
        )
        .width(32)
        .align_x(Alignment::Center)
        .into()
    }))
    .spacing(2);

    let first_day = NaiveDate::from_ymd_opt(state.view_year, state.view_month, 1)
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
    let offset = first_day.weekday().num_days_from_monday() as usize;
    let total_days = days_in_month(state.view_year, state.view_month);
    let today = chrono::Utc::now().date_naive();

    let mut day_cells: Vec<Element<'a, Message>> = Vec::with_capacity(42);

    // Leading empty cells
    for _ in 0..offset {
        day_cells.push(space().width(32).height(24).into());
    }

    // Days in current month
    for day in 1..=total_days {
        if let Some(date) = NaiveDate::from_ymd_opt(state.view_year, state.view_month, day) {
            let is_selected = state.selected_date == date;
            let is_today = date == today;

            let btn = button(text(format!("{day}")).size(11).font(style::AZERET_MONO))
                .width(32)
                .height(24)
                .padding([1, 0])
                .style(move |theme: &Theme, status| {
                    let p = theme.extended_palette();
                    if is_selected {
                        button::Style {
                            background: Some(p.primary.weak.color.into()),
                            text_color: p.primary.weak.text,
                            border: iced::Border {
                                radius: 2.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    } else {
                        let is_hovered = matches!(status, button::Status::Hovered);
                        let bg = if is_hovered {
                            Some(p.background.weak.color.into())
                        } else {
                            None
                        };
                        let border = if is_today {
                            iced::Border {
                                radius: 2.0.into(),
                                width: 1.0,
                                color: p.primary.base.color,
                            }
                        } else {
                            iced::Border::default()
                        };
                        button::Style {
                            background: bg,
                            text_color: p.background.base.text,
                            border,
                            ..Default::default()
                        }
                    }
                })
                .on_press(to_msg(Action::SelectDate(date)));

            day_cells.push(btn.into());
        }
    }

    // Pad trailing cells to fill row
    while !day_cells.len().is_multiple_of(7) {
        day_cells.push(space().width(32).height(24).into());
    }

    let mut calendar_rows: Vec<Element<'a, Message>> = Vec::new();
    let mut current_row: Vec<Element<'a, Message>> = Vec::with_capacity(7);
    for cell in day_cells {
        current_row.push(cell);
        if current_row.len() == 7 {
            calendar_rows.push(row(std::mem::take(&mut current_row)).spacing(2).into());
        }
    }
    if !current_row.is_empty() {
        calendar_rows.push(row(current_row).spacing(2).into());
    }
    let calendar_grid = column(calendar_rows).spacing(2);

    let time_box_style = |theme: &Theme| {
        let p = theme.extended_palette();
        container::Style {
            background: Some(p.background.weak.color.into()),
            text_color: Some(p.background.base.text),
            border: iced::Border {
                radius: 2.0.into(),
                width: 1.0,
                color: p.background.strong.color,
            },
            ..Default::default()
        }
    };

    let time_section = row![
        text("Time:").size(10).font(style::AZERET_MONO),
        row![
            button(text("−").size(10).font(style::AZERET_MONO))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(to_msg(Action::AdjustHour(-1)))
                .padding([1, 4]),
            container(
                text(format!("{:02}", state.hour))
                    .size(11)
                    .font(style::AZERET_MONO)
            )
            .padding([1, 5])
            .style(time_box_style),
            button(text("+").size(10).font(style::AZERET_MONO))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(to_msg(Action::AdjustHour(1)))
                .padding([1, 4]),
        ]
        .spacing(1)
        .align_y(Alignment::Center),
        text(":").size(11).font(style::AZERET_MONO),
        row![
            button(text("−").size(10).font(style::AZERET_MONO))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(to_msg(Action::AdjustMinute(-5)))
                .padding([1, 4]),
            container(
                text(format!("{:02}", state.minute))
                    .size(11)
                    .font(style::AZERET_MONO)
            )
            .padding([1, 5])
            .style(time_box_style),
            button(text("+").size(10).font(style::AZERET_MONO))
                .style(|theme, status| style::button::transparent(theme, status, false))
                .on_press(to_msg(Action::AdjustMinute(5)))
                .padding([1, 4]),
        ]
        .spacing(1)
        .align_y(Alignment::Center),
        space(),
        button(text("00:00").size(9).font(style::AZERET_MONO))
            .style(|theme, status| style::button::transparent(theme, status, false))
            .on_press(to_msg(Action::SetTime(0, 0)))
            .padding([1, 3]),
        button(text("12:00").size(9).font(style::AZERET_MONO))
            .style(|theme, status| style::button::transparent(theme, status, false))
            .on_press(to_msg(Action::SetTime(12, 0)))
            .padding([1, 3]),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let actions = row![
        button(
            row![
                random_icon(None, 12.0),
                text("Random Bar").size(11).font(style::AZERET_MONO),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .style(|theme, status| style::button::confirm(theme, status, false))
        .on_press(to_msg(Action::RandomBar))
        .padding([4, 10]),
        space(),
        button(text("Today").size(10).font(style::AZERET_MONO))
            .style(|theme, status| style::button::transparent(theme, status, false))
            .on_press(to_msg(Action::JumpToToday))
            .padding([3, 6]),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let content = column![
        header,
        rule::horizontal(1),
        weekday_header,
        calendar_grid,
        rule::horizontal(1),
        time_section,
        rule::horizontal(1),
        actions,
    ]
    .spacing(6)
    .width(Length::Fixed(246.0));

    container(content)
        .padding([8, 10])
        .style(style::chart_modal)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calendar_state_and_timestamp_roundtrip() {
        let ts = 1710504000000u64; // 2024-03-15 12:00:00 UTC
        let state = ReplayDatePickerState::new(ts);
        assert_eq!(state.view_year, 2024);
        assert_eq!(state.view_month, 3);
        assert_eq!(
            state.selected_date,
            NaiveDate::from_ymd_opt(2024, 3, 15).unwrap()
        );
        assert_eq!(state.hour, 12);
        assert_eq!(state.minute, 0);

        let roundtrip = state.to_timestamp_millis().unwrap();
        assert_eq!(roundtrip, ts);
    }

    #[test]
    fn test_days_in_month() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2025, 2), 28);
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 4), 30);
        assert_eq!(days_in_month(2024, 12), 31);
    }
}

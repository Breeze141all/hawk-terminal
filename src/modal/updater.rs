use crate::style::{self, Icon, icon_text};
use crate::updater::UpdateStatus;
use crate::widget::scrollable_content;
use iced::widget::{button, column, container, row, rule, text};
use iced::{Alignment, Element, Length};
use std::path::PathBuf;

pub fn update_modal_view<'a, Message: Clone + 'a>(
    status: &'a UpdateStatus,
    on_start_download: Message,
    on_cancel_download: Message,
    on_install: impl Fn(PathBuf) -> Message + 'a,
    on_close: Message,
) -> Element<'a, Message> {
    let header = row![
        icon_text(Icon::Cog, 18),
        column![
            text("Оновлення Hawk Terminal").size(16),
            text(format!("Поточна версія: v{}", env!("CARGO_PKG_VERSION"))).size(11),
        ]
        .spacing(2),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let content: Element<'a, Message> = match status {
        UpdateStatus::Idle | UpdateStatus::Checking => column![
            text("Перевірка наявності нових версій...").size(13),
            rule::horizontal(1.0).style(style::split_ruler),
            row![
                button(text("Закрити").size(12))
                    .padding([6, 16])
                    .on_press(on_close)
                    .style(|theme, status| style::button::transparent(theme, status, false)),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(12)
        .into(),

        UpdateStatus::UpToDate => column![
            row![
                icon_text(Icon::Checkmark, 16),
                text("У вас встановлена найновіша версія Hawk Terminal.").size(13),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            rule::horizontal(1.0).style(style::split_ruler),
            row![
                button(text("Зрозуміло").size(12))
                    .padding([6, 16])
                    .on_press(on_close)
                    .style(|theme, status| style::button::transparent(theme, status, false)),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(12)
        .into(),

        UpdateStatus::Available(release) => {
            let version_badge = row![
                text(format!("Доступна версія: v{}", release.version)).size(14),
                if let Some(date) = &release.release_date {
                    text(format!("(від {date})")).size(12)
                } else {
                    text("").size(0)
                },
            ]
            .spacing(8)
            .align_y(Alignment::Center);

            let mut changelog_col = column![text("Що нового:").size(12),].spacing(4);

            if let Some(list) = &release.changelog {
                for item in list {
                    changelog_col = changelog_col.push(text(format!("• {item}")).size(11));
                }
            } else {
                changelog_col = changelog_col
                    .push(text("• Покращення продуктивності та стабільності роботи.").size(11));
            }

            let changelog_box = container(scrollable_content(changelog_col))
                .style(style::modal_container)
                .padding(10)
                .width(Length::Fill)
                .max_height(140.0);

            let safety_notice = container(
                row![
                    icon_text(Icon::Locked, 14),
                    text("Ваші дані (журнал угод, малювання на графіках, лейаути та алерти) автоматично зберігаються й резервуються.")
                        .size(11),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .style(style::modal_container)
            .padding(8)
            .width(Length::Fill);

            let actions = row![
                button(text("Пізніше").size(12))
                    .padding([6, 16])
                    .on_press(on_close)
                    .style(|theme, status| style::button::transparent(theme, status, false)),
                button(text("Завантажити та оновити").size(12))
                    .padding([6, 18])
                    .on_press(on_start_download)
                    .style(|theme, status| style::button::modifier(theme, status, true)),
            ]
            .spacing(10)
            .align_y(Alignment::Center);

            column![
                version_badge,
                changelog_box,
                safety_notice,
                rule::horizontal(1.0).style(style::split_ruler),
                actions,
            ]
            .spacing(12)
            .into()
        }

        UpdateStatus::Downloading {
            release,
            progress,
            bytes_downloaded,
            total_bytes,
        } => {
            let pct = (progress * 100.0).clamp(0.0, 100.0);
            let mb_downloaded = *bytes_downloaded as f64 / 1_048_576.0;
            let total_str = match total_bytes {
                Some(total) => format!("{:.1} МБ", *total as f64 / 1_048_576.0),
                None => "невідомо".to_string(),
            };

            let progress_text = text(format!(
                "Завантаження v{}: {:.0}% ({:.1} МБ / {})",
                release.version, pct, mb_downloaded, total_str
            ))
            .size(12);

            let progress_bar_bg = container(
                container(iced::widget::space())
                    .width(Length::FillPortion((pct as u16).max(1)))
                    .height(Length::Fixed(8.0))
                    .style(|theme: &iced::Theme| container::Style {
                        background: Some(theme.extended_palette().primary.base.color.into()),
                        border: iced::Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            )
            .width(Length::Fill)
            .height(Length::Fixed(8.0))
            .style(|theme: &iced::Theme| container::Style {
                background: Some(theme.extended_palette().background.weak.color.into()),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });

            let actions = row![
                button(text("Скасувати").size(12))
                    .padding([6, 16])
                    .on_press(on_cancel_download)
                    .style(|theme, status| style::button::transparent(theme, status, false)),
            ]
            .spacing(10)
            .align_y(Alignment::Center);

            column![
                progress_text,
                progress_bar_bg,
                rule::horizontal(1.0).style(style::split_ruler),
                actions,
            ]
            .spacing(12)
            .into()
        }

        UpdateStatus::ReadyToInstall {
            release,
            installer_path,
            backup_path,
        } => {
            let ready_text = row![
                icon_text(Icon::Checkmark, 16),
                text(format!(
                    "Оновлення v{} готове до встановлення!",
                    release.version
                ))
                .size(13),
            ]
            .spacing(8)
            .align_y(Alignment::Center);

            let backup_info = if let Some(bp) = backup_path {
                format!(
                    "✓ Створено резервну копію даних у: {}",
                    bp.file_name().and_then(|n| n.to_str()).unwrap_or("backups")
                )
            } else {
                "✓ Усі дані користувача захищено.".to_string()
            };

            let info_box = container(
                column![
                    text(backup_info).size(11),
                    text("При натисканні кнопки нижче програма завершить роботу та запустить інсталятор для швидкого оновлення.")
                        .size(11),
                ]
                .spacing(4),
            )
            .style(style::modal_container)
            .padding(10)
            .width(Length::Fill);

            let install_msg = on_install(installer_path.clone());
            let actions = row![
                button(text("Пізніше").size(12))
                    .padding([6, 16])
                    .on_press(on_close)
                    .style(|theme, status| style::button::transparent(theme, status, false)),
                button(text("Встановити та перезапустити").size(12))
                    .padding([6, 18])
                    .on_press(install_msg)
                    .style(|theme, status| style::button::modifier(theme, status, true)),
            ]
            .spacing(10)
            .align_y(Alignment::Center);

            column![
                ready_text,
                info_box,
                rule::horizontal(1.0).style(style::split_ruler),
                actions,
            ]
            .spacing(12)
            .into()
        }

        UpdateStatus::Error(err) => {
            let err_row = row![
                icon_text(Icon::Close, 16),
                text(format!("Помилка оновлення: {err}")).size(12),
            ]
            .spacing(8)
            .align_y(Alignment::Center);

            let actions = row![
                button(text("Закрити").size(12))
                    .padding([6, 16])
                    .on_press(on_close)
                    .style(|theme, status| style::button::transparent(theme, status, false)),
            ]
            .align_y(Alignment::Center);

            column![
                err_row,
                rule::horizontal(1.0).style(style::split_ruler),
                actions,
            ]
            .spacing(12)
            .into()
        }
    };

    let dialog_content = column![
        header,
        rule::horizontal(1.0).style(style::split_ruler),
        content,
    ]
    .spacing(12)
    .width(Length::Fixed(460.0));

    container(dialog_content)
        .style(style::dashboard_modal)
        .padding(16)
        .into()
}

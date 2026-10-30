use crate::{
    app::{App, EditorPane},
    message::Message,
    state::{AppState, EditorBlock, EditorParameter},
};
use iced::widget::{
    button, column, container, pane_grid, pick_list, progress_bar, row, rule, scrollable, slider,
    space, text, text_input,
};
use iced::{Alignment, Color, Element, Length, Theme};
use openhx_core::Setlist;
use openhx_i18n::fl;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetlistChoice {
    pub index: u8,
    pub name: String,
}

impl From<&Setlist> for SetlistChoice {
    fn from(setlist: &Setlist) -> Self {
        Self {
            index: setlist.index,
            name: setlist.name.clone(),
        }
    }
}

impl std::fmt::Display for SetlistChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.name.is_empty() {
            let number = u32::from(self.index) + 1;
            write!(f, "{}", fl!("setlist-option", number = number))
        } else {
            write!(f, "{}", self.name)
        }
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    let content = match app.state {
        AppState::Waiting => column![
            text(fl!("waiting-title")).size(30),
            text(fl!("waiting-subtitle")),
            button(text("Open offline editor"))
                .on_press(Message::EnterOfflineMode)
                .padding([8, 14])
                .style(button::secondary)
        ]
        .align_x(Alignment::Center)
        .spacing(15)
        .into(),
        AppState::Connected => editor_workspace(app),
        AppState::Offline => editor_workspace(app),
        AppState::Error => {
            let error_msg = app
                .error_log
                .clone()
                .unwrap_or_else(|| fl!("error-unknown"));

            column![text(fl!("error-title")).size(30), text(error_msg)]
                .align_x(Alignment::Center)
                .spacing(15)
                .into()
        }
    };

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(12)
        .into()
}

fn editor_workspace(app: &App) -> Element<'_, Message> {
    let header = top_bar(app);
    let footer = snapshot_bar(app);
    let panes = pane_grid(&app.panes, |_pane, pane, _maximized| {
        let content: Element<'_, Message> = match pane {
            EditorPane::Workspace => row![preset_browser(app), signal_chain(app)]
                .spacing(10)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            EditorPane::Inspector => parameter_inspector(app),
        };
        pane_grid::Content::new(content)
    })
    .on_resize(8, Message::PaneResized)
    .width(Length::Fill)
    .height(Length::Fill);

    column![header, rule::horizontal(1), panes, footer,]
        .spacing(10)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn top_bar(app: &App) -> Element<'_, Message> {
    let device = container(
        text(if app.state == AppState::Offline {
            "Offline editor".to_string()
        } else {
            fl!("connected-header", device_name = app.device_name.clone())
        })
        .size(12)
        .style(|_theme: &Theme| text::Style {
            color: Some(Color::from_rgb(0.45, 0.9, 0.55)),
        }),
    )
    .padding([5, 10])
    .style(move |_theme: &Theme| container::Style {
        background: Some(Color::from_rgb(0.08, 0.13, 0.1).into()),
        border: iced::Border {
            color: Color::from_rgb(0.2, 0.55, 0.3),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    });

    let preset_name = text_input("Preset name", &app.editor.preset_name)
        .on_input(Message::PresetNameChanged)
        .on_submit(Message::RenamePreset)
        .width(Length::Fixed(240.0))
        .padding([7, 10]);

    let rename = button(text("Rename"))
        .on_press(Message::RenamePreset)
        .padding([7, 12])
        .style(button::secondary);
    let save = button(text("Save"))
        .on_press(Message::SavePreset)
        .padding([7, 12])
        .style(button::primary);

    let mode_button = if app.state == AppState::Offline {
        button(text("Close offline editor"))
            .on_press(Message::ExitOfflineMode)
            .style(button::secondary)
    } else {
        button(text("Offline"))
            .on_press(Message::EnterOfflineMode)
            .style(button::secondary)
    }
    .padding([7, 12]);

    row![
        text("OpenHX").size(22),
        rule::vertical(24),
        preset_name,
        rename,
        save,
        space::horizontal(),
        text("HX Edit workspace").size(13),
        device,
        mode_button,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

fn preset_browser(app: &App) -> Element<'_, Message> {
    let choices: Vec<SetlistChoice> = app.setlists.iter().map(SetlistChoice::from).collect();
    let selected = choices.iter().find(|c| c.index == app.setlist).cloned();
    let picker = if choices.len() > 1 {
        Some(
            pick_list(choices, selected, |choice| {
                Message::SetlistChosen(choice.index)
            })
            .width(Length::Fill)
            .text_size(12),
        )
    } else {
        None
    };

    let mut list = column![text("PRESETS").size(11)].spacing(5);
    if let Some(picker) = picker {
        list = list.push(picker);
    }

    for (position, preset) in app.presets.iter().enumerate() {
        let selected = app.selected_preset == Some(preset.index);
        let item = button(
            row![
                text(format!("{:03}", preset.index))
                    .width(Length::Fixed(28.0))
                    .size(11),
                text(&preset.name).size(12),
            ]
            .spacing(7),
        )
        .width(Length::Fill)
        .padding([6, 7])
        .on_press(Message::PresetSelected(preset.index))
        .style(move |_theme: &Theme, status| {
            let background = if selected {
                Color::from_rgb(0.12, 0.34, 0.58)
            } else if position % 2 == 0 {
                Color::from_rgb(0.12, 0.12, 0.13)
            } else {
                Color::from_rgb(0.09, 0.09, 0.1)
            };
            let border_color = if matches!(status, button::Status::Hovered) {
                Color::from_rgb(0.3, 0.55, 0.85)
            } else {
                background
            };
            button::Style {
                background: Some(background.into()),
                text_color: Color::WHITE,
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        });
        list = list.push(item);
    }

    panel(
        scrollable(list).height(Length::Fill),
        Color::from_rgb(0.22, 0.3, 0.35),
    )
    .width(Length::Fixed(205.0))
    .into()
}

fn signal_chain(app: &App) -> Element<'_, Message> {
    let canvas = container(
        column![
            row![
                text("SIGNAL CHAIN").size(11),
                space::horizontal(),
                text("Preset edit buffer").size(11)
            ],
            rule::horizontal(1),
            pane_grid(&app.block_panes, |_pane, slot, _maximized| {
                let block = app.editor.blocks.iter().find(|block| block.slot == *slot);
                pane_grid::Content::new(match block {
                    Some(_) => Element::from(text("")),
                    None => Element::from(text("No block")),
                })
                .title_bar(pane_grid::TitleBar::new(match block {
                    Some(block) => block_card(block, app.editor.selected_block == block.slot),
                    None => text("No block").into(),
                }))
            })
            .on_drag(Message::BlockDragged)
            .spacing(8)
            .min_size(140)
            .height(Length::Fill),
            row![
                button(text("+ Block").size(12))
                    .padding([6, 10])
                    .style(button::secondary),
                space::horizontal(),
                text("Drag blocks to reorder the signal chain").size(11),
            ]
        ]
        .spacing(10),
    )
    .padding(12)
    .width(Length::FillPortion(6))
    .height(Length::Fill)
    .style(move |_theme: &Theme| container::Style {
        background: Some(Color::from_rgb(0.07, 0.07, 0.08).into()),
        border: iced::Border {
            color: Color::from_rgb(0.2, 0.2, 0.23),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    });

    canvas.into()
}

fn block_card(block: &EditorBlock, selected: bool) -> Element<'_, Message> {
    let color = block_color(&block.model_id);
    button(
        column![
            container(
                text(block.model.clone())
                    .size(10)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center)
                    .wrapping(text::Wrapping::WordOrGlyph),
            )
            .width(Length::Fill)
            .padding([5, 7])
            .style(move |_theme: &Theme| container::Style {
                background: Some(color.into()),
                ..Default::default()
            }),
            text(block.name.clone())
                .size(12)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .wrapping(text::Wrapping::WordOrGlyph),
            text(format!("Slot {}", block.slot)).size(10),
        ]
        .spacing(5)
        .align_x(Alignment::Center),
    )
    .width(Length::Fixed(140.0))
    .padding(0)
    .on_press(Message::BlockSelected(block.slot))
    .style(move |_theme: &Theme, _status| button::Style {
        background: Some(if selected {
            Color::from_rgb(0.14, 0.3, 0.48).into()
        } else {
            Color::from_rgb(0.12, 0.12, 0.14).into()
        }),
        text_color: Color::WHITE,
        border: iced::Border {
            color: if selected {
                Color::from_rgb(0.35, 0.65, 0.95)
            } else {
                Color::from_rgb(0.28, 0.28, 0.3)
            },
            width: if selected { 2.0 } else { 1.0 },
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn parameter_inspector(app: &App) -> Element<'_, Message> {
    let Some(block) = app.editor.selected_block() else {
        return panel(
            text("Select a block to edit"),
            Color::from_rgb(0.32, 0.58, 0.72),
        )
        .into();
    };

    let mut controls = column![
        text("PARAMETERS").size(11),
        text(block.name.clone()).size(19),
        text(block.model.clone()).size(11),
        rule::horizontal(1),
    ]
    .spacing(10);

    for parameter in &block.parameters {
        controls = controls.push(parameter_control(
            block.slot,
            parameter,
            block_color(&block.model_id),
        ));
    }

    let color = block_color(&block.model_id);
    let ir_status: Element<'_, Message> = match &app.loaded_ir {
        Some(ir) => text(format!(
            "IR: {} samples, {} channel(s)\n{}",
            ir.samples, ir.channels, ir.path
        ))
        .size(11)
        .into(),
        None => text("No IR loaded").size(11).into(),
    };
    panel(
        column![
            button(text("Load IR WAV"))
                .on_press(Message::LoadIr)
                .padding([6, 10])
                .style(button::secondary),
            ir_status,
            scrollable(controls).height(Length::Fill),
        ]
        .spacing(8),
        color,
    )
    .into()
}

fn parameter_control(block: u8, parameter: &EditorParameter, color: Color) -> Element<'_, Message> {
    let value = parameter.value;
    let label = row![
        text(parameter.name.clone()).size(12),
        space::horizontal(),
        text(format!("{:.2}", value)).size(11),
    ];
    let control = slider(parameter.min..=parameter.max, value, {
        let index = parameter.index;
        move |value| Message::ParameterChanged(index, value)
    })
    .step(0.01_f32)
    .width(Length::Fill)
    .on_release(Message::ParameterEditReleased {
        block,
        parameter: parameter.index,
    });

    column![
        label,
        control,
        progress_bar(parameter.min..=parameter.max, value).style(move |_theme| {
            progress_bar::Style {
                background: color.scale_alpha(0.25).into(),
                bar: color.into(),
                border: iced::Border::default(),
            }
        })
    ]
    .spacing(5)
    .into()
}

fn snapshot_bar(app: &App) -> Element<'_, Message> {
    let mut snapshots = row![text("SNAPSHOTS").size(11)].spacing(6);
    for snapshot in 0..4u8 {
        let label = format!("{}", snapshot + 1);
        let selected = app.editor.selected_snapshot == snapshot;
        snapshots = snapshots.push(
            button(text(label))
                .width(Length::Fixed(34.0))
                .padding(6)
                .on_press(Message::SnapshotSelected(snapshot))
                .style(if selected {
                    button::primary
                } else {
                    button::secondary
                }),
        );
    }

    let footswitches = row![
        text("FOOTSWITCHES").size(11),
        button(text("FS1  Clean")).style(button::secondary),
        button(text("FS2  Drive")).style(button::secondary),
        button(text("FS3  Delay")).style(button::secondary),
        button(text("FS4  Reverb")).style(button::secondary),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    container(row![snapshots, space::horizontal(), footswitches].align_y(Alignment::Center))
        .padding([8, 10])
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(Color::from_rgb(0.1, 0.1, 0.11).into()),
            border: iced::Border {
                color: Color::from_rgb(0.25, 0.25, 0.28),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn block_color(model_id: &str) -> Color {
    let id = model_id.to_ascii_lowercase();
    if id.contains("amp") || id.contains("preamp") {
        Color::from_rgb(0.72, 0.3, 0.18)
    } else if id.contains("cab") || id.contains("ir") {
        Color::from_rgb(0.42, 0.32, 0.65)
    } else if id.contains("delay") {
        Color::from_rgb(0.2, 0.48, 0.72)
    } else if id.contains("reverb") {
        Color::from_rgb(0.2, 0.62, 0.58)
    } else if id.contains("mod") || id.contains("chorus") || id.contains("flanger") {
        Color::from_rgb(0.28, 0.58, 0.35)
    } else if id.contains("dist") || id.contains("fuzz") || id.contains("overdrive") {
        Color::from_rgb(0.78, 0.48, 0.18)
    } else if id.contains("comp") {
        Color::from_rgb(0.7, 0.58, 0.2)
    } else if id.contains("eq") || id.contains("filter") {
        Color::from_rgb(0.58, 0.35, 0.68)
    } else if id.contains("gate") {
        Color::from_rgb(0.36, 0.42, 0.48)
    } else {
        Color::from_rgb(0.3, 0.48, 0.55)
    }
}

fn panel<'a>(
    content: impl Into<Element<'a, Message>>,
    accent: Color,
) -> iced::widget::Container<'a, Message> {
    container(content)
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Color::from_rgb(0.1, 0.1, 0.11).into()),
            border: iced::Border {
                color: accent.scale_alpha(0.7),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
}

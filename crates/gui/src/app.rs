use crate::{message::Message, state::AppState, update, view, worker};
use iced::{Subscription, Task, widget::pane_grid};
use openhx_core::{Preset, Setlist};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorPane {
    Workspace,
    Inspector,
}

#[derive(Debug, Clone)]
pub struct LoadedIr {
    pub path: String,
    pub sample_rate: u32,
    pub samples: usize,
}

pub struct App {
    pub state: AppState,
    pub device_name: String,
    /// The connected device's setlists, named as on the device.
    pub setlists: Vec<Setlist>,
    /// Setlist whose presets are currently shown.
    pub setlist: u8,
    pub presets: Vec<Preset>,
    pub error_log: Option<String>,
    pub selected_preset: Option<u8>,
    pub editor: crate::state::EditorState,
    pub panes: pane_grid::State<EditorPane>,
    pub block_panes: pane_grid::State<u8>,
    pub loaded_ir: Option<LoadedIr>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            state: AppState::Waiting,
            device_name: String::new(),
            setlists: Vec::new(),
            setlist: 0,
            presets: Vec::new(),
            error_log: None,
            selected_preset: None,
            editor: crate::state::EditorState::default(),
            panes: pane_grid::State::with_configuration(pane_grid::Configuration::Split {
                axis: pane_grid::Axis::Vertical,
                ratio: 0.72,
                a: Box::new(pane_grid::Configuration::Pane(EditorPane::Workspace)),
                b: Box::new(pane_grid::Configuration::Pane(EditorPane::Inspector)),
            }),
            block_panes: pane_grid::State::new(0).0,
            loaded_ir: None,
        }
    }
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        (Self::default(), Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        update::handle_message(self, message)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match self.state {
            AppState::Waiting => Subscription::run(worker::usb_poll),
            AppState::Connected => Subscription::run(worker::usb_check_disconnect),
            _ => Subscription::none(),
        }
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        view::view(self)
    }

    pub fn sync_block_panes(&mut self) {
        let Some(first) = self.editor.blocks.first() else {
            self.block_panes = pane_grid::State::new(0).0;
            return;
        };
        let mut configuration = pane_grid::Configuration::Pane(first.slot);
        for block in self.editor.blocks.iter().skip(1) {
            configuration = pane_grid::Configuration::Split {
                axis: pane_grid::Axis::Vertical,
                ratio: 0.5,
                a: Box::new(configuration),
                b: Box::new(pane_grid::Configuration::Pane(block.slot)),
            };
        }
        self.block_panes = pane_grid::State::with_configuration(configuration);
    }
}

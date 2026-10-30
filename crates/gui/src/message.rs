use openhx_core::{Preset, PresetDetails, Setlist};

#[derive(Debug, Clone)]
pub enum Message {
    DeviceDetected {
        name: String,
        setlists: Vec<Setlist>,
        presets: Vec<Preset>,
        details: PresetDetails,
        selected_preset: Option<u8>,
    },
    DeviceDisconnected,
    EnterOfflineMode,
    ExitOfflineMode,
    ConnectionError(String),
    /// The user picked another setlist in the sidebar.
    SetlistChosen(u8),
    /// A setlist's presets finished loading.
    SetlistLoaded(u8, Vec<Preset>),
    /// Loading a setlist failed; the previous list stays on screen.
    SetlistLoadFailed(u8, String),
    PresetSelected(u8),
    // Reorder the block list in-place within a DSP path. This is the reliable
    // fallback when pane-grid drag gestures are not firing consistently.
    MoveBlock {
        slot: u8,
        direction: i8,
    },
    PresetLoaded {
        index: u8,
        result: Result<(String, PresetDetails), String>,
    },
    PresetNameChanged(String),
    RenamePreset,
    PresetRenameFinished(Result<(), String>),
    SavePreset,
    PresetSaveFinished(Result<(), String>),
    PaneResized(iced::widget::pane_grid::ResizeEvent),
    BlockDragged(iced::widget::pane_grid::DragEvent),
    BlockMoveFinished {
        source: u8,
        destination: u8,
        temporary: u8,
        result: Result<(), String>,
    },
    LoadIr,
    IrLoaded(Result<crate::app::LoadedIr, String>),
    BlockSelected(u8),
    ParameterChanged(u8, f32),
    ParameterEditReleased {
        block: u8,
        parameter: u8,
    },
    ParameterEditFailed(String),
    SnapshotSelected(u8),
}

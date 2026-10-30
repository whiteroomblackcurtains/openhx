use openhx_core::{Preset, PresetDetails, Setlist};

#[derive(Debug, Clone)]
pub enum Message {
    DeviceDetected {
        name: String,
        setlists: Vec<Setlist>,
        presets: Vec<Preset>,
        details: PresetDetails,
    },
    DeviceDisconnected,
    ConnectionError(String),
    /// The user picked another setlist in the sidebar.
    SetlistChosen(u8),
    /// A setlist's presets finished loading.
    SetlistLoaded(u8, Vec<Preset>),
    /// Loading a setlist failed; the previous list stays on screen.
    SetlistLoadFailed(u8, String),
    PresetSelected(u8),
    PresetLoaded(Result<(String, PresetDetails), String>),
    PresetNameChanged(String),
    RenamePreset,
    PresetRenameFinished(Result<(), String>),
    SavePreset,
    PresetSaveFinished(Result<(), String>),
    BlockSelected(u8),
    ParameterChanged(u8, f32),
    ParameterEditReleased {
        block: u8,
        parameter: u8,
    },
    ParameterEditFailed(String),
    SnapshotSelected(u8),
}

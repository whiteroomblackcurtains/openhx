use crate::{app::App, message::Message, state::AppState};
use iced::Task;
use openhx_core::{HxError, reset_shared_device, with_device};
use tracing::{debug, error, info};

pub fn handle_message(app: &mut App, message: Message) -> Task<Message> {
    debug!("Received message: {message:?}");

    match message {
        Message::DeviceDetected {
            name,
            setlists,
            presets,
            details,
        } => {
            info!(
                "Device connected successfully: {name} ({} setlist(s))",
                setlists.len()
            );
            app.device_name = name;
            app.setlists = setlists;
            app.setlist = 0;
            app.presets = presets;
            app.state = AppState::Connected;
            app.error_log = None;
            app.selected_preset = None;
            app.editor = crate::state::EditorState::default();
            if let Err(error) = app.editor.load_details("Current preset", &details) {
                error!("Current preset model decode failed: {error}");
                app.error_log = Some(error);
            }
            Task::none()
        }
        Message::DeviceDisconnected => {
            info!("Device disconnected");
            reset_shared_device();
            app.state = AppState::Waiting;
            app.device_name.clear();
            app.setlists.clear();
            app.setlist = 0;
            app.presets.clear();
            app.selected_preset = None;
            app.editor = crate::state::EditorState::default();
            Task::none()
        }
        Message::SetlistChosen(setlist) => {
            if setlist == app.setlist {
                return Task::none();
            }
            info!("Loading setlist {setlist}");

            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        with_device(|client| client.read_setlist_presets(setlist))
                    })
                    .await
                    .unwrap_or_else(|e| Err(HxError::Protocol(e.to_string())))
                },
                move |result| match result {
                    Ok(presets) => Message::SetlistLoaded(setlist, presets),
                    Err(e) => Message::SetlistLoadFailed(setlist, e.to_string()),
                },
            )
        }
        Message::SetlistLoaded(setlist, presets) => {
            info!("Setlist {setlist} loaded ({} presets)", presets.len());
            app.setlist = setlist;
            app.presets = presets;
            app.selected_preset = None;
            Task::none()
        }
        Message::SetlistLoadFailed(setlist, err) => {
            error!("Failed to load setlist {setlist}: {err}");
            Task::none()
        }
        Message::ConnectionError(err) => {
            error!("Failed to connect to device: {err}");
            app.state = AppState::Error;
            app.error_log = Some(err);
            Task::none()
        }
        Message::PresetSelected(index) => {
            let setlist = app.setlist;
            info!("Preset selected: setlist {setlist}, slot {index:03}");
            app.selected_preset = Some(index);
            let name = app
                .presets
                .iter()
                .find(|preset| preset.index == index)
                .map(|preset| preset.name.clone())
                .unwrap_or_default();

            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        with_device(|client| {
                            client.select_preset(setlist, index)?;
                            client.read_current_preset()
                        })
                    })
                    .await
                    .unwrap_or_else(|e| Err(HxError::Protocol(e.to_string())))
                    .map(|details| (name, details))
                    .map_err(|e| e.to_string())
                },
                Message::PresetLoaded,
            )
        }
        Message::PresetLoaded(result) => {
            match result {
                Ok((name, details)) => {
                    if let Err(error) = app.editor.load_details(name, &details) {
                        error!("Preset model decode failed: {error}");
                        app.error_log = Some(error);
                    } else {
                        app.error_log = None;
                    }
                }
                Err(error) => {
                    error!("Preset load failed: {error}");
                    app.error_log = Some(error);
                }
            }
            Task::none()
        }
        Message::PresetNameChanged(name) => {
            app.editor.preset_name = name;
            Task::none()
        }
        Message::RenamePreset => {
            let Some(preset) = app.selected_preset else {
                return Task::none();
            };
            let setlist = app.setlist;
            let name = app.editor.preset_name.clone();
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        with_device(|client| client.rename_preset(setlist, preset, &name))
                    })
                    .await
                    .unwrap_or_else(|e| Err(HxError::Protocol(e.to_string())))
                },
                |result| Message::PresetRenameFinished(result.map_err(|e| e.to_string())),
            )
        }
        Message::PresetRenameFinished(result) => {
            if let Err(error) = result {
                error!("Preset rename failed: {error}");
                app.error_log = Some(error);
            }
            Task::none()
        }
        Message::SavePreset => {
            let Some(preset) = app.selected_preset else {
                app.error_log = Some("Select a preset before saving".to_string());
                return Task::none();
            };
            let setlist = app.setlist;
            let name = app.editor.preset_name.clone();
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        with_device(|client| client.save_preset(setlist, preset, &name))
                    })
                    .await
                    .unwrap_or_else(|e| Err(HxError::Protocol(e.to_string())))
                },
                |result| Message::PresetSaveFinished(result.map_err(|e| e.to_string())),
            )
        }
        Message::PresetSaveFinished(result) => {
            if let Err(error) = result {
                error!("Preset save failed: {error}");
                app.error_log = Some(error);
            } else {
                app.error_log = None;
            }
            Task::none()
        }
        Message::BlockSelected(block) => {
            app.editor.selected_block = block;
            Task::none()
        }
        Message::ParameterChanged(parameter, value) => {
            if let Some(block) = app.editor.selected_block_mut()
                && let Some(parameter) = block
                    .parameters
                    .iter_mut()
                    .find(|candidate| candidate.index == parameter)
            {
                parameter.value = value;
            }
            Task::none()
        }
        Message::ParameterEditReleased { block, parameter } => {
            let value = app
                .editor
                .blocks
                .iter()
                .find(|candidate| candidate.slot == block)
                .and_then(|candidate| {
                    candidate
                        .parameters
                        .iter()
                        .find(|parameter_value| parameter_value.index == parameter)
                })
                .map(|parameter_value| parameter_value.value);

            let Some(value) = value else {
                return Task::none();
            };

            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        with_device(|client| client.set_parameter(block, parameter, value, 0))
                    })
                    .await
                    .unwrap_or_else(|e| Err(HxError::Protocol(e.to_string())))
                },
                |result| match result {
                    Ok(()) => Message::ParameterEditFailed(String::new()),
                    Err(e) => Message::ParameterEditFailed(e.to_string()),
                },
            )
        }
        Message::ParameterEditFailed(error) => {
            if !error.is_empty() {
                error!("Preset edit failed: {error}");
                app.error_log = Some(error);
            }
            Task::none()
        }
        Message::SnapshotSelected(snapshot) => {
            app.editor.selected_snapshot = snapshot;
            Task::none()
        }
    }
}

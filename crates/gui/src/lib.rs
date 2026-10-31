use dioxus::prelude::*;
use openhx_core::{HxError, Preset, PresetDetails, Setlist, with_device};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

pub mod app;
mod catalog;
mod message;
pub mod state;
mod update;
mod view;
mod worker;

const CSS: &str = include_str!("style.css");

#[derive(Clone)]
enum Command {
    SelectPreset(u8, u8),
    SelectSetlist(u8),
    Rename(u8, u8, String),
    Save(u8, u8, String),
    Parameter(u8, u8, f32),
    SelectBlock(u8, u8),
}

enum Event {
    Connected {
        name: String,
        setlists: Vec<Setlist>,
        presets: Vec<Preset>,
        details: PresetDetails,
        selected: Option<u8>,
    },
    PresetLoaded {
        index: u8,
        details: PresetDetails,
    },
    SetlistLoaded {
        index: u8,
        presets: Vec<Preset>,
    },
    OperationFinished(Result<(), String>),
    IrLoaded(Result<app::LoadedIr, String>),
    Disconnected,
    Error(String),
}

struct UiState {
    state: state::AppState,
    device_name: String,
    setlists: Vec<Setlist>,
    setlist: u8,
    presets: Vec<Preset>,
    selected_preset: Option<u8>,
    editor: state::EditorState,
    loaded_ir: Option<app::LoadedIr>,
    error: Option<String>,
    command_tx: Sender<Command>,
    event_tx: Sender<Event>,
    event_rx: Receiver<Event>,
    dragging: Option<u8>,
    drop_target: Option<u8>,
    pending: bool,
}

impl UiState {
    fn new() -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let worker_events = event_tx.clone();
        thread::Builder::new()
            .name("openhx-device-worker".to_string())
            .spawn(move || device_worker(command_rx, worker_events))
            .expect("device worker must start");
        Self {
            state: state::AppState::Waiting,
            device_name: String::new(),
            setlists: Vec::new(),
            setlist: 0,
            presets: Vec::new(),
            selected_preset: None,
            editor: state::EditorState::default(),
            loaded_ir: None,
            error: None,
            command_tx,
            event_tx,
            event_rx,
            dragging: None,
            drop_target: None,
            pending: false,
        }
    }

    fn drain(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            self.pending = false;
            match event {
                Event::Connected {
                    name,
                    setlists,
                    presets,
                    details,
                    selected,
                } => {
                    self.state = state::AppState::Connected;
                    self.device_name = name;
                    self.setlists = setlists;
                    self.presets = presets;
                    self.selected_preset = selected;
                    self.error = self.editor.load_details("Current preset", &details).err();
                }
                Event::PresetLoaded { index, details } if self.selected_preset == Some(index) => {
                    self.error = self
                        .editor
                        .load_details(
                            self.presets
                                .iter()
                                .find(|preset| preset.index == index)
                                .map(|preset| preset.name.clone())
                                .unwrap_or_else(|| "Current preset".to_string()),
                            &details,
                        )
                        .err();
                }
                Event::SetlistLoaded { index, presets } => {
                    self.setlist = index;
                    self.presets = presets;
                    self.selected_preset = None;
                }
                Event::OperationFinished(result) => self.error = result.err(),
                Event::IrLoaded(result) => match result {
                    Ok(ir) => self.loaded_ir = Some(ir),
                    Err(error) => self.error = Some(error),
                },
                Event::Disconnected => {
                    self.state = state::AppState::Waiting;
                    self.device_name.clear();
                    self.presets.clear();
                    self.selected_preset = None;
                }
                Event::Error(error) => {
                    self.state = state::AppState::Error;
                    self.error = Some(error);
                }
                Event::PresetLoaded { .. } => {}
            }
        }
    }

    fn enter_offline(&mut self) {
        self.state = state::AppState::Offline;
        self.device_name = "Offline".to_string();
        self.editor = state::EditorState::offline_demo();
        self.presets.clear();
        self.selected_preset = None;
        self.error = None;
    }

    fn select_preset(&mut self, index: u8) {
        if self.pending {
            return;
        }
        self.selected_preset = Some(index);
        self.pending = true;
        let _ = self
            .command_tx
            .send(Command::SelectPreset(self.setlist, index));
    }

    fn reorder(&mut self, target: u8) {
        let Some(source) = self.dragging.take() else {
            return;
        };
        self.drop_target = None;
        let Some(source_index) = self
            .editor
            .blocks
            .iter()
            .position(|block| block.slot == source)
        else {
            return;
        };
        let Some(target_index) = self
            .editor
            .blocks
            .iter()
            .position(|block| block.slot == target)
        else {
            return;
        };
        if source_index == target_index {
            return;
        }

        if source / 20 != target / 20 {
            self.error = Some("Blocks can only be reordered within one DSP path.".to_string());
            return;
        }
        if self.state == state::AppState::Connected {
            self.error = Some(
                "Block reordering is temporarily disabled until the HX Stomp move exchange is verified."
                    .to_string(),
            );
            return;
        }
        self.editor.blocks.swap(source_index, target_index);
    }

    fn select_block(&mut self, slot: u8) {
        self.editor.selected_block = slot;
        if self.state == state::AppState::Connected {
            self.pending = true;
            let _ = self.command_tx.send(Command::SelectBlock(slot, 0));
        }
    }

    fn load_ir(&mut self) {
        let event_tx = self.event_tx.clone();
        thread::spawn(move || {
            let result = (|| {
                let path = rfd::FileDialog::new()
                    .add_filter("Impulse response", &["wav"])
                    .pick_file()
                    .ok_or_else(|| "IR loading cancelled".to_string())?;
                let mut reader = hound::WavReader::open(&path)
                    .map_err(|error| format!("failed to open IR WAV: {error}"))?;
                let spec = reader.spec();
                if spec.sample_rate != 48_000 || !(spec.channels == 1 || spec.channels == 2) {
                    return Err("IR must be mono or stereo at 48 kHz.".to_string());
                }
                let samples = reader.samples::<i32>().count();
                Ok(app::LoadedIr {
                    path: path.display().to_string(),
                    sample_rate: spec.sample_rate,
                    channels: spec.channels,
                    samples,
                })
            })();
            let _ = event_tx.send(Event::IrLoaded(result));
        });
    }
}

#[component]
fn App() -> Element {
    let mut ui = use_signal(UiState::new);
    use_future(move || async move {
        loop {
            ui.write().drain();
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    });

    rsx! {
        style { {CSS} }
        div { class: "app",
            TopBar { ui }
            if ui.read().state == state::AppState::Waiting {
                Waiting { ui }
            } else {
                div { class: "workspace",
                    PresetPanel { ui }
                    ChainPanel { ui }
                    Inspector { ui }
                }
            }
            Footer { ui }
        }
    }
}

#[component]
fn TopBar(ui: Signal<UiState>) -> Element {
    let snapshot = ui.read();
    let offline = snapshot.state == state::AppState::Offline;
    let connected = snapshot.state == state::AppState::Connected;
    let name = snapshot.editor.preset_name.clone();
    let device = snapshot.device_name.clone();
    drop(snapshot);
    rsx! {
        header { class: "topbar",
            div { class: "brand", "OPENHX" }
            div { class: if offline { "status offline" } else { "status" },
                if connected { "● {device}" } else if offline { "● Offline editor" } else { "○ Waiting for HX device" }
            }
            div { class: "spacer" }
            button { onclick: move |_| ui.write().enter_offline(), "Offline editor" }
            if !offline {
                button { class: "primary", disabled: !connected, onclick: move |_| {
                    let mut state = ui.write();
                    if let Some(index) = state.selected_preset {
                        state.pending = true;
                        let _ = state.command_tx.send(Command::Save(state.setlist, index, state.editor.preset_name.clone()));
                    }
                }, "Save" }
            }
        }
        div { class: "toolbar",
            span { "Preset" }
            input {
                value: "{name}",
                placeholder: "Preset name",
                oninput: move |event| ui.write().editor.preset_name = event.value(),
                onkeydown: move |event| if event.key() == Key::Enter {
                    let mut state = ui.write();
                    if state.state == state::AppState::Connected
                        && let Some(index) = state.selected_preset
                    {
                        state.pending = true;
                        let _ = state.command_tx.send(Command::Rename(state.setlist, index, state.editor.preset_name.clone()));
                    }
                }
            }
            button { onclick: move |_| {
                let mut state = ui.write();
                if state.state == state::AppState::Connected
                    && let Some(index) = state.selected_preset
                {
                    state.pending = true;
                    let _ = state.command_tx.send(Command::Rename(state.setlist, index, state.editor.preset_name.clone()));
                }
            }, "Rename" }
            if ui.read().pending { span { "Working…" } }
        }
    }
}

#[component]
fn Waiting(ui: Signal<UiState>) -> Element {
    rsx! {
        main { class: "empty",
            div {
                h1 { "OpenHX" }
                p { "Waiting for a supported HX device…" }
                button { onclick: move |_| ui.write().enter_offline(), "Open offline editor" }
                if let Some(error) = ui.read().error.clone() {
                    div { class: "error", "{error}" }
                }
            }
        }
    }
}

#[component]
fn PresetPanel(ui: Signal<UiState>) -> Element {
    let state = ui.read();
    let setlists = state.setlists.clone();
    let presets = state.presets.clone();
    let selected = state.selected_preset;
    let current_setlist = state.setlist;
    drop(state);
    rsx! {
        aside { class: "panel",
            div { class: "panel-title", "PRESETS" }
            if setlists.len() > 1 {
                select { class: "setlist", value: "{current_setlist}", onchange: move |event| {
                    if let Ok(index) = event.value().parse::<u8>() {
                        let mut state = ui.write();
                        state.pending = true;
                        let _ = state.command_tx.send(Command::SelectSetlist(index));
                    }
                },
                    for setlist in setlists {
                        option { value: "{setlist.index}", "{setlist.name}" }
                    }
                }
            }
            for preset in presets {
                button {
                    class: if selected == Some(preset.index) { "preset selected" } else { "preset" },
                    id: "preset-{preset.index}",
                    tabindex: "-1",
                    autofocus: selected == Some(preset.index),
                    onmounted: move |event| {
                        if selected == Some(preset.index) {
                            spawn(async move {
                                let _ = event
                                    .data()
                                    .scroll_to(dioxus::html::ScrollBehavior::Instant)
                                    .await;
                            });
                        }
                    },
                    onclick: move |_| ui.write().select_preset(preset.index),
                    span { class: "preset-index", "{preset.index:03}" }
                    span { "{preset.name}" }
                }
            }
        }
    }
}

#[component]
fn ChainPanel(ui: Signal<UiState>) -> Element {
    let state = ui.read();
    let blocks = state.editor.blocks.clone();
    let selected = state.editor.selected_block;
    let dragging = state.dragging;
    drop(state);
    let selected_block = blocks.iter().find(|block| block.slot == selected).cloned();
    let dsp0: Vec<_> = blocks
        .iter()
        .filter(|block| block.slot < 20)
        .cloned()
        .collect();
    let dsp1: Vec<_> = blocks
        .iter()
        .filter(|block| block.slot >= 20)
        .cloned()
        .collect();
    let split = dsp0
        .iter()
        .find(|block| block.model_id.contains("DSPFlowSplit"))
        .map(|block| block.slot);
    let merge = dsp0
        .iter()
        .find(|block| block.model_id.contains("DSPFlowJoin"))
        .map(|block| block.slot);
    rsx! {
        section { class: "panel chain-panel",
            div { class: "panel-title", "SIGNAL CHAIN" }
            div { class: "chain-scroll",
                if let (Some(split), Some(merge)) = (split, merge) {
                    {
                        let branch_boundary = (split + merge) / 2 + 1;
                        rsx! {
                            PathRow { ui, label: "MAIN", blocks: dsp0.iter().filter(|block| block.slot <= split).cloned().collect(), selected, dragging }
                            PathRow { ui, label: "PATH A", blocks: dsp0.iter().filter(|block| block.slot > split && block.slot < branch_boundary).cloned().collect(), selected, dragging }
                            PathRow { ui, label: "PATH B", blocks: dsp0.iter().filter(|block| block.slot >= branch_boundary && block.slot < merge).cloned().collect(), selected, dragging }
                            PathRow { ui, label: "MERGE", blocks: dsp0.iter().filter(|block| block.slot >= merge).cloned().collect(), selected, dragging }
                        }
                    }
                } else {
                    PathRow { ui, label: "PATH 1", blocks: dsp0, selected, dragging }
                }
                PathRow {
                    ui,
                    label: "PATH 2",
                    blocks: dsp1,
                    selected,
                    dragging,
                }
            }
            if let Some(block) = selected_block {
                ModelPreview { ui, block }
            }
        }
    }
}

#[component]
fn PathRow(
    ui: Signal<UiState>,
    label: &'static str,
    blocks: Vec<state::EditorBlock>,
    selected: u8,
    dragging: Option<u8>,
) -> Element {
    rsx! {
        div { class: "path-row",
            div { class: "path-label", "{label}" }
            div { class: "chain",
                for (position, block) in blocks.iter().enumerate() {
                    if position > 0 { div { class: "arrow", "→" } }
                    BlockTile {
                        ui,
                        block: block.clone(),
                        selected: selected == block.slot,
                        dragging,
                    }
                }
            }
        }
    }
}

#[component]
fn BlockTile(
    ui: Signal<UiState>,
    block: state::EditorBlock,
    selected: bool,
    dragging: Option<u8>,
) -> Element {
    let slot = block.slot;
    let colour = block_colour(&block.model_id);
    let is_drop_target = ui.read().drop_target == Some(slot);
    rsx! {
        div {
            class: if selected && is_drop_target { "block selected drop-target" } else if selected { "block selected" } else if is_drop_target { "block drop-target" } else { "block" },
            style: "--block-color: {colour};",
            onmousedown: move |event| {
                event.prevent_default();
                let mut state = ui.write();
                state.select_block(slot);
                state.dragging = Some(slot);
            },
            onmouseup: move |event| {
                event.prevent_default();
                ui.write().reorder(slot);
            },
            onmouseenter: move |_| {
                if dragging.is_some() {
                    ui.write().drop_target = Some(slot);
                }
            },
            onmouseleave: move |_| if dragging == Some(slot) { ui.write().drop_target = None },
            ondragend: move |_| {
                let mut state = ui.write();
                state.dragging = None;
                state.drop_target = None;
            },
            onclick: move |_| ui.write().select_block(slot),
            div { class: "block-head",
                span { class: "drag-grip", "⠿" }
                span { "{slot}" }
            }
            img { class: if state::is_flow_model(&block.model_id) { "block-icon flow-icon" } else { "block-icon" }, src: "{category_icon(&block.model_id)}", alt: "block icon" }
            div { class: "block-name", "{block.name}" }
        }
    }
}

#[component]
fn ModelPreview(ui: Signal<UiState>, block: state::EditorBlock) -> Element {
    let colour = block_colour(&block.model_id);
    let preview_parameters: Vec<_> = block.parameters.iter().take(6).cloned().collect();
    rsx! {
        div { class: "preview",
            div { class: "preview-content",
                img { class: "model-art", src: "{category_icon(&block.model_id)}", alt: "model artwork" }
                div {
                    div { class: "preview-title", "{block.name}" }
                    div { class: "preview-model", "{block.model}" }
                    div { class: "preview-model", "Model preview" }
                }
                div { class: "knobs",
                    for parameter in preview_parameters {
                        div { class: "knob-wrap",
                            div { class: "knob", style: "--knob-color: {colour};",                             "{parameter.value:.1} {parameter.unit}" }
                            input {
                                class: "knob-input",
                                r#type: "range",
                                min: "{parameter.min}",
                                max: "{parameter.max}",
                                step: "{parameter.step}",
                                value: "{parameter.value}",
                                onchange: {
                                    let block_slot = block.slot;
                                    let parameter_index = parameter.index;
                                    move |event| if let Ok(next) = event.value().parse::<f32>() {
                                        let mut state = ui.write();
                                        if let Some(block) = state.editor.blocks.iter_mut().find(|block| block.slot == block_slot)
                                            && let Some(parameter) = block.parameters.iter_mut().find(|parameter| parameter.index == parameter_index)
                                        {
                                            parameter.value = next;
                                        }
                                        if state.state == state::AppState::Connected {
                                            state.pending = true;
                                            let _ = state.command_tx.send(Command::Parameter(
                                                block_slot,
                                                parameter_index,
                                                next / parameter.device_scale,
                                            ));
                                        }
                                    }
                                }
                            }
                            div { "{parameter.name}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Inspector(ui: Signal<UiState>) -> Element {
    let state = ui.read();
    let selected = state.editor.selected_block;
    let block = state
        .editor
        .blocks
        .iter()
        .find(|block| block.slot == selected)
        .cloned();
    let error = state.error.clone();
    let ir = state.loaded_ir.clone();
    drop(state);
    rsx! {
        aside { class: "panel",
            div { class: "panel-title", "PARAMETERS" }
            if let Some(block) = block {
                div { class: "inspector-header", style: "--block-color: {block_colour(&block.model_id)};",
                    div { class: "inspector-name", "{block.name}" }
                    div { class: "inspector-model", "{block.model}" }
                }
                for parameter in block.parameters {
                    Parameter { ui, block_slot: block.slot, parameter }
                }
            } else {
                div { class: "empty", "Select a block to edit." }
            }
            button { onclick: move |_| ui.write().load_ir(), "Load IR WAV" }
            if let Some(ir) = ir {
                div { class: "preview-model", "{ir.path} · {ir.sample_rate} Hz · {ir.channels} ch · {ir.samples} samples" }
            }
            if let Some(error) = error {
                div { class: "error", "{error}" }
            }
        }
    }
}

#[component]
fn Parameter(ui: Signal<UiState>, block_slot: u8, parameter: state::EditorParameter) -> Element {
    let colour = block_colour(
        &ui.read()
            .editor
            .blocks
            .iter()
            .find(|block| block.slot == block_slot)
            .map(|block| block.model_id.clone())
            .unwrap_or_default(),
    );
    let parameter_index = parameter.index;
    let value = parameter.value;
    let min = parameter.min;
    let max = parameter.max;
    let step = parameter.step;
    let unit = parameter.unit.clone();
    let device_scale = parameter.device_scale;
    let progress = if max > min {
        ((value - min) / (max - min) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    rsx! {
        div { class: "parameter", style: "--param-color: {colour};",
            div { class: "parameter-row",
                span { "{parameter.name}" }
                span { "{value:.2} {unit}" }
            }
            input {
                r#type: "range",
                min: "{min}",
                max: "{max}",
                step: "{step}",
                value: "{value}",
                oninput: move |event| if let Ok(next) = event.value().parse::<f32>() {
                    let mut state = ui.write();
                    if let Some(block) = state.editor.blocks.iter_mut().find(|block| block.slot == block_slot)
                        && let Some(parameter) = block.parameters.iter_mut().find(|parameter| parameter.index == parameter_index)
                    {
                        parameter.value = next;
                    }
                },
                onchange: move |event| if let Ok(next) = event.value().parse::<f32>() {
                    let mut state = ui.write();
                    if state.state == state::AppState::Connected {
                        state.pending = true;
                        let _ = state.command_tx.send(Command::Parameter(
                            block_slot,
                            parameter_index,
                            next / device_scale,
                        ));
                    }
                }
            }
            div { class: "meter", div { style: "width: {progress}%;"} }
        }
    }
}

#[component]
fn Footer(ui: Signal<UiState>) -> Element {
    let selected = ui.read().editor.selected_snapshot;
    rsx! {
        footer { class: "footer",
            span { "SNAPSHOTS" }
            div { class: "snapshots",
                for snapshot in 0..4u8 {
                    button {
                        class: if selected == snapshot { "snapshot selected" } else { "snapshot" },
                        onclick: move |_| ui.write().editor.selected_snapshot = snapshot,
                        "{snapshot + 1}"
                    }
                }
            }
        }
    }
}

fn category_key(model: &str) -> &'static str {
    let lower = model.to_ascii_lowercase();
    if lower.contains("split") {
        "split"
    } else if lower.contains("join") || lower.contains("merge") {
        "merge"
    } else if lower.contains("preamp") {
        "preamp"
    } else if lower.contains("amp") {
        "amp"
    } else if lower.contains("cab") || lower.contains("ir") {
        "cab"
    } else if lower.contains("delay") {
        "delay"
    } else if lower.contains("dist") || lower.contains("fuzz") || lower.contains("drive") {
        "distortion"
    } else if lower.contains("compress") || lower.contains("gate") {
        "dynamics"
    } else if lower.contains("eq") {
        "eq"
    } else if lower.contains("filter") {
        "filter"
    } else if lower.contains("mod")
        || lower.contains("chorus")
        || lower.contains("flanger")
        || lower.contains("tremolo")
        || lower.contains("vibrato")
    {
        "modulation"
    } else if lower.contains("pitch") || lower.contains("synth") {
        "pitch"
    } else if lower.contains("reverb") {
        "reverb"
    } else if lower.contains("wah") {
        "wah"
    } else if lower.contains("volume") || lower.contains("pan") {
        "volume"
    } else {
        "none"
    }
}

fn category_icon(model: &str) -> String {
    let filename = match category_key(model) {
        "split" => "FX_HX_Category_Split.png",
        "merge" => "FX_HX_Category_Merge.png",
        "amp" => "FX_HX_Category_Amp.png",
        "preamp" => "FX_HX_Category_Preamp.png",
        "cab" => "FX_HX_Category_Cab.png",
        "delay" => "FX_HX_Category_Delay.png",
        "distortion" => "FX_HX_Category_Distortion.png",
        "dynamics" => "FX_HX_Category_Dynamics.png",
        "eq" => "FX_HX_Category_EQ.png",
        "filter" => "FX_HX_Category_Filter.png",
        "modulation" => "FX_HX_Category_Modulation.png",
        "pitch" => "FX_HX_Category_PitchSynth.png",
        "reverb" => "FX_HX_Category_Reverb.png",
        "wah" => "FX_HX_Category_Wah.png",
        "volume" => "FX_HX_Category_VolumePan.png",
        _ => "FX_HX_Category_None.png",
    };
    let bytes: &[u8] = match filename {
        "FX_HX_Category_Split.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Split.png")
        }
        "FX_HX_Category_Merge.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Merge.png")
        }
        "FX_HX_Category_Amp.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Amp.png")
        }
        "FX_HX_Category_Preamp.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Preamp.png")
        }
        "FX_HX_Category_Cab.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Cab.png")
        }
        "FX_HX_Category_Delay.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Delay.png")
        }
        "FX_HX_Category_Distortion.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Distortion.png")
        }
        "FX_HX_Category_Dynamics.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Dynamics.png")
        }
        "FX_HX_Category_EQ.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_EQ.png")
        }
        "FX_HX_Category_Filter.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Filter.png")
        }
        "FX_HX_Category_Modulation.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Modulation.png")
        }
        "FX_HX_Category_PitchSynth.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_PitchSynth.png")
        }
        "FX_HX_Category_Reverb.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Reverb.png")
        }
        "FX_HX_Category_Wah.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_Wah.png")
        }
        "FX_HX_Category_VolumePan.png" => {
            include_bytes!("../../../res/icons_category/FX_HX_Category_VolumePan.png")
        }
        _ => include_bytes!("../../../res/icons_category/FX_HX_Category_None.png"),
    };
    format!("data:image/png;base64,{}", base64(bytes))
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (a << 16) | (b << 8) | c;
        output.push(TABLE[((triple >> 18) & 63) as usize] as char);
        output.push(TABLE[((triple >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((triple >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(triple & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn block_colour(model: &str) -> String {
    match category_key(model) {
        "amp" | "preamp" => "#e68a54",
        "cab" => "#7896c5",
        "delay" | "reverb" => "#5bb0df",
        "distortion" => "#df6172",
        "dynamics" => "#d4a84e",
        "eq" => "#55c99b",
        "filter" | "wah" => "#c18ee8",
        "modulation" => "#a978df",
        "pitch" => "#65d0d9",
        "volume" => "#88a6bf",
        _ => "#778294",
    }
    .to_string()
}

fn device_worker(command_rx: Receiver<Command>, event_tx: Sender<Event>) {
    let mut connected = false;
    loop {
        if !connected {
            match with_device(|client| {
                let name = client.profile().name.to_string();
                let setlists = client.list_setlists()?;
                let presets = client.read_presets()?;
                let details = client.read_current_preset()?;
                let selected = client.read_current_preset_index().unwrap_or(None);
                Ok((name, setlists, presets, details, selected))
            }) {
                Ok((name, setlists, presets, details, selected)) => {
                    connected = true;
                    let _ = event_tx.send(Event::Connected {
                        name,
                        setlists,
                        presets,
                        details,
                        selected,
                    });
                }
                Err(HxError::DeviceNotFound) => {}
                Err(error) => {
                    let _ = event_tx.send(Event::Error(error.to_string()));
                }
            }
        }
        match command_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(command) => {
                let result = match command {
                    Command::SelectPreset(setlist, index) => with_device(|client| {
                        client.select_preset(setlist, index)?;
                        let details = client.read_current_preset()?;
                        Ok((index, details))
                    })
                    .map_err(|error| error.to_string())
                    .and_then(|(index, details)| {
                        event_tx
                            .send(Event::PresetLoaded { index, details })
                            .map_err(|error| error.to_string())
                    }),
                    Command::SelectSetlist(index) => {
                        with_device(|client| client.read_setlist_presets(index))
                            .map_err(|error| error.to_string())
                            .and_then(|presets| {
                                event_tx
                                    .send(Event::SetlistLoaded { index, presets })
                                    .map_err(|error| error.to_string())
                            })
                    }
                    Command::Rename(setlist, preset, name) => {
                        with_device(|client| client.rename_preset(setlist, preset, &name))
                            .map_err(|error| error.to_string())
                            .and_then(|_| {
                                event_tx
                                    .send(Event::OperationFinished(Ok(())))
                                    .map_err(|error| error.to_string())
                            })
                    }
                    Command::Save(setlist, preset, name) => {
                        with_device(|client| client.save_preset(setlist, preset, &name))
                            .map_err(|error| error.to_string())
                            .and_then(|_| {
                                event_tx
                                    .send(Event::OperationFinished(Ok(())))
                                    .map_err(|error| error.to_string())
                            })
                    }
                    Command::Parameter(block, parameter, value) => {
                        with_device(|client| client.set_parameter(block, parameter, value, 0))
                            .map_err(|error| error.to_string())
                            .and_then(|_| {
                                event_tx
                                    .send(Event::OperationFinished(Ok(())))
                                    .map_err(|error| error.to_string())
                            })
                    }
                    Command::SelectBlock(block, model) => {
                        with_device(|client| client.select_block(block, model))
                            .map_err(|error| error.to_string())
                            .and_then(|_| {
                                event_tx
                                    .send(Event::OperationFinished(Ok(())))
                                    .map_err(|error| error.to_string())
                            })
                    }
                };
                if let Err(error) = result {
                    let _ = event_tx.send(Event::Error(error));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if connected && !openhx_core::is_device_present() {
                    connected = false;
                    openhx_core::reset_shared_device();
                    let _ = event_tx.send(Event::Disconnected);
                }
            }
        }
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let log_dir = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
        .join("OpenHX");
    std::fs::create_dir_all(&log_dir)?;
    let log_path = log_dir.join("openhx.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let (writer, log_guard) = tracing_appender::non_blocking(log_file);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("openhx_core=debug,openhx_gui=debug")
            }),
        )
        .with_target(false)
        .with_ansi(false)
        .with_writer(writer)
        .init();
    tracing::info!(path = %log_path.display(), "OpenHX diagnostic logging started");
    let _log_guard = log_guard;
    dioxus::launch(App);
    Ok(())
}

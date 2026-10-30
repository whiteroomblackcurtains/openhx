#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Waiting,
    Connected,
    Offline,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditorParameter {
    pub index: u8,
    pub symbolic_id: String,
    pub name: String,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub unit: String,
    pub device_scale: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditorBlock {
    pub slot: u8,
    pub model_id: String,
    pub name: String,
    pub model: String,
    pub parameters: Vec<EditorParameter>,
}

#[derive(Debug, Clone, Default)]
pub struct EditorState {
    pub preset_name: String,
    pub selected_block: u8,
    pub selected_snapshot: u8,
    pub blocks: Vec<EditorBlock>,
}

impl EditorState {
    pub fn offline_demo() -> Self {
        let mut editor = Self {
            preset_name: "Offline preset".to_string(),
            ..Self::default()
        };
        for (slot, model_id) in [
            (0, "HD2_GateNoiseGate"),
            (1, "HD2_AmpBrit2204"),
            (2, "HD2_DelaySimpleDelay"),
        ] {
            if let Some(definition) = crate::catalog::model(model_id) {
                let names: Vec<String> = definition
                    .params
                    .iter()
                    .map(|parameter| parameter.symbolic_id.clone())
                    .collect();
                if let Ok(block) = editor_block_with_values(slot, model_id, &names, &[]) {
                    editor.blocks.push(block);
                }
            }
        }
        editor.selected_block = editor.blocks.first().map_or(0, |block| block.slot);
        editor
    }

    pub fn load_preset(&mut self, name: impl Into<String>) {
        self.preset_name = name.into();
        self.selected_block = 0;
        self.selected_snapshot = 0;
    }

    pub fn load_details(
        &mut self,
        name: impl Into<String>,
        details: &openhx_core::PresetDetails,
    ) -> Result<(), String> {
        let mut blocks = Vec::with_capacity(details.blocks.len());
        for block in &details.blocks {
            let model_index = block
                .model_index
                .ok_or_else(|| format!("block {} has no model identity", block.slot))?;
            let model_id = crate::catalog::symbol_at(model_index)
                .ok_or_else(|| format!("unknown model symbol index {model_index}"))?;
            blocks.push(editor_block_with_values(
                block.slot,
                model_id,
                crate::catalog::parameter_names(model_index)
                    .ok_or_else(|| format!("no parameter order for model {model_id}"))?,
                &block.parameters,
            )?);
        }
        self.preset_name = name.into();
        self.selected_block = blocks.first().map_or(0, |block| block.slot);
        self.selected_snapshot = 0;
        self.blocks = blocks;
        Ok(())
    }

    pub fn selected_block(&self) -> Option<&EditorBlock> {
        self.blocks
            .iter()
            .find(|block| block.slot == self.selected_block)
    }

    pub fn selected_block_mut(&mut self) -> Option<&mut EditorBlock> {
        self.blocks
            .iter_mut()
            .find(|block| block.slot == self.selected_block)
    }
}

fn editor_block_with_values(
    slot: u8,
    model_id: &str,
    parameter_names: &[String],
    values: &[f32],
) -> Result<EditorBlock, String> {
    let definition = crate::catalog::model(model_id);
    let Some(definition) = definition else {
        if is_flow_model(model_id) {
            return Ok(EditorBlock {
                slot,
                model_id: model_id.to_string(),
                name: flow_model_name(model_id).to_string(),
                model: model_id.to_string(),
                parameters: parameter_names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| EditorParameter {
                        index: index as u8,
                        symbolic_id: name.clone(),
                        name: name.clone(),
                        value: values.get(index).copied().unwrap_or(0.5),
                        min: 0.0,
                        max: 1.0,
                        step: 0.01,
                        unit: String::new(),
                        device_scale: 1.0,
                    })
                    .collect(),
            });
        }
        return Err(format!("model resource not found for {model_id}"));
    };
    let name = definition
        .name
        .clone()
        .unwrap_or_else(|| model_id.to_string());
    let parameters = parameter_names
        .iter()
        .enumerate()
        .map(|(index, parameter_name)| {
            let parameter = definition.params.iter().find(|parameter| {
                parameter.symbolic_id == *parameter_name
                    || parameter.name.as_deref() == Some(parameter_name)
            });
            let (symbolic_id, name, min, max, fallback, step, unit, device_scale) = match parameter
            {
                Some(parameter) => (
                    parameter.symbolic_id.clone(),
                    parameter.display_name(),
                    parameter.min_value().unwrap_or(0.0) * display_scale(model_id, parameter),
                    parameter.max_value().unwrap_or(1.0) * display_scale(model_id, parameter),
                    parameter.default_value().unwrap_or_else(|| {
                        match (parameter.min_value(), parameter.max_value()) {
                            (Some(min), Some(max)) => (min + max) / 2.0,
                            _ => 0.5,
                        }
                    }) * display_scale(model_id, parameter),
                    display_step(model_id, parameter),
                    display_unit(parameter),
                    display_scale(model_id, parameter),
                ),
                None => (
                    parameter_name.clone(),
                    parameter_name.clone(),
                    0.0,
                    1.0,
                    0.5,
                    0.01,
                    String::new(),
                    1.0,
                ),
            };
            Ok(EditorParameter {
                index: index as u8,
                symbolic_id,
                name,
                value: values
                    .get(index)
                    .copied()
                    .map(|value| value * device_scale)
                    .unwrap_or(fallback),
                min,
                max,
                step,
                unit,
                device_scale,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(EditorBlock {
        slot,
        model_id: model_id.to_string(),
        name,
        model: model_id.to_string(),
        parameters,
    })
}

pub fn is_flow_model(model_id: &str) -> bool {
    model_id.starts_with("HD2_AppDSPFlow")
}

fn flow_model_name(model_id: &str) -> &'static str {
    match model_id {
        "HD2_AppDSPFlowSplitY" => "Split Y",
        "HD2_AppDSPFlowSplitAB" => "Split A/B",
        "HD2_AppDSPFlowSplitXOver" => "Split Crossover",
        "HD2_AppDSPFlowSplitDyn" => "Split Dynamic",
        "HD2_AppDSPFlowJoin" => "Merge",
        "HD2_AppDSPFlow1Input" | "HD2_AppDSPFlow2Input" => "Input",
        "HD2_AppDSPFlowOutput" => "Output",
        _ => "Signal flow",
    }
}

fn display_scale(model_id: &str, parameter: &crate::catalog::ParameterDefinition) -> f32 {
    match parameter.display_type.as_deref() {
        Some("time_ms_reverb") => 1000.0,
        Some("percent") => 100.0,
        Some("generic_knob") if model_id.contains("Reverb") && parameter.symbolic_id == "Decay" => {
            10.0
        }
        _ => 1.0,
    }
}

fn display_step(model_id: &str, parameter: &crate::catalog::ParameterDefinition) -> f32 {
    match parameter.display_type.as_deref() {
        Some("time_ms_reverb") | Some("mod_low_cut") | Some("mod_high_cut") => 1.0,
        Some("percent") => 1.0,
        Some("generic_knob") if model_id.contains("Reverb") && parameter.symbolic_id == "Decay" => {
            0.1
        }
        _ => 0.01,
    }
}

fn display_unit(parameter: &crate::catalog::ParameterDefinition) -> String {
    match parameter.display_type.as_deref() {
        Some("time_ms_reverb") => "ms".to_string(),
        Some("percent") => "%".to_string(),
        Some("mod_low_cut") | Some("mod_high_cut") => "Hz".to_string(),
        Some("volume") => "dB".to_string(),
        _ => String::new(),
    }
}

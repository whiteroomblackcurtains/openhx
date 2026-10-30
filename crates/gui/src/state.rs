#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Waiting,
    Connected,
    Error,
}

#[derive(Debug, Clone)]
pub struct EditorParameter {
    pub index: u8,
    pub symbolic_id: String,
    pub name: String,
    pub value: f32,
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone)]
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
            let (symbolic_id, name, min, max, fallback) = match parameter {
                Some(parameter) => (
                    parameter.symbolic_id.clone(),
                    parameter.display_name(),
                    parameter.min_value().unwrap_or(0.0),
                    parameter.max_value().unwrap_or(1.0),
                    parameter.default_value().unwrap_or_else(|| {
                        match (parameter.min_value(), parameter.max_value()) {
                            (Some(min), Some(max)) => (min + max) / 2.0,
                            _ => 0.5,
                        }
                    }),
                ),
                None => (
                    parameter_name.clone(),
                    parameter_name.clone(),
                    0.0,
                    1.0,
                    0.5,
                ),
            };
            Ok(EditorParameter {
                index: index as u8,
                symbolic_id,
                name,
                value: values.get(index).copied().unwrap_or(fallback),
                min,
                max,
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

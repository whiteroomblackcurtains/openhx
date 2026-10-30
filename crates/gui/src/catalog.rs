use serde::Deserialize;
use std::{collections::HashMap, sync::OnceLock};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDefinition {
    #[serde(rename = "symbolicID")]
    pub symbolic_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub params: Vec<ParameterDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterDefinition {
    #[serde(rename = "symbolicID")]
    pub symbolic_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub min: Option<serde_json::Value>,
    #[serde(default)]
    pub max: Option<serde_json::Value>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

impl ParameterDefinition {
    pub fn min_value(&self) -> Option<f32> {
        numeric_value(self.min.as_ref())
    }

    pub fn max_value(&self) -> Option<f32> {
        numeric_value(self.max.as_ref())
    }

    pub fn default_value(&self) -> Option<f32> {
        numeric_value(self.default.as_ref())
    }

    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| self.symbolic_id.clone())
    }
}

fn numeric_value(value: Option<&serde_json::Value>) -> Option<f32> {
    match value? {
        serde_json::Value::Number(number) => number.as_f64().map(|value| value as f32),
        serde_json::Value::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
        _ => None,
    }
}

static MODELS: OnceLock<HashMap<String, ModelDefinition>> = OnceLock::new();
#[derive(Debug, Deserialize)]
struct SymbolDefinition {
    symbol: String,
    parameters: Vec<String>,
}

pub fn model(symbolic_id: &str) -> Option<&'static ModelDefinition> {
    models()
        .get(symbolic_id)
        .or_else(|| {
            symbolic_id
                .strip_suffix("Mono")
                .and_then(|id| models().get(id))
        })
        .or_else(|| {
            symbolic_id
                .strip_suffix("Stereo")
                .and_then(|id| models().get(id))
        })
}

pub fn symbol_at(index: u16) -> Option<&'static str> {
    symbol_definitions()
        .get(usize::from(index))
        .map(|symbol| symbol.symbol.as_str())
}

pub fn parameter_names(index: u16) -> Option<&'static [String]> {
    symbol_definitions()
        .get(usize::from(index))
        .map(|symbol| symbol.parameters.as_slice())
}

fn symbol_definitions() -> &'static Vec<SymbolDefinition> {
    static DEFINITIONS: OnceLock<Vec<SymbolDefinition>> = OnceLock::new();
    DEFINITIONS.get_or_init(|| {
        serde_json::from_str(include_str!("../../../res/Helix.sym"))
            .expect("bundled Helix symbols must contain a valid symbol array")
    })
}

fn models() -> &'static HashMap<String, ModelDefinition> {
    MODELS.get_or_init(|| {
        let mut models = HashMap::new();
        for source in [
            include_str!("../../../res/amp.models"),
            include_str!("../../../res/preamp.models"),
            include_str!("../../../res/cab.models"),
            include_str!("../../../res/cabmicirs.models"),
            include_str!("../../../res/cabmicirswithpan.models"),
            include_str!("../../../res/compressor.models"),
            include_str!("../../../res/delay.models"),
            include_str!("../../../res/distortion.models"),
            include_str!("../../../res/eq.models"),
            include_str!("../../../res/filter.models"),
            include_str!("../../../res/fixed.models"),
            include_str!("../../../res/gate.models"),
            include_str!("../../../res/io.models"),
            include_str!("../../../res/modulation.models"),
            include_str!("../../../res/pitch-synth.models"),
            include_str!("../../../res/reverb.models"),
            include_str!("../../../res/sendreturn.models"),
            include_str!("../../../res/volumepan.models"),
            include_str!("../../../res/wah.models"),
        ] {
            let definitions: Vec<ModelDefinition> = serde_json::from_str(source)
                .expect("bundled HX Edit model resource must contain a valid model array");
            models.extend(
                definitions
                    .into_iter()
                    .map(|definition| (definition.symbolic_id.clone(), definition)),
            );
        }

        models
    })
}

#[cfg(test)]
mod tests {
    use super::model;

    #[test]
    fn loads_model_specific_parameters() {
        let amp = model("HD2_AmpBrit2204").expect("Brit 2204 must be bundled");
        let delay = model("HD2_DelaySimpleDelay").expect("Simple Delay must be bundled");

        assert!(amp.params.len() > 3);
        assert!(delay.params.len() > 3);
        assert_ne!(amp.params.len(), delay.params.len());
        assert_eq!(amp.params[0].display_name(), "Drive");
        assert!(amp.params[0].min_value().is_some());
        assert!(amp.params[0].max_value().is_some());
    }

    #[test]
    fn handles_boolean_resource_values() {
        let gate = model("HD2_GateNoiseGate").expect("Noise Gate must be bundled");
        let enabled = gate
            .params
            .iter()
            .find(|parameter| parameter.symbolic_id == "@enabled")
            .expect("Noise Gate enabled parameter must be present");

        assert_eq!(enabled.default_value(), Some(1.0));
    }
}

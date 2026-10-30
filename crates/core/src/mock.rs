use crate::{
    client::DeviceClient,
    device::{PROFILE_HX_STOMP_XL, profile::DeviceProfile},
    error::HxError,
    models::{Preset, PresetDetails, Setlist},
};

/// A simulated HX Stomp XL.
pub struct MockClient;

impl MockClient {
    /// Creates a new `MockClient` targeting the HX Stomp XL profile.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceClient for MockClient {
    fn profile(&self) -> &'static DeviceProfile {
        &PROFILE_HX_STOMP_XL
    }

    /// Returns a static list of 128 realistic preset names without any I/O.
    fn read_presets(&self) -> Result<Vec<Preset>, HxError> {
        let presets = MOCK_PRESET_NAMES
            .iter()
            .enumerate()
            .map(|(i, &name)| Preset::new(i as u8, name))
            .collect();

        Ok(presets)
    }

    fn list_setlists(&self) -> Result<Vec<Setlist>, HxError> {
        Ok((0..self.profile().setlist_count)
            .map(|i| Setlist::new(i, format!("MOCK {}", i + 1)))
            .collect())
    }

    fn read_setlist_presets(&self, setlist: u8) -> Result<Vec<Preset>, HxError> {
        self.profile().validate_setlist(setlist)?;
        self.read_presets()
    }

    /// Simulates a preset selection command.
    ///
    /// Validates that `bank` and `preset` fall within the range supported by
    /// the HX Stomp XL (4 banks × 32 presets) and returns `Ok(())`. No I/O
    /// is performed.
    fn select_preset(&self, setlist: u8, preset: u8) -> Result<(), HxError> {
        let profile = self.profile();
        profile.validate_setlist(setlist)?;

        if u16::from(preset) >= profile.preset_count {
            return Err(HxError::protocol(format!(
                "preset {preset} out of range (0–{})",
                profile.preset_count - 1
            )));
        }

        eprintln!("[mock] select_preset(setlist={setlist}, preset={preset}) — OK");

        Ok(())
    }

    fn read_current_preset(&self) -> Result<PresetDetails, HxError> {
        Ok(PresetDetails { blocks: Vec::new() })
    }

    fn read_current_preset_index(&self) -> Result<Option<u8>, HxError> {
        Ok(Some(0))
    }

    fn rename_preset(&self, setlist: u8, preset: u8, name: &str) -> Result<(), HxError> {
        self.profile().validate_setlist(setlist)?;
        if u16::from(preset) >= self.profile().preset_count {
            return Err(HxError::protocol(format!("preset {preset} out of range")));
        }
        if name.is_empty() || name.contains('\0') {
            return Err(HxError::protocol(
                "preset name must be non-empty and contain no NUL",
            ));
        }
        eprintln!("[mock] rename_preset(setlist={setlist}, preset={preset}, name={name:?}) — OK");
        Ok(())
    }

    fn save_preset(&self, setlist: u8, preset: u8, name: &str) -> Result<(), HxError> {
        self.rename_preset(setlist, preset, name)?;
        eprintln!("[mock] save_preset(setlist={setlist}, preset={preset}, name={name:?}) — OK");
        Ok(())
    }

    fn move_block(&self, source: u8, destination: u8, temporary: u8) -> Result<(), HxError> {
        eprintln!(
            "[mock] move_block(source={source}, destination={destination}, temporary={temporary}) — OK"
        );
        Ok(())
    }

    fn select_block(&self, block: u8, model: u8) -> Result<(), HxError> {
        eprintln!("[mock] select_block(block={block}, model={model}) — OK");
        Ok(())
    }

    fn set_parameter(
        &self,
        block: u8,
        parameter: u8,
        value: f32,
        model: u8,
    ) -> Result<(), HxError> {
        if model > 1 {
            return Err(HxError::protocol("model must be 0 (main) or 1 (paired)"));
        }
        if !value.is_finite() {
            return Err(HxError::protocol("parameter value must be finite"));
        }
        eprintln!(
            "[mock] set_parameter(block={block}, parameter={parameter}, value={value}, model={model}) — OK"
        );
        Ok(())
    }
}

static MOCK_PRESET_NAMES: [&str; 128] = [
    "Clean Tweed",
    "Warm Jazz",
    "Vintage AC",
    "Blackface Clean",
    "Boutique Clean",
    "Crystal Clear",
    "Funk Machine",
    "Sparkle Top",
    "Tweed Crunch",
    "Plexi Crunch",
    "Edge Breakup",
    "Blues Drive",
    "Smooth OD",
    "Dirty Thirty",
    "Texas Blues",
    "Chicago Blues",
    "Hi-Gain Mesa",
    "Double Recto",
    "Modern Metal",
    "Heavy Rhythm",
    "Djent Machine",
    "Thrash Lead",
    "Doom Riff",
    "Death Chunk",
    "Sustain Lead",
    "Screaming Lead",
    "Singing Lead",
    "Violin Lead",
    "Pink Lead",
    "Arena Solo",
    "AOR Lead",
    "Country Lead",
    "Chorus Pad",
    "Vintage Chorus",
    "Lush Chorus",
    "CE-1 Vibe",
    "Rotary Sim",
    "Tremolo Pad",
    "Vibrato Warm",
    "Ring Mod",
    "Long Echo",
    "Ping Pong",
    "Slapback",
    "Tape Echo",
    "Vintage Echo",
    "Reverse Dly",
    "Lofi Delay",
    "Stereo Echo",
    "Hall Reverb",
    "Room Reverb",
    "Plate Reverb",
    "Spring Reverb",
    "Church Verb",
    "Cave Reverb",
    "Arena Verb",
    "Shimmer Pad",
    "Wah Drive",
    "Auto Wah",
    "Envelope Fltr",
    "Tron Vibe",
    "Q Filter",
    "LFO Filter",
    "Step Filter",
    "Cocked Wah",
    "Fuzz Face",
    "Big Muff",
    "OCD Drive",
    "Klon Drive",
    "Tubescreamer",
    "Dist Plus",
    "RAT Drive",
    "Metal Zone",
    "Comp Sustain",
    "Orange Squish",
    "LA-2A Style",
    "1176 Style",
    "Optical Comp",
    "Dyna Comp",
    "Smooth Comp",
    "Studio Limit",
    "Octave Down",
    "Whammy Up",
    "Pitch Harm",
    "Smart Harm",
    "Detune Pad",
    "Drop Tune",
    "Bass Octave",
    "Synth Glide",
    "Synth Bass",
    "Poly Synth",
    "Organ Tones",
    "Keyboard Sim",
    "Acoustic Sim",
    "12-String",
    "Nashville Dbl",
    "Baritone Dn",
    "Studio DI",
    "Broadcast Dry",
    "Lofi Radio",
    "Telephone FX",
    "Megaphone",
    "Cassette Warm",
    "Bass Direct",
    "Amp Sim",
    "Pedalboard 1",
    "Pedalboard 2",
    "Pedalboard 3",
    "Pedalboard 4",
    "Live Rhythm",
    "Live Lead",
    "Live Clean",
    "Live Solo",
    "Studio A",
    "Studio B",
    "Tracking 1",
    "Tracking 2",
    "Mix Ref",
    "Reamp 1",
    "Reamp 2",
    "Scratch Pad",
    "Empty 120",
    "Empty 121",
    "Empty 122",
    "Empty 123",
    "Empty 124",
    "Empty 125",
    "Empty 126",
    "Empty 127",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_client_returns_128_presets() {
        let client = MockClient::new();
        let presets = client.read_presets().unwrap();
        assert_eq!(presets.len(), 128);
    }

    #[test]
    fn mock_preset_indices_are_sequential() {
        let client = MockClient::new();
        let presets = client.read_presets().unwrap();
        for (expected, preset) in presets.iter().enumerate() {
            assert_eq!(
                preset.index, expected as u8,
                "preset at position {expected} has wrong index"
            );
        }
    }

    #[test]
    fn mock_preset_names_are_non_empty() {
        let client = MockClient::new();
        let presets = client.read_presets().unwrap();
        for preset in &presets {
            assert!(
                !preset.name.is_empty(),
                "preset {} has an empty name",
                preset.index
            );
        }
    }

    #[test]
    fn mock_profile_is_hx_stomp_xl() {
        let client = MockClient::new();
        assert_eq!(client.profile().name, "HX Stomp XL");
    }

    #[test]
    fn mock_list_setlists_matches_profile_count() {
        let client = MockClient::new();
        let setlists = client.list_setlists().unwrap();
        assert_eq!(setlists.len() as u8, client.profile().setlist_count);
        assert_eq!(setlists[0].index, 0);
    }

    #[test]
    fn mock_read_setlist_presets_accepts_setlist_zero() {
        let client = MockClient::new();
        assert_eq!(client.read_setlist_presets(0).unwrap().len(), 128);
    }

    #[test]
    fn mock_read_setlist_presets_rejects_other_setlists() {
        let client = MockClient::new();
        assert!(client.read_setlist_presets(1).is_err());
    }

    #[test]
    fn mock_select_preset_valid_range() {
        let client = MockClient::new();
        assert!(client.select_preset(0, 0).is_ok());
        assert!(client.select_preset(0, 127).is_ok());
    }

    #[test]
    fn mock_select_preset_setlist_out_of_range() {
        let client = MockClient::new();
        assert!(client.select_preset(1, 0).is_err());
    }

    #[test]
    fn mock_select_preset_preset_out_of_range() {
        let client = MockClient::new();
        assert!(client.select_preset(0, 128).is_err());
    }
}

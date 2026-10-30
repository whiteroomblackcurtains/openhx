use crate::args::DeviceArg;
use openhx_core::{connect_client, device::KnownDevice, error::HxError};
use openhx_i18n::fl;

pub fn execute(
    device: Option<DeviceArg>,
    setlist: u8,
    preset: u8,
    name: &str,
) -> Result<(), HxError> {
    #[cfg(feature = "mock")]
    eprintln!("{}", fl!("mock-mode-active"));
    let known_device = device.map(KnownDevice::from);
    let client = connect_client(known_device)?;
    println!(
        "{}",
        fl!(
            "cli-renaming-preset",
            setlist = setlist,
            preset = preset,
            name = name
        )
    );
    client.rename_preset(setlist, preset, name)?;
    println!("{}", fl!("cli-preset-renamed-success"));
    Ok(())
}

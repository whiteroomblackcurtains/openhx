use crate::args::DeviceArg;
use openhx_core::{connect_client, device::KnownDevice, error::HxError};
use openhx_i18n::fl;

pub fn execute(
    device: Option<DeviceArg>,
    block: u8,
    parameter: u8,
    value: f32,
    model: u8,
) -> Result<(), HxError> {
    #[cfg(feature = "mock")]
    eprintln!("{}", fl!("mock-mode-active"));
    let known_device = device.map(KnownDevice::from);
    let client = connect_client(known_device)?;
    println!(
        "{}",
        fl!(
            "cli-setting-parameter",
            block = block,
            parameter = parameter,
            value = value
        )
    );
    client.set_parameter(block, parameter, value, model)?;
    println!("{}", fl!("cli-parameter-set-success"));
    Ok(())
}

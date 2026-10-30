<div align="center">
  <img width="190"  alt="OpenHX Logo" src="https://github.com/user-attachments/assets/4b159aec-5a66-4afd-8f5d-e68fb9c1eab0" />

  # OpenHX

  <p><em>An unofficial, open-source alternative to HX Edit — built in Rust, runs everywhere.</em></p>

  [![Built with Rust](https://img.shields.io/badge/built_with-Rust-dca282.svg)](https://www.rust-lang.org/)
  [![Version](https://img.shields.io/github/v/release/allansomensi/openhx?color=blue&label=version)](https://github.com/allansomensi/openhx/releases)
  [![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

  <br/>
</div>

OpenHX is a community-driven tool for managing Line 6 HX series devices (HX Stomp, HX Stomp XL, and Helix family) from any operating system. It ships as both a **command-line interface** and a **graphical interface**, with no dependency on HX Edit or any official Line 6 software.

---

## 🎯 Motivation

HX Edit — the official companion software for Line 6 HX devices — is only supported on Windows and macOS. Linux users, BSD users, and anyone running an unsupported OS are left without a way to manage their presets, update settings, or interact with their hardware programmatically.

OpenHX was built to close that gap.

---

## ✨ Features

> ⚠️ OpenHX is in early development. Features marked 🚧 are planned but not yet implemented.

- ✅ List setlists (with the names stored on the device)
- ✅ List presets (per setlist on Helix-family devices)
- ✅ Select preset (by setlist and slot)
- ✅ Rename preset
- ✅ Change floating-point block parameters (CLI)
- 🚧 Import and export presets (`.hlx` files)
- 🚧 Reorder blocks by drag and drop (UI interaction is present; hardware protocol validation is pending)
- 🚧 Toggle individual effect blocks
- ✅ Load and validate mono or stereo 48 kHz impulse-response WAV files
- 🚧 Upload impulse responses to device flash
- ✅ Offline preset editing workspace
- 🚧 Configure Global EQ
- ✅ HX Edit-inspired GUI with model-specific block parameters
- 🚧 Support for multiple HX devices

---

## ✅ Supported Devices

| Device | Status |
|---|---|
| HX Stomp | ✅ Validated |
| HX Stomp XL | ✅ Validated |
| HX Effects | 🚧 Planned |
| Helix Floor | ✅ Validated (all 8 setlists) |
| Helix LT | 🚧 Planned |

---

## 📥 Installation

You can find pre-built binaries, installers (`.msi`), and automated installation scripts (`.ps1`, `.sh`) on the **Releases** page.

> **Windows Users:** If you encounter a "SmartScreen" warning when running the MSI, click **"More info"** → **"Run anyway"**. This is normal for unsigned open-source tools.

<details>
<summary><strong>Or install from source (Cargo)</strong></summary>

**Prerequisites:** Rust toolchain (`rustup`), `libusb` 1.0+.

```bash
git clone https://github.com/allansomensi/openhx
cd openhx
cargo build --release
```

The binary will be at `target/release/openhx`.
</details>

---

## 💻 Usage

### CLI

```bash
# List all presets of the first setlist
openhx-cli preset list

# List the device's setlists
openhx-cli setlist list

# List the presets of another setlist (Helix-family devices hold 8, indexed 0–7)
openhx-cli preset list --setlist 2

# Select a preset by setlist and slot (slot 0–127 within the setlist)
openhx-cli preset select --setlist <SETLIST> --preset <PRESET>

# Rename a stored preset without changing its signal chain
openhx-cli preset rename --setlist <SETLIST> --preset <PRESET> --name "<NAME>"

# Change a floating-point parameter in the active preset
# block and parameter use the zero-based device indices
openhx-cli preset set-parameter --block <BLOCK> --parameter <PARAMETER> --value <VALUE>
```

### GUI

```bash
openhx
```

The GUI uses Dioxus Desktop with an HX Edit-inspired dark workspace: a preset browser,
compact colour-coded signal-chain tiles, model artwork, a selected-block preview, and a
resizable parameter inspector. The bundled HX Edit category artwork is embedded in the
Windows binary and used for the block tiles and model preview. Parameter controls are
loaded from the bundled HX Edit model resources, so each supported block shows its own
parameter names, ranges, and defaults. Parameter changes are sent to the active edit
buffer as controls change; preset renaming is persisted immediately. Use the **Save**
button to write the edited active buffer to the selected preset slot. Clicking a block
selects it on the hardware using the verified HX Edit block-selection exchange. Dragging
provides the UI interaction, but connected-device block reordering is temporarily
disabled until the separate HX Edit move exchange is captured and verified; this prevents
an unsafe command from causing USB timeouts.
Preset block discovery and current parameter values are read from the device edit buffer.
Routing, snapshots, and the remaining editor workflows are still being expanded.

---

## 🔬 How It Works

OpenHX communicates with HX devices over **USB bulk transfers** using the same vendor-specific protocol as HX Edit.

The protocol was reverse-engineered entirely **black-box** — by capturing and analyzing USB traffic between an HX Stomp XL and the official HX Edit software. No Line 6 source code, firmware, or proprietary SDKs were accessed or used at any point. The reverse engineering was done **strictly for the purpose of interoperability** with a platform not officially supported by Line 6.

The full protocol is documented in [`docs/protocol/`](./docs/protocol/README.md).

---

## 🤝 Contributing

Contributions are very welcome, especially:

- Protocol research for additional devices or operations
- Testing on different hardware and operating systems
- GUI development
- Documentation improvements

Please open an issue before starting work on a large feature so we can coordinate.

---

## ⚖️ Legal

**OpenHX is an unofficial, community-driven, open-source project. It is not affiliated with, endorsed by, sponsored by, or associated with Line 6 or Yamaha Guitar Group, Inc. in any way.**

"Line 6", "HX Stomp", "HX Effects", "Helix", and "HX Edit" are trademarks of Yamaha Guitar Group, Inc. All trademarks are the property of their respective owners and are used here strictly for descriptive and nominative purposes.

The reverse engineering performed in this project was conducted entirely black-box — by observing USB communication between the device and the official software — and is intended solely to enable interoperability on platforms not officially supported. No proprietary code, firmware, or trade secrets were accessed or incorporated.

OpenHX is released under the [MIT License](./LICENSE).

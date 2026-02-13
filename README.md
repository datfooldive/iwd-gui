# iwd-gui

A simple GUI to manage Wi-Fi through `iwd`, built with Rust + `egui`.

## Features

- View wireless devices
- Scan visible Wi-Fi networks
- Connect to networks (with passphrase when required)
- View saved networks
- Update `AutoConnect` on saved networks
- Forget saved networks

## Requirements

- Linux with `iwd` running
- D-Bus access to service `net.connman.iwd`
- Rust toolchain (stable)

## Run

```bash
cargo run
```

## Build

```bash
cargo build --release
```

## Code Structure

- `src/main.rs`: application entry point
- `src/app.rs`: app state and UI logic
- `src/dbus.rs`: D-Bus integration for iwd
- `src/models.rs`: shared data models

## Notes

The app shows status errors when connection to iwd D-Bus fails or when no wireless devices are found.

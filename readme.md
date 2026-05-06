# kgba

An experimental Game Boy Advance emulator written in Rust using KVM. The current implementation is a minimal milestone that can run a bitmap mode 3 homebrew ROM.

## Requirements

- Raspberry Pi 4 / aarch64 Linux
- `/dev/kvm` available
- SDL2 installed
- Rust toolchain installed

## Running

By default, kgba runs the ROM through the KVM backend.

```bash
cargo run -- roms/02.gba
```

Use `--duration-ms` to run for a fixed amount of time and then exit.

```bash
cargo run -- --duration-ms 3000 roms/02.gba
```

## Software Fallback

For development checks without KVM, use the limited software runner for this sample ROM.

```bash
cargo run -- --software --headless roms/02.gba
cargo run -- --software --duration-ms 1000 roms/02.gba
```

`--software` is only a development fallback. The main execution path is the KVM backend.

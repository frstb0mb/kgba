# kgba

An experimental Game Boy Advance emulator written in Rust using KVM. The current implementation is a minimal milestone that can run a bitmap mode 3 homebrew ROM.

## Requirements

- Raspberry Pi 4 / aarch64 Linux
- `/dev/kvm` available
- SDL2 installed
- Rust toolchain installed

## Running

Pass a ROM path to run it through the KVM backend.

```bash
cargo run -- <ROM PATH>
```

Run another ROM by passing a different path.

```bash
cargo run -- <ROM PATH>
```

Use `--duration-ms` to run for a fixed amount of time and then exit.

```bash
cargo run -- --duration-ms 3000 <ROM PATH>
```

## Software Fallback

For development checks without KVM, use the limited software runner for this sample ROM.

```bash
cargo run -- --software --headless <ROM PATH>
cargo run -- --software --duration-ms 1000 <ROM PATH>
```

`--software` is only a development fallback. The main execution path is the KVM backend.

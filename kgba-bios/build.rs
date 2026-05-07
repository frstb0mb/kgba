use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const BIOS_SIZE: usize = 0x4000;

fn main() {
    println!("cargo:rerun-if-changed=src/bios.arm.s");
    println!("cargo:rerun-if-changed=src/bios.ld");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let object = out_dir.join("bios.o");
    let elf = out_dir.join("bios.elf");
    let bin = out_dir.join("bios.bin");
    let map = out_dir.join("bios.map");

    run(
        Command::new(tool("as")).args(["-g", "-o", path_arg(&object), "src/bios.arm.s"]),
        "assemble BIOS",
    );
    run(
        Command::new(tool("ld")).args([
            "-T",
            "src/bios.ld",
            "-Map",
            path_arg(&map),
            "-o",
            path_arg(&elf),
            path_arg(&object),
        ]),
        "link BIOS",
    );
    run(
        Command::new(tool("objcopy")).args(["-O", "binary", path_arg(&elf), path_arg(&bin)]),
        "extract BIOS binary",
    );

    let mut image = fs::read(&bin).expect("read BIOS binary");
    assert!(
        image.len() <= BIOS_SIZE,
        "BIOS binary is too large: {} bytes",
        image.len()
    );
    image.resize(BIOS_SIZE, 0);
    fs::write(&bin, &image).expect("write padded BIOS binary");
    fs::write(out_dir.join("bios_image.rs"), rust_image(&image)).expect("write BIOS Rust image");
}

fn tool(name: &str) -> String {
    let var = format!("KGBA_BIOS_{}", name.to_ascii_uppercase());
    env::var(&var).unwrap_or_else(|_| format!("arm-none-eabi-{name}"))
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("path is UTF-8")
}

fn run(command: &mut Command, context: &str) {
    let status = command
        .status()
        .unwrap_or_else(|err| panic!("{context}: failed to start {command:?}: {err}"));
    assert!(
        status.success(),
        "{context}: command failed with {status}: {command:?}"
    );
}

fn rust_image(image: &[u8]) -> String {
    let mut output = format!("pub const DEFAULT_BIOS_IMAGE: [u8; {}] = [\n", BIOS_SIZE);
    for chunk in image.chunks(16) {
        output.push_str("    ");
        for byte in chunk {
            output.push_str(&format!("0x{byte:02x}, "));
        }
        output.push('\n');
    }
    output.push_str("];\n");
    output
}

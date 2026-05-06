use std::{
    env,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use kgba::{
    gba::{
        bus::Bus,
        cartridge::Cartridge,
        memory::GbaMemory,
        software::{RunResult, SoftwareRunner},
    },
    kvm::KvmGba,
    platform::sdl::Video,
};

fn main() {
    if let Err(err) = run() {
        eprintln!("kgba: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut headless = false;
    let mut software = false;
    let mut duration_ms = None;
    let mut rom_path = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--headless" {
            headless = true;
        } else if arg == "--software" {
            software = true;
        } else if arg == "--duration-ms" {
            let value = args
                .next()
                .ok_or_else(|| "--duration-ms requires a value".to_owned())?;
            duration_ms = Some(
                value
                    .parse::<u64>()
                    .map_err(|err| format!("invalid --duration-ms value {value}: {err}"))?,
            );
        } else {
            rom_path = Some(arg);
        }
    }
    let rom_path = rom_path.unwrap_or_else(|| "roms/02.gba".to_owned());
    let cartridge = Cartridge::load(&rom_path).map_err(|err| format!("{rom_path}: {err}"))?;

    if software {
        return run_software(&rom_path, &cartridge, headless, duration_ms);
    }

    run_kvm(&cartridge, duration_ms)
}

fn run_kvm(cartridge: &Cartridge, duration_ms: Option<u64>) -> Result<(), String> {
    let machine = KvmGba::new(cartridge)?;
    let shared = machine.shared_memory();
    let stop = Arc::new(AtomicBool::new(false));
    let kvm_stop = Arc::clone(&stop);

    std::thread::spawn(move || {
        if let Err(err) = machine.run(kvm_stop) {
            eprintln!("kgba kvm: {err}");
        }
    });

    let mut video = Video::new("kgba - KVM mode 3")?;
    if let Some(duration_ms) = duration_ms {
        let started = Instant::now();
        let mut vcount = 0u16;
        while started.elapsed() < Duration::from_millis(duration_ms) {
            shared.set_vcount(if vcount < 160 { vcount } else { 160 });
            video.present(&shared.render_mode3())?;
            vcount = if vcount + 1 >= 228 { 0 } else { vcount + 1 };
            std::thread::sleep(Duration::from_millis(16));
        }
        stop.store(true, Ordering::Relaxed);
        return Ok(());
    }

    video.run_frame_loop(|vcount| {
        shared.set_vcount(if vcount < 160 { vcount } else { 160 });
        shared.render_mode3()
    })
}

fn run_software(
    rom_path: &str,
    cartridge: &Cartridge,
    headless: bool,
    duration_ms: Option<u64>,
) -> Result<(), String> {
    let mut memory = GbaMemory::new();
    let mut bus = Bus::new(&mut memory);
    let mut runner = SoftwareRunner::new_for_rom(&cartridge);

    let result = runner.run_until_frame(&cartridge, &mut bus)?;
    if result != RunResult::FrameReady {
        return Err(format!("ROM did not produce a frame: {result:?}"));
    }

    let frame = bus.render_frame_argb8888();
    if headless {
        let lit_pixels = frame.iter().filter(|&&pixel| pixel != 0xff000000).count();
        println!(
            "kgba loaded={} dispcnt={:#06x} lit_pixels={}",
            rom_path,
            bus.ppu().dispcnt(),
            lit_pixels
        );
        return Ok(());
    }

    let mut video = Video::new("kgba - mode 3")?;
    if let Some(duration_ms) = duration_ms {
        video.present(&frame)?;
        std::thread::sleep(Duration::from_millis(duration_ms));
        Ok(())
    } else {
        video.run_until_quit(&frame, Duration::from_millis(500))
    }
}

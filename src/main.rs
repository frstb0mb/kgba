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
        ppu::TOTAL_SCANLINES,
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
    let rom_path = rom_path.ok_or_else(|| {
        "usage: kgba [--duration-ms N] [--software] [--headless] <rom.gba>".to_owned()
    })?;
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
    let vcount_stop = Arc::clone(&stop);
    let vcount_memory = Arc::clone(&shared);

    std::thread::spawn(move || {
        if let Err(err) = machine.run(kvm_stop) {
            eprintln!("kgba kvm: {err}");
        }
    });

    std::thread::spawn(move || run_vcount_clock(vcount_memory, vcount_stop));

    let mut video = Video::new("kgba - KVM mode 3")?;
    if let Some(duration_ms) = duration_ms {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(duration_ms) {
            let (_, keyinput) = video.poll_events_and_input();
            shared.set_keyinput(keyinput);
            video.present(&shared.render_frame())?;
        }
        stop.store(true, Ordering::Relaxed);
        return Ok(());
    }

    let result = video.run_frame_loop(
        |_| shared.render_frame(),
        |keyinput| shared.set_keyinput(keyinput),
    );
    stop.store(true, Ordering::Relaxed);
    result
}

fn run_vcount_clock(shared: Arc<kgba::kvm::KvmSharedMemory>, stop: Arc<AtomicBool>) {
    let scanline = Duration::from_nanos(73_433);
    let mut vcount = 0u16;
    let mut next_tick = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        shared.set_vcount(vcount);
        vcount += 1;
        if vcount >= TOTAL_SCANLINES {
            vcount = 0;
        }
        next_tick += scanline;
        while Instant::now() < next_tick {
            std::hint::spin_loop();
        }
    }
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

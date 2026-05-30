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
    platform::sdl::{Audio, Video},
};

const USAGE: &str = "\
usage: kgba [--duration-ms N] [--headless] <rom.gba>
       kgba --software [--duration-ms N] [--headless] <rom.gba>

Default execution uses the KVM backend. The software runner is a development
fallback and is enabled only when --software is explicitly passed.";

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
        if arg == "--help" || arg == "-h" {
            println!("{USAGE}");
            return Ok(());
        } else if arg == "--headless" {
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
    let rom_path = rom_path.ok_or_else(|| USAGE.to_owned())?;
    let cartridge = Cartridge::load(&rom_path).map_err(|err| format!("{rom_path}: {err}"))?;

    if software {
        return run_software(&rom_path, &cartridge, headless, duration_ms);
    }

    run_kvm(&cartridge, headless, duration_ms)
}

fn run_kvm(cartridge: &Cartridge, headless: bool, duration_ms: Option<u64>) -> Result<(), String> {
    let machine = KvmGba::new(cartridge)?;
    let shared = machine.shared_memory();
    let stop = Arc::new(AtomicBool::new(false));
    let kvm_stop = Arc::clone(&stop);
    let kvm_error_stop = Arc::clone(&stop);
    let vcount_stop = Arc::clone(&stop);
    let vcount_memory = Arc::clone(&shared);
    let audio_stop = Arc::clone(&stop);
    let audio_memory = Arc::clone(&shared);

    std::thread::spawn(move || {
        if let Err(err) = machine.run(kvm_stop) {
            eprintln!("kgba kvm: {err}");
            kvm_error_stop.store(true, Ordering::Relaxed);
        }
    });

    std::thread::spawn(move || run_vcount_clock(vcount_memory, vcount_stop));
    std::thread::spawn(move || run_audio_clock(audio_memory, audio_stop));

    if headless {
        let duration_ms = duration_ms.unwrap_or(500);
        run_headless_input_script(&shared, duration_ms);
        let frame = if shared.needs_scanline_renderer() {
            shared.with_completed_frame(|_, frame| frame.to_vec())
        } else {
            shared.render_frame()
        };
        let frame_hash = frame_hash(&frame);
        let lit_pixels = frame.iter().filter(|&&pixel| pixel != 0).count();
        let video = shared.debug_video_state();
        println!(
            "kgba kvm lit_pixels={} frame_hash={:#018x} dispcnt={:#06x} bg0cnt={:#06x} bg1cnt={:#06x} bg2cnt={:#06x} bg0hofs={:#06x} bg1hofs={:#06x} keyinput={:#06x} irq_waitflags={:#06x} mosaic={:#06x} dma3cnt={:#06x} palette0={:#06x} vram0={:#06x} bg0_map_nonzero={} bg0_text_1={:#06x} bg0_text_2={:#06x} cycle_digits={}{} cx_digits={}{} bg1_raster_min={:#06x} bg1_raster_max={:#06x} bg1_raster_sample={:#06x} bg1_raster_checksum={} audio_dma1sad={:#010x} audio_dma2sad={:#010x} audio_dma1cnt={:#06x} audio_dma2cnt={:#06x} audio_wave_left_nonzero={} audio_wave_right_nonzero={} maxmod_wavebuffer_var={:?} maxmod_writepos={:#010x} maxmod_mixlen={} maxmod_mix_seg={}",
            lit_pixels,
            frame_hash,
            video.dispcnt,
            video.bg0cnt,
            video.bg1cnt,
            video.bg2cnt,
            video.bg0hofs,
            video.bg1hofs,
            video.keyinput,
            video.irq_waitflags,
            video.mosaic,
            video.dma3cnt,
            video.palette0,
            video.vram0,
            video.bg0_map_nonzero,
            video.bg0_text_1,
            video.bg0_text_2,
            tile_ascii(video.cycle_digit_10),
            tile_ascii(video.cycle_digit_1),
            tile_ascii(video.cx_digit_10),
            tile_ascii(video.cx_digit_1),
            video.bg1_raster_min,
            video.bg1_raster_max,
            video.bg1_raster_sample,
            video.bg1_raster_checksum,
            video.audio_dma1sad,
            video.audio_dma2sad,
            video.audio_dma1cnt,
            video.audio_dma2cnt,
            video.audio_wave_left_nonzero,
            video.audio_wave_right_nonzero,
            video.maxmod_wavebuffer_var,
            video.maxmod_writepos,
            video.maxmod_mixlen,
            video.maxmod_mix_seg
        );
        stop.store(true, Ordering::Relaxed);
        return Ok(());
    }

    let mut video = Video::new("kgba - KVM mode 3")?;
    let _audio = Audio::new(Arc::clone(&shared))?;
    if let Some(duration_ms) = duration_ms {
        let started = Instant::now();
        let scripted_key = env::var("KGBA_HEADLESS_KEY")
            .ok()
            .and_then(|key| keyinput_for_name(&key));
        let scripted_press_at = Duration::from_millis((duration_ms / 4).max(1));
        let mut next_present = Instant::now();
        let mut present_count = 0u64;
        let mut last_presented_seq = None;
        let mut last_keyinput = 0xffff;
        while started.elapsed() < Duration::from_millis(duration_ms) {
            let (_, mut keyinput) = video.poll_events_and_input();
            if let Some(scripted_key) = scripted_key {
                if started.elapsed() >= scripted_press_at {
                    keyinput = scripted_key;
                }
            }
            shared.set_keyinput(keyinput);
            trace_video_input(keyinput, &mut last_keyinput);
            let now = Instant::now();
            if now >= next_present {
                if shared.needs_scanline_renderer() {
                    let seq = shared.completed_frame_seq();
                    if last_presented_seq != Some(seq) {
                        let snapshot = shared.latest_frame_snapshot();
                        let present_duration = video.present_timed(&snapshot.pixels)?;
                        shared.record_sdl_present(present_duration);
                        trace_video_frame(present_count, snapshot.seq, &snapshot.pixels, &shared);
                        last_presented_seq = Some(snapshot.seq);
                    } else {
                        trace_video_frame_skip(present_count, seq, &shared);
                    }
                } else {
                    let frame = shared.render_frame();
                    let present_duration = video.present_timed(&frame)?;
                    shared.record_sdl_present(present_duration);
                    trace_video_frame(present_count, shared.completed_frame_seq(), &frame, &shared);
                }
                present_count += 1;
                next_present += FRAME_INTERVAL;
                if next_present < now {
                    next_present = now + FRAME_INTERVAL;
                }
            }
            std::thread::sleep(INPUT_POLL_INTERVAL);
        }
        stop.store(true, Ordering::Relaxed);
        return Ok(());
    }

    let mut present_count = 0u64;
    let mut last_presented_seq = None;
    let mut last_keyinput = 0xffff;
    let result = video.run_frame_loop(
        |video, _| {
            if shared.needs_scanline_renderer() {
                let seq = shared.completed_frame_seq();
                if last_presented_seq != Some(seq) {
                    let snapshot = shared.latest_frame_snapshot();
                    let present_duration = video.present_timed(&snapshot.pixels)?;
                    shared.record_sdl_present(present_duration);
                    trace_video_frame(present_count, snapshot.seq, &snapshot.pixels, &shared);
                    last_presented_seq = Some(snapshot.seq);
                } else {
                    trace_video_frame_skip(present_count, seq, &shared);
                }
            } else {
                let frame = shared.render_frame();
                let present_duration = video.present_timed(&frame)?;
                shared.record_sdl_present(present_duration);
                trace_video_frame(present_count, shared.completed_frame_seq(), &frame, &shared);
            }
            present_count += 1;
            Ok(())
        },
        |keyinput| {
            shared.set_keyinput(keyinput);
            trace_video_input(keyinput, &mut last_keyinput);
        },
    );
    stop.store(true, Ordering::Relaxed);
    result
}

fn run_vcount_clock(shared: Arc<kgba::kvm::KvmSharedMemory>, stop: Arc<AtomicBool>) {
    let scanline = Duration::from_nanos(73_433);
    let hdraw = Duration::from_nanos(57_213);
    let debug_vcount0_hold = std::env::var("KGBA_DEBUG_VCOUNT0_HOLD_US")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_micros)
        .unwrap_or_default();
    let debug_drop_late_vcount = std::env::var_os("KGBA_DEBUG_DROP_LATE_VCOUNT").is_some();
    let mut vcount = 0u16;
    let mut next_tick = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let needs_scanline_renderer = shared.needs_scanline_renderer();
        shared.set_vcount(vcount);
        shared.tick_scanline();
        if vcount < 160 && needs_scanline_renderer {
            shared.render_scanline(usize::from(vcount));
        }
        wait_until(next_tick + hdraw, &stop);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if !shared.hblank_irq_pending() {
            if let Some(seq) = shared.enter_hblank() {
                wait_for_hblank_complete(&shared, seq, &stop);
            }
        }
        vcount += 1;
        if vcount == 160 {
            if needs_scanline_renderer {
                shared.publish_completed_frame();
            }
        }
        if vcount >= TOTAL_SCANLINES {
            vcount = 0;
        }
        next_tick += scanline;
        if debug_drop_late_vcount && Instant::now().saturating_duration_since(next_tick) > scanline
        {
            next_tick = Instant::now() + scanline;
        }
        if vcount == 0 && !debug_vcount0_hold.is_zero() {
            wait_until(Instant::now() + debug_vcount0_hold, &stop);
        }
        wait_until(next_tick, &stop);
    }
}

fn run_audio_clock(shared: Arc<kgba::kvm::KvmSharedMemory>, stop: Arc<AtomicBool>) {
    let mut last = Instant::now();
    let mut fractional_cycles = 0u64;
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        let elapsed_ns = now.duration_since(last).as_nanos() as u64;
        last = now;

        let total = elapsed_ns
            .saturating_mul(GBA_CLOCK_HZ)
            .saturating_add(fractional_cycles);
        let cycles = total / 1_000_000_000;
        fractional_cycles = total % 1_000_000_000;
        if cycles != 0 {
            shared.tick_audio_cycles(cycles.min(u64::from(u32::MAX)) as u32);
        }

        std::thread::sleep(AUDIO_CLOCK_INTERVAL);
    }
}

fn wait_for_hblank_complete(shared: &kgba::kvm::KvmSharedMemory, seq: u64, stop: &AtomicBool) {
    if !stop.load(Ordering::Relaxed) {
        shared.wait_for_hblank_complete(seq, HBLANK_COMPLETION_TIMEOUT);
    }
}

fn wait_until(deadline: Instant, stop: &AtomicBool) {
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline.duration_since(now);
        if remaining > Duration::from_micros(500) {
            std::thread::sleep(remaining - Duration::from_micros(200));
        } else {
            std::hint::spin_loop();
        }
    }
}

const FRAME_INTERVAL: Duration = Duration::from_micros(16_742);
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(1);
const HBLANK_COMPLETION_TIMEOUT: Duration = Duration::from_millis(1);
const AUDIO_CLOCK_INTERVAL: Duration = Duration::from_micros(500);
const GBA_CLOCK_HZ: u64 = 16_777_216;
const KEYINPUT_RELEASED: u16 = 0x03ff;
const KEY_A: u16 = 1 << 0;
const KEY_B: u16 = 1 << 1;
const KEY_SELECT: u16 = 1 << 2;
const KEY_START: u16 = 1 << 3;
const KEY_RIGHT: u16 = 1 << 4;
const KEY_LEFT: u16 = 1 << 5;
const KEY_UP: u16 = 1 << 6;
const KEY_DOWN: u16 = 1 << 7;
const KEY_R: u16 = 1 << 8;
const KEY_L: u16 = 1 << 9;

fn run_headless_input_script(shared: &Arc<kgba::kvm::KvmSharedMemory>, duration_ms: u64) {
    let Some(keyinput) = env::var("KGBA_HEADLESS_KEY")
        .ok()
        .and_then(|key| keyinput_for_name(&key))
    else {
        std::thread::sleep(Duration::from_millis(duration_ms));
        return;
    };

    let press_at = Duration::from_millis((duration_ms / 4).max(1));
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(duration_ms) {
        if started.elapsed() >= press_at {
            shared.set_keyinput(keyinput);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    shared.set_keyinput(KEYINPUT_RELEASED);
}

fn keyinput_for_name(name: &str) -> Option<u16> {
    let bit = match name {
        "a" => KEY_A,
        "b" => KEY_B,
        "select" => KEY_SELECT,
        "start" => KEY_START,
        "right" | "d" => KEY_RIGHT,
        "left" => KEY_LEFT,
        "up" | "w" => KEY_UP,
        "down" | "s" => KEY_DOWN,
        "r" => KEY_R,
        "l" => KEY_L,
        _ => return None,
    };
    Some(KEYINPUT_RELEASED & !bit)
}

fn tile_ascii(tile: u16) -> char {
    char::from_u32(u32::from(tile & 0x00ff) + u32::from(b' ')).unwrap_or('?')
}

fn frame_hash(frame: &[u16]) -> u64 {
    frame.iter().fold(0xcbf2_9ce4_8422_2325, |hash, pixel| {
        (hash ^ u64::from(*pixel)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn trace_video_frame(count: u64, seq: u64, frame: &[u16], shared: &kgba::kvm::KvmSharedMemory) {
    if env::var_os("KGBA_TRACE_VIDEO").is_some() && count.is_multiple_of(30) {
        eprintln!(
            "kgba video event=present count={} completed_seq={} frame_hash={:#018x}",
            count,
            seq,
            frame_hash(frame)
        );
        trace_video_perf(count, seq, shared);
    }
}

fn trace_video_frame_skip(count: u64, seq: u64, shared: &kgba::kvm::KvmSharedMemory) {
    if env::var_os("KGBA_TRACE_VIDEO").is_some() && count.is_multiple_of(30) {
        eprintln!("kgba video event=present_skip count={count} completed_seq={seq}");
        trace_video_perf(count, seq, shared);
    }
}

fn trace_video_perf(count: u64, seq: u64, shared: &kgba::kvm::KvmSharedMemory) {
    let perf = shared.take_video_perf_snapshot();
    eprintln!(
        "kgba video event=perf presents=30 frames={} completed_seq={} present_lag={} render_scanline_us={} hblank_wait_us={} hblank_wait_max_us={} hblank_timeouts={} fast_hblank_us={} fast_hblank_count={} fast_hblank_shared={} fast_hblank_mmio={} mmio_exits={} mmio_fast_exit={} mmio_io_reads={} mmio_io_writes={} mmio_io_if={} mmio_io_ime={} mmio_io_bg_hofs={} mmio_io_bg_vofs={} mmio_io_other={} sdl_present_us={}",
        perf.frames,
        seq,
        count.saturating_sub(seq),
        perf.render_scanline_us,
        perf.hblank_wait_us,
        perf.hblank_wait_max_us,
        perf.hblank_wait_timeouts,
        perf.fast_hblank_us,
        perf.fast_hblank_count,
        perf.fast_hblank_shared_count,
        perf.fast_hblank_mmio_count,
        perf.kvm_mmio_exits,
        perf.kvm_mmio_fast_exit,
        perf.kvm_mmio_io_reads,
        perf.kvm_mmio_io_writes,
        perf.kvm_mmio_io_if,
        perf.kvm_mmio_io_ime,
        perf.kvm_mmio_io_bg_hofs,
        perf.kvm_mmio_io_bg_vofs,
        perf.kvm_mmio_io_other,
        perf.sdl_present_us
    );
}

fn trace_video_input(keyinput: u16, last_keyinput: &mut u16) {
    if env::var_os("KGBA_TRACE_VIDEO").is_some() && keyinput != *last_keyinput {
        eprintln!("kgba video event=input keyinput={keyinput:#06x}");
        *last_keyinput = keyinput;
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

    let frame = bus.render_frame_bgr555();
    if headless {
        let lit_pixels = frame.iter().filter(|&&pixel| pixel != 0).count();
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

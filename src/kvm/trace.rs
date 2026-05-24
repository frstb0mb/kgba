use std::{
    env,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use crate::gba::memory_map::{DISPSTAT, IE, IF, IME, IO_START, KEYINPUT, MOSAIC};

pub fn trace_timer_register_write(addr: u32, value: u16) {
    if trace_enabled("KGBA_TRACE_TIMER") {
        eprintln!(
            "kgba timer write addr={addr:#010x} value={value:#06x} kind={}",
            if (addr - (IO_START + 0x0100)) & 0x2 == 0 {
                "reload"
            } else {
                "control"
            }
        );
    }
}

pub fn trace_timer_counter(timer_index: usize, value: u16, ticks: u32, overflows: u32) {
    if !trace_enabled("KGBA_TRACE_TIMER") || ticks == 0 {
        return;
    }

    static COUNTER_LOGS: AtomicU64 = AtomicU64::new(0);
    let log_index = COUNTER_LOGS.fetch_add(1, Ordering::Relaxed);
    if log_index < 32 || log_index.is_multiple_of(1024) {
        eprintln!(
            "kgba timer advance timer={} value={} ticks={} overflows={}",
            timer_index, value, ticks, overflows
        );
    }
}

pub fn trace_io_mmio(kind: &str, addr: u32, len: u32, data: &[u8; 8]) {
    if !trace_enabled("KGBA_TRACE_MMIO") && !is_timer_register_access(addr, len) {
        return;
    }

    eprintln!(
        "kgba mmio {kind} addr={addr:#010x} len={} data={}",
        len,
        format_mmio_data(data, len)
    );
}

pub fn trace_input_keyinput(value: u16) {
    if trace_enabled("KGBA_TRACE_INPUT") {
        eprintln!(
            "kgba input t={} event=host_keyinput value={value:#06x}",
            trace_micros()
        );
    }
}

pub fn trace_input_vblank(vcount: u16, ie: u16, iflag: u16, ime: u16) {
    if trace_enabled("KGBA_TRACE_INPUT") {
        eprintln!(
            "kgba input t={} event=vblank vcount={} ie={ie:#06x} if={iflag:#06x} ime={ime:#06x}",
            trace_micros(),
            vcount
        );
    }
}

pub fn trace_input_irq_line(asserted: bool, ie: u16, iflag: u16, ime: u16) {
    if trace_enabled("KGBA_TRACE_INPUT") {
        eprintln!(
            "kgba input t={} event=irq_line asserted={} ie={ie:#06x} if={iflag:#06x} ime={ime:#06x}",
            trace_micros(),
            asserted
        );
    }
}

pub fn trace_input_io_write(addr: u32, value: u16, keyinput: u16, vcount: u16) {
    if !trace_enabled("KGBA_TRACE_INPUT") {
        return;
    }

    let event = match addr {
        DISPSTAT => "dispstat_write",
        IE => "ie_write",
        IF => "if_write",
        IME => "ime_write",
        MOSAIC => "mosaic_write",
        KEYINPUT => "keyinput_write",
        _ => return,
    };
    eprintln!(
        "kgba input t={} event={} addr={addr:#010x} value={value:#06x} keyinput={keyinput:#06x} vcount={vcount}",
        trace_micros(),
        event
    );
}

pub fn trace_fast_hblank(vcount: u16, hofs: u16, ack: u16) {
    if trace_enabled("KGBA_TRACE_FASTIRQ") {
        eprintln!(
            "kgba fastirq t={} event=hblank_complete vcount={} bg1hofs={hofs:#06x} ack={ack:#06x}",
            trace_micros(),
            vcount
        );
    }
}

pub fn trace_kvm_exit(reason: u32) {
    if !trace_enabled("KGBA_TRACE_KVMEXIT") {
        return;
    }

    static EXIT_LOGS: AtomicU64 = AtomicU64::new(0);
    let log_index = EXIT_LOGS.fetch_add(1, Ordering::Relaxed);
    if log_index < 64 || log_index.is_multiple_of(4096) {
        eprintln!(
            "kgba kvmexit t={} reason={} count={}",
            trace_micros(),
            reason,
            log_index + 1
        );
    }
}

pub fn trace_hblank_pending(vcount: u16, ie: u16, iflag: u16, ime: u16) {
    if !trace_enabled("KGBA_TRACE_FASTIRQ") {
        return;
    }

    static PENDING_LOGS: AtomicU64 = AtomicU64::new(0);
    let log_index = PENDING_LOGS.fetch_add(1, Ordering::Relaxed);
    if log_index < 16 || log_index.is_multiple_of(64) {
        eprintln!(
            "kgba fastirq t={} event=hblank_pending vcount={} ie={ie:#06x} if={iflag:#06x} ime={ime:#06x} count={}",
            trace_micros(),
            vcount,
            log_index + 1
        );
    }
}

fn is_timer_register_access(addr: u32, len: u32) -> bool {
    let end = addr.saturating_add(len);
    addr < IO_START + 0x0110 && end > IO_START + 0x0100
}

fn format_mmio_data(data: &[u8; 8], len: u32) -> String {
    data[..len as usize]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn trace_enabled(name: &'static str) -> bool {
    static TIMER: OnceLock<bool> = OnceLock::new();
    static MMIO: OnceLock<bool> = OnceLock::new();
    static INPUT: OnceLock<bool> = OnceLock::new();
    static FASTIRQ: OnceLock<bool> = OnceLock::new();
    static KVMEXIT: OnceLock<bool> = OnceLock::new();

    match name {
        "KGBA_TRACE_TIMER" => *TIMER.get_or_init(|| env::var_os(name).is_some()),
        "KGBA_TRACE_MMIO" => *MMIO.get_or_init(|| env::var_os(name).is_some()),
        "KGBA_TRACE_INPUT" => *INPUT.get_or_init(|| env::var_os(name).is_some()),
        "KGBA_TRACE_FASTIRQ" => *FASTIRQ.get_or_init(|| env::var_os(name).is_some()),
        "KGBA_TRACE_KVMEXIT" => *KVMEXIT.get_or_init(|| env::var_os(name).is_some()),
        _ => false,
    }
}

fn trace_micros() -> u128 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_micros()
}

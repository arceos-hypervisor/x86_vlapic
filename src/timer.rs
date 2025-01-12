

/// Local APIC timer modes.
#[derive(Debug, Copy, Clone)]
#[repr(u8)]
#[allow(dead_code)]
pub enum TimerMode {
    /// Timer only fires once.
    OneShot = 0b00,
    /// Timer fires periodically.
    Periodic = 0b01,
    /// Timer fires at an absolute time.
    TscDeadline = 0b10,
}

/// A virtual local APIC timer. (SDM Vol. 3C, Section 11.5.4)
pub struct ApicTimer {
    lvt_timer_bits: u32,
    divide_shift: u8,
    initial_count: u32,
    last_start_ns: u64,
    deadline_ns: u64,
}


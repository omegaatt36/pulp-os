// uptime helper backed by embassy's monotonic clock
pub fn uptime_secs() -> u32 {
    let ticks = embassy_time::Instant::now().as_ticks();
    (ticks / embassy_time::TICK_HZ) as u32
}

// microsecond reading of the same clock: sd-metrics timing and the slicing of
// long CJK preparations (fonts::cjk)
pub fn uptime_us() -> u64 {
    embassy_time::Instant::now().as_micros()
}

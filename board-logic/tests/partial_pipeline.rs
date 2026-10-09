use pulp_board_logic::lifecycle::{DisplayHealth, Refresher, retry_refresh, run_refresh};
use pulp_board_logic::power::{DelayMs, DisplayReset};
use pulp_board_logic::ssd1677::{
    BusyPin, DisplayError, Epd, EpdBus, PartialBegin, Rotation, Snapshot, StripSource,
    WindowSource, align_partial_region, capture_snapshot, cmd,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ev {
    Cmd(u8),
    Data(Vec<u8>),
    Delay(u32),
    Busy(bool),
}

struct FakePort {
    log: Vec<Ev>,
    now: u64,
    busy_until: u64,
    busy_ms: Option<u64>,
    fail_after_writes: Option<usize>,
    writes: usize,
    samples: u32,
}

impl FakePort {
    fn new(busy_ms: Option<u64>) -> Self {
        Self {
            log: Vec::new(),
            now: 0,
            busy_until: 0,
            busy_ms,
            fail_after_writes: None,
            writes: 0,
            samples: 0,
        }
    }

    fn start_busy(&mut self) {
        self.busy_until = match self.busy_ms {
            Some(ms) => self.now + ms,
            None => u64::MAX,
        };
    }

    fn write_gate(&mut self) -> Result<(), DisplayError> {
        self.writes += 1;
        match self.fail_after_writes {
            Some(n) if self.writes > n => Err(DisplayError::Bus),
            _ => Ok(()),
        }
    }
}

impl EpdBus for FakePort {
    fn command(&mut self, c: u8) -> Result<(), DisplayError> {
        self.write_gate()?;
        self.log.push(Ev::Cmd(c));
        if c == cmd::SW_RESET || c == cmd::MASTER_ACTIVATION {
            self.start_busy();
        }
        Ok(())
    }
    fn data(&mut self, d: &[u8]) -> Result<(), DisplayError> {
        self.write_gate()?;
        self.log.push(Ev::Data(d.to_vec()));
        Ok(())
    }
}

impl DelayMs for FakePort {
    fn delay_ms(&mut self, ms: u32) {
        self.log.push(Ev::Delay(ms));
        self.now += ms as u64;
    }
}

impl BusyPin for FakePort {
    fn is_busy(&mut self) -> bool {
        self.samples += 1;
        let b = self.now < self.busy_until;
        self.log.push(Ev::Busy(b));
        b
    }
    fn now_ms(&mut self) -> u64 {
        self.now
    }
}

fn ram_stream(log: &[Ev], ram_cmd: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut on = false;
    for e in log {
        match e {
            Ev::Cmd(c) => on = *c == ram_cmd,
            Ev::Data(d) if on => out.extend_from_slice(d),
            _ => {}
        }
    }
    out
}

struct BlankStrips;
impl StripSource for BlankStrips {
    fn render_strip(&mut self, _rotation: Rotation, _idx: u16) -> &[u8] {
        static BLANK: [u8; 4000] = [0xFF; 4000];
        &BLANK
    }
}

fn epd_ready() -> Epd<FakePort> {
    let mut epd = Epd::new(FakePort::new(Some(10)));
    epd.init(DisplayReset::Software).unwrap();
    let mut blank = BlankStrips;
    epd.full_refresh(&mut blank).unwrap();
    epd.port_mut().log.clear();
    epd
}

/// A dynamic window source whose output is different on every call:
/// fills successive bytes with an incrementing counter.
struct ChangingSource {
    counter: u8,
    calls: usize,
    buf: Vec<u8>,
}

impl ChangingSource {
    fn new() -> Self {
        Self {
            counter: 0,
            calls: 0,
            buf: Vec::new(),
        }
    }
}

impl WindowSource for ChangingSource {
    fn render_window(
        &mut self,
        _rotation: Rotation,
        _px: u16,
        _py: u16,
        pw: u16,
        rows: u16,
    ) -> &[u8] {
        self.calls += 1;
        let len = (pw as usize / 8) * rows as usize;
        self.buf.clear();
        for _ in 0..len {
            self.buf.push(self.counter);
            self.counter = self.counter.wrapping_add(1);
        }
        &self.buf
    }
}

#[test]
fn snapshot_equality_phase_1_and_phase_3_receive_identical_bytes_from_single_pass() {
    let mut epd = epd_ready();
    // 32x24 physical region -> 4 bytes wide x 24 rows = 96 bytes
    let rs = align_partial_region(Rotation::Deg270, 16, 8, 32, 24).unwrap();
    let needed = (rs.pw as usize / 8) * (rs.ph as usize);
    let mut snapshot_buf = vec![0u8; needed];

    let mut changing = ChangingSource::new();
    capture_snapshot(&mut changing, Rotation::Deg270, &rs, &mut snapshot_buf).unwrap();
    let calls_during_capture = changing.calls;
    assert!(calls_during_capture > 0);

    let mut snapshot = Snapshot::new(&snapshot_buf, &rs).unwrap();

    // Begin (phase 1: BW RAM)
    epd.begin_partial_refresh_state(&rs, &mut snapshot).unwrap();

    // Finish (phase 3: RED then BW RAM)
    epd.port_mut().now += 20; // let BUSY clear
    epd.finish_partial_refresh(&mut snapshot).unwrap();

    // Verify source was not queried again after capture
    assert_eq!(changing.calls, calls_during_capture);

    let log = &epd.port_mut().log;
    let red_stream = ram_stream(log, cmd::WRITE_RAM_RED);
    let bw_stream = ram_stream(log, cmd::WRITE_RAM_BW);

    assert_eq!(red_stream.len(), needed);
    assert_eq!(red_stream, snapshot_buf);

    // BW stream contains phase 1 AND phase 3 (2 * needed bytes)
    assert_eq!(bw_stream.len(), 2 * needed);
    assert_eq!(&bw_stream[..needed], &snapshot_buf[..]);
    assert_eq!(&bw_stream[needed..], &snapshot_buf[..]);

    // All phases match identically
    assert_eq!(&bw_stream[..needed], &red_stream[..]);
    assert_eq!(&bw_stream[needed..], &red_stream[..]);
}

#[test]
fn busy_progress_wait_yields_and_completes_when_busy_drops() {
    let mut epd = epd_ready();
    epd.port_mut().busy_ms = Some(50);
    let rs = align_partial_region(Rotation::Deg270, 0, 0, 16, 16).unwrap();
    let needed = (rs.pw as usize / 8) * (rs.ph as usize);
    let snapshot_buf = vec![0xAA; needed];
    let mut snapshot = Snapshot::new(&snapshot_buf, &rs).unwrap();

    epd.begin_partial_refresh_state(&rs, &mut snapshot).unwrap();

    // Pending state asserted
    assert!(epd.has_pending());
    assert!(!epd.is_initialized(), "cannot appear healthy while pending");
    assert!(!epd.is_idle(), "cannot appear idle while pending");

    let mut yields = 0;
    while epd.check_busy().unwrap() {
        yields += 1;
        epd.port_mut().now += 10;
    }

    assert!(yields >= 5, "polled and yielded multiple times: {}", yields);
    assert!(!epd.port_mut().is_busy());

    epd.finish_partial_refresh(&mut snapshot).unwrap();
    assert!(!epd.has_pending());
    assert!(epd.is_initialized());
    assert!(epd.is_idle());
}

#[test]
fn timeout_recovery_controller_marks_failure_and_recovers_via_reinit_retry() {
    let mut epd = epd_ready();
    epd.port_mut().busy_ms = None; // BUSY never clears
    epd.set_busy_timeout_ms(100);

    let rs = align_partial_region(Rotation::Deg270, 0, 0, 16, 16).unwrap();
    let needed = (rs.pw as usize / 8) * (rs.ph as usize);
    let snapshot_buf = vec![0xBB; needed];
    let mut snapshot = Snapshot::new(&snapshot_buf, &rs).unwrap();

    epd.begin_partial_refresh_state(&rs, &mut snapshot).unwrap();

    // Advance clock past timeout
    epd.port_mut().now += 150;

    let res = epd.check_busy();
    assert_eq!(res, Err(DisplayError::BusyTimeout));

    // Controller marked failure and invalid
    assert!(!epd.is_initialized());
    assert!(!epd.has_pending());
    assert!(epd.needs_initial_refresh());

    // Subsequent refresh is rejected
    assert_eq!(
        epd.begin_partial_refresh(0, 0, 16, 16, &mut snapshot),
        Err(DisplayError::NotInitialized)
    );

    // Reinit recovery
    epd.port_mut().busy_ms = Some(5);
    epd.port_mut().busy_until = epd.port_mut().now;
    epd.init(DisplayReset::Software).unwrap();
    assert!(epd.is_initialized());
    assert!(epd.needs_initial_refresh());

    // Next refresh requests full refresh
    assert_eq!(
        epd.begin_partial_refresh(0, 0, 16, 16, &mut snapshot),
        Ok(PartialBegin::NeedsFull)
    );

    // Full refresh succeeds and clears needs_initial_refresh
    let mut blank = BlankStrips;
    epd.full_refresh(&mut blank).unwrap();
    assert!(!epd.needs_initial_refresh());

    // Now partial refresh begins normally
    assert!(matches!(
        epd.begin_partial_refresh(0, 0, 16, 16, &mut snapshot),
        Ok(PartialBegin::Started(_))
    ));
}

#[test]
fn no_stale_publication_dropped_or_deferred_state_does_not_expose_stale_pixels() {
    let mut epd = epd_ready();
    let rs = align_partial_region(Rotation::Deg270, 0, 0, 16, 16).unwrap();
    let needed = (rs.pw as usize / 8) * (rs.ph as usize);
    let snapshot_buf = vec![0x11; needed];
    let mut snapshot = Snapshot::new(&snapshot_buf, &rs).unwrap();

    epd.begin_partial_refresh_state(&rs, &mut snapshot).unwrap();

    // Dropping future: do NOT finish
    assert!(epd.has_pending());
    assert!(!epd.is_initialized());
    assert!(!epd.is_idle());

    // Attempting another refresh fails and refuses to proceed with unsynchronized controller
    let fresh_snapshot_buf = vec![0x22; needed];
    let mut fresh_snapshot = Snapshot::new(&fresh_snapshot_buf, &rs).unwrap();

    let err = epd.begin_partial_refresh(0, 0, 16, 16, &mut fresh_snapshot);
    assert_eq!(err, Err(DisplayError::NotInitialized));

    // Fresh pixels 0x22 were NEVER written to RAM
    let log = &epd.port_mut().log;
    let red_stream = ram_stream(log, cmd::WRITE_RAM_RED);
    assert!(red_stream.is_empty(), "RED RAM was not modified");
    let bw_stream = ram_stream(log, cmd::WRITE_RAM_BW);
    assert!(
        !bw_stream.contains(&0x22),
        "Stale pixels were not published"
    );
}

#[test]
fn async_partial_failure_keeps_the_same_frame_retry_budget() {
    // The asynchronous scheduler cannot run on the host. Pin its adapter to
    // the shared, tested final-attempt policy rather than restarting a frame.
    let scheduler = include_str!("../../kernel/src/kernel/scheduler_c61.rs");
    let recovery = scheduler
        .split_once("async fn render_partial")
        .expect("asynchronous partial renderer")
        .1
        .split_once("fn render_full")
        .expect("full renderer")
        .0
        .rsplit_once("self.epd.abort_pending();")
        .expect("partial failure recovery")
        .1
        .split_once("if let Some(transition) = deferred_transition")
        .expect("transitions stay deferred through recovery")
        .0;
    assert!(recovery.contains("retry_refresh("));
    assert!(!recovery.contains("self.render_full("));
    assert!(!recovery.contains("run_refresh("));
    let reinit = recovery.find("if reinit_display(").unwrap();
    let prepare = recovery.find("app.prepare_render(").unwrap();
    let retry = recovery.find("retry_refresh(").unwrap();
    assert!(reinit < prepare && prepare < retry);
    assert!(recovery.contains("self.partial_refreshes = 0;"));
    assert!(recovery.contains("self.hw.display.on_failure(1);"));
}

// Runs real driver operations: an initial asynchronous partial BUSY timeout,
// controller re-init and a full retry. A hypothetical third refresh succeeds,
// making an accidentally restarted retry budget observable.
struct PartialRecovery {
    epd: Epd<FakePort>,
    full_ok: bool,
    reinit_ok: bool,
    refreshes: usize,
    reinits: usize,
}

impl PartialRecovery {
    fn new(full_ok: bool, reinit_ok: bool) -> Self {
        let mut epd = epd_ready();
        epd.set_busy_timeout_ms(100);
        Self {
            epd,
            full_ok,
            reinit_ok,
            refreshes: 0,
            reinits: 0,
        }
    }
}

impl Refresher for PartialRecovery {
    fn refresh(&mut self) -> bool {
        self.refreshes += 1;
        if self.refreshes == 1 {
            self.epd.port_mut().busy_ms = None;
            let rs = align_partial_region(Rotation::Deg270, 0, 0, 16, 16).unwrap();
            let pixels = [0xAA; 32];
            let mut snapshot = Snapshot::new(&pixels, &rs).unwrap();
            self.epd
                .begin_partial_refresh_state(&rs, &mut snapshot)
                .unwrap();
            self.epd.port_mut().now += 150;
            assert_eq!(self.epd.check_busy(), Err(DisplayError::BusyTimeout));
            self.epd.abort_pending();
            false
        } else {
            assert!(
                self.epd.needs_initial_refresh(),
                "retry must be full after re-init"
            );
            self.epd.port_mut().busy_ms = if self.full_ok || self.refreshes > 2 {
                Some(5)
            } else {
                None
            };
            self.epd.full_refresh(&mut BlankStrips).is_ok()
        }
    }

    fn reinit(&mut self) -> bool {
        self.reinits += 1;
        let port = self.epd.port_mut();
        port.busy_until = port.now;
        port.busy_ms = if self.reinit_ok { Some(5) } else { None };
        self.epd.init(DisplayReset::Software).is_ok()
    }
}

#[test]
fn partial_timeout_then_full_failure_uses_two_attempts_and_fails_one_frame() {
    let mut health = DisplayHealth::new();
    let mut r = PartialRecovery::new(false, true);
    assert!(!run_refresh(&mut health, &mut r));
    assert_eq!((r.refreshes, r.reinits), (2, 1));
    assert!(health.is_stale());
    assert_eq!(health.failed_frames(), 1);
    assert!(!r.epd.is_initialized());
}

#[test]
fn asynchronous_partial_timeout_then_full_failure_does_not_restart_recovery() {
    let mut health = DisplayHealth::new();
    let mut r = PartialRecovery::new(false, true);
    assert!(!r.refresh()); // asynchronous attempt already completed
    health.on_failure(0);
    assert!(r.reinit());
    assert!(!retry_refresh(&mut health, &mut r));
    assert_eq!((r.refreshes, r.reinits), (2, 1));
    assert!(health.is_stale());
    assert_eq!(health.failed_frames(), 1);
    assert!(!r.epd.is_initialized());
}

#[test]
fn partial_timeout_then_full_success_clears_stale_without_failing_another_frame() {
    let mut health = DisplayHealth::new();
    health.on_failure(1);
    let mut r = PartialRecovery::new(true, true);
    assert!(run_refresh(&mut health, &mut r));
    assert_eq!((r.refreshes, r.reinits), (2, 1));
    assert!(!health.is_stale());
    assert_eq!(health.failed_frames(), 1);
    assert!(r.epd.is_initialized());
    assert!(!r.epd.needs_initial_refresh());
}

#[test]
fn asynchronous_partial_timeout_then_full_success_clears_stale() {
    let mut health = DisplayHealth::new();
    health.on_failure(1);
    let mut r = PartialRecovery::new(true, true);
    assert!(!r.refresh());
    health.on_failure(0);
    assert!(r.reinit());
    assert!(retry_refresh(&mut health, &mut r));
    assert_eq!((r.refreshes, r.reinits), (2, 1));
    assert!(!health.is_stale());
    assert_eq!(health.failed_frames(), 1);
    assert!(r.epd.is_initialized());
    assert!(!r.epd.needs_initial_refresh());
}

#[test]
fn partial_timeout_then_failed_reinit_gives_up_without_full_retry() {
    let mut health = DisplayHealth::new();
    let mut r = PartialRecovery::new(true, false);
    assert!(!run_refresh(&mut health, &mut r));
    assert_eq!((r.refreshes, r.reinits), (1, 1));
    assert!(health.is_stale());
    assert_eq!(health.failed_frames(), 1);
    assert!(!r.epd.is_initialized());
}

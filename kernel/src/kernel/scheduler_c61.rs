// OnePage C61 half of the scheduler: boot, render, card detect, battery
// and deep-sleep entry. The event loop itself (scheduler.rs `run`,
// `handle_input`, `poll_housekeeping`) is shared with the X4.
//
// What differs from the X4 on purpose:
//   * every refresh is a FULL refresh unless the `partial-refresh` feature is
//     on (`cargo build-c61-partial`, off by default).
//     Without it `Redraw::Partial` is promoted, so the panel flashes on each
//     page turn, and `ghost_clear_every` has no effect. With it a
//     `Redraw::Partial(region)` is a blocking differential refresh of that
//     region (`Epd::partial_refresh`, ~0.5 s), and every `ghost_clear_every`
//     partials a full refresh clears the ghosting. The first frame, and the
//     first one after a controller re-init, are always full;
//   * `Epd::full_refresh` blocks until the controller is idle (BUSY bounded at
//     5 s). There is no `busy_wait_with_background`, so no input is collected
//     and no background work runs during a refresh; events queue in
//     `INPUT_EVENTS` and are served right after;
//   * a failed refresh follows `pulp_board_logic::lifecycle::DisplayHealth`
//     (re-init and retry once, then give up for this frame and redraw on the
//     next input). The panel cannot show its own error, so there is no screen
//     for it: the failure is in the log;
//   * the session is on the SD card (two slot files) and goes through
//     `board_c61::sleep::enter_deep_sleep`, never through `begin_shutdown` /
//     `cut_peripheral_power` directly;
//   * SD card detect (GPIO28) and the battery monitor (GPIO5/10) are polled
//     here, on the main loop, because the sleep sequence needs `&mut` access to
//     the same parts (see `board_c61::hw`).

use embassy_time::Instant;
use log::{error, info, warn};

#[cfg(feature = "partial-refresh")]
use crate::kernel::bigbuf::{BigBuf, BufClass};
#[cfg(feature = "partial-refresh")]
use pulp_board_logic::lifecycle::retry_refresh;
use pulp_board_logic::lifecycle::{
    DisplayHealth, PostRestore, Refresher, RestoreEnv, check_restorable, post_restore, run_refresh,
};
use pulp_board_logic::power::PeripheralPower;
use pulp_board_logic::session::SessionState;
use pulp_board_logic::sleep::BootPlan;
use pulp_board_logic::ssd1677::{FullKind, Rotation, StripSource};
#[cfg(feature = "partial-refresh")]
use pulp_board_logic::ssd1677::{
    PartialResult, Snapshot, WindowSource, align_partial_region, capture_snapshot,
};
use pulp_board_logic::wallpaper;

use super::app::{AppLayer, Redraw};
use super::sleep_wallpaper;
use super::tasks;
use crate::board_c61::epd::{Epd, FnStrips};
use crate::board_c61::power::Gpio27Rail;
use crate::board_c61::sd::{self, CardEvent};
use crate::board_c61::session::{self, SdSessionStore};
use crate::board_c61::sleep::{self, StoreSaver, WakeCause};
use crate::drivers::sdcard::SdStorage;
use crate::drivers::storage;
use crate::drivers::strip::StripBuffer;
#[cfg(feature = "partial-refresh")]
use crate::ui::Region;

#[inline]
fn now_ms() -> u64 {
    Instant::now().as_millis()
}

// feeds the SSD1677 driver one strip at a time from the app's `draw`
struct AppStrips<'a, A: AppLayer> {
    strip: &'a mut StripBuffer,
    app: &'a A,
}

impl<A: AppLayer> StripSource for AppStrips<'_, A> {
    fn render_strip(&mut self, rotation: Rotation, idx: u16) -> &[u8] {
        self.strip.begin_strip(rotation, idx);
        self.app.draw(self.strip);
        self.strip.data()
    }
}

// the same draw callback, one window of the partial region at a time
#[cfg(feature = "partial-refresh")]
impl<A: AppLayer> WindowSource for AppStrips<'_, A> {
    fn render_window(&mut self, rotation: Rotation, px: u16, py: u16, pw: u16, rows: u16) -> &[u8] {
        self.strip.begin_window(rotation, px, py, pw, rows);
        self.app.draw(self.strip);
        self.strip.data()
    }
}

// the sleep screen, same text and position as the X4
fn draw_sleep_screen(strip: &mut StripBuffer) {
    use embedded_graphics::mono_font::MonoTextStyle;
    use embedded_graphics::mono_font::ascii::FONT_9X18;
    use embedded_graphics::pixelcolor::BinaryColor;
    use embedded_graphics::prelude::*;
    use embedded_graphics::text::Text;

    let style = MonoTextStyle::new(&FONT_9X18, BinaryColor::On);
    let _ = Text::new("(sleep)", Point::new(210, 400), style).draw(strip);
}

// controller init through the GPIO27 policy (software reset only); false when
// it failed (the state machine refused, or BUSY / bus error). Free functions
// over the individual fields so a strip source can borrow `Kernel::strip` at
// the same time.
fn reinit_display(epd: &mut Epd, power: &mut PeripheralPower<Gpio27Rail>) -> bool {
    let reset = match power.display_reset() {
        Ok(r) => r,
        Err(e) => {
            error!("display: reset refused: {:?}", e);
            return false;
        }
    };
    match epd.init(reset) {
        Ok(()) => true,
        Err(e) => {
            error!("display: init failed: {}", e.as_str());
            false
        }
    }
}

// Hardware side of one refresh attempt for `run_refresh`: the driver, the GPIO27
// policy (for the re-init token) and the strip source, borrowed as separate
// fields so the source can read `Kernel::strip`.
struct EpdRefresher<'a, S: StripSource> {
    epd: &'a mut Epd,
    power: &'a mut PeripheralPower<Gpio27Rail>,
    src: &'a mut S,
    kind: FullKind,
}

impl<S: StripSource> Refresher for EpdRefresher<'_, S> {
    fn refresh(&mut self) -> bool {
        match self.epd.full_refresh_kind(self.src, self.kind) {
            Ok(()) => true,
            Err(e) => {
                warn!("display: full refresh failed: {}", e.as_str());
                false
            }
        }
    }

    fn reinit(&mut self) -> bool {
        reinit_display(self.epd, self.power)
    }
}

// Hardware side of one partial-refresh frame for `run_refresh`: a differential
// refresh of `region`, or a full refresh when the controller cannot do one yet
// (first frame, or after a re-init: `Epd::init` makes the next frame full, so
// the retry that follows a failed partial is a full refresh).
#[cfg(feature = "partial-refresh")]
struct EpdPartialRefresher<'a, S: StripSource + WindowSource> {
    epd: &'a mut Epd,
    power: &'a mut PeripheralPower<Gpio27Rail>,
    src: &'a mut S,
    region: Region,
    // the attempt that showed the frame was a full refresh
    full: bool,
}

#[cfg(feature = "partial-refresh")]
impl<S: StripSource + WindowSource> Refresher for EpdPartialRefresher<'_, S> {
    fn refresh(&mut self) -> bool {
        let Region { x, y, w, h } = self.region;
        let shown = match self.epd.partial_refresh(x, y, w, h, self.src) {
            Ok(PartialResult::Done) => {
                self.full = false;
                Ok(())
            }
            Ok(PartialResult::NeedsFull) => {
                self.full = true;
                self.epd.full_refresh(self.src)
            }
            Err(e) => Err(e),
        };
        match shown {
            Ok(()) => true,
            Err(e) => {
                warn!("display: partial refresh failed: {}", e.as_str());
                false
            }
        }
    }

    fn reinit(&mut self) -> bool {
        reinit_display(self.epd, self.power)
    }
}

// Run one frame through `src` with the recovery policy. true = the panel shows
// the new frame; false = given up (logged), redrawn on the next input.
fn refresh_with_recovery<S: StripSource>(
    epd: &mut Epd,
    power: &mut PeripheralPower<Gpio27Rail>,
    health: &mut DisplayHealth,
    src: &mut S,
    kind: FullKind,
) -> bool {
    let shown = run_refresh(
        health,
        &mut EpdRefresher {
            epd,
            power,
            src,
            kind,
        },
    );
    if !shown {
        error!("display: giving up on this frame, redraw on next input");
    }
    shown
}

impl super::Kernel {
    // one-time boot: load caches and settings, restore the saved position if
    // there is a valid and applicable one, render the first screen
    pub async fn boot<A: AppLayer>(&mut self, app_mgr: &mut A, wake: WakeCause) {
        self.bm_cache.ensure_loaded(&self.sd);

        // Decide before anything else touches the session files
        let plan = sleep::plan_boot(wake, session::restore_session(&self.hw.power, &self.sd));

        {
            let mut handle = self.handle();
            app_mgr.load_eager_settings(&mut handle);
            app_mgr.load_initial_state(&mut handle);
        }

        tasks::set_idle_timeout(app_mgr.system_settings().sleep_timeout);
        self.log_stats();

        let restored = match plan {
            BootPlan::Restore { cause, restored } => {
                info!(
                    "boot: session restore (cause {:?}, slot {:?} seq {})",
                    cause, restored.slot, restored.seq
                );
                self.restore_session(app_mgr, &restored.state)
            }
            BootPlan::Normal { cause, reason } => {
                info!("boot: normal boot (cause {:?}, {:?})", cause, reason);
                false
            }
        };

        if !restored {
            app_mgr.enter_initial(&mut self.handle());
        }

        if !reinit_display(&mut self.epd, &mut self.hw.power) {
            // not fatal: the first render re-initialises again
            self.hw.display.on_failure(1);
        }
        self.render_full(app_mgr, false);
        let _ = app_mgr.take_redraw();

        info!("ui ready.");
    }

    // A validated record is applied if this firmware and this card can show
    // it; the record is kept after a successful apply and deleted when it
    // cannot be applied, so bad data cannot loop at boot
    fn restore_session<A: AppLayer>(&mut self, app_mgr: &mut A, state: &SessionState) -> bool {
        let env = RestoreEnv {
            // Upload is active only inside the special-mode call (the scheduler
            // pops it right after), so a saved session never contains it; the
            // app layer's id mapper has no Upload entry in either build.
            upload_available: false,
        };
        let sd = &self.sd;
        let applicable = check_restorable(state, env, |name| {
            core::str::from_utf8(name)
                .ok()
                .is_some_and(|n| storage::file_size(sd, n).is_ok())
        });
        let applied = match applicable {
            Ok(()) => {
                self.hw.wake_count = state.wake_count;
                app_mgr.apply_session(state, &mut self.handle())
            }
            Err(why) => {
                warn!("boot: saved session not applicable: {:?}", why);
                false
            }
        };
        match post_restore(applied) {
            PostRestore::Keep => {}
            PostRestore::ClearAndBootNormally => {
                warn!("boot: deleting the saved session, booting to Home");
                if let Err(e) = session::clear_session(&self.hw.power, &self.sd) {
                    warn!("boot: clearing the saved session failed: {:?}", e);
                }
            }
        }
        applied
    }

    // full refresh, or a partial one with the `partial-refresh` feature;
    // returns false (never "sleep requested": the C61 has no Power key)
    pub(super) async fn render<A: AppLayer>(&mut self, app_mgr: &mut A, redraw: Redraw) -> bool {
        if matches!(redraw, Redraw::None) {
            return false;
        }
        self.log_stats();
        #[cfg(feature = "partial-refresh")]
        if let Redraw::Partial(region) = redraw {
            if self.partial_refreshes < app_mgr.ghost_clear_every() {
                self.render_partial(app_mgr, region).await;
                embassy_futures::yield_now().await;
                return false;
            }
            info!("display: promoted partial to full (ghosting clear)");
        }
        // a request for a clean screen, or a partial promoted by the ghost-clear
        // count, wants the real-temperature waveform; any other full redraw
        // (first frame, app change) only paints
        let needs_clean =
            app_mgr.ctx_mut().take_clean_refresh() || matches!(redraw, Redraw::Partial(_));
        self.render_full(app_mgr, needs_clean);
        self.partial_refreshes = 0;
        // the refresh blocked the executor: let the input / housekeeping tasks
        // run before the loop continues
        embassy_futures::yield_now().await;
        false
    }

    // blocking differential refresh of `region`; counts toward the ghosting
    // clear unless the controller needed a full refresh instead
    #[cfg(feature = "partial-refresh")]
    async fn render_partial<A: AppLayer>(&mut self, app: &mut A, region: Region) {
        if self.epd.needs_initial_refresh() {
            self.render_full(app, true);
            self.partial_refreshes = 0;
            return;
        }

        app.prepare_render(&mut self.handle());
        let started = now_ms();
        let Region { x, y, w, h } = region;
        let Some(rs) = align_partial_region(self.epd.rotation(), x, y, w, h) else {
            return;
        };

        let needed_bytes = (rs.pw as usize / 8) * (rs.ph as usize);
        let snapshot_buf = BigBuf::zeroed(BufClass::DisplayFrame, needed_bytes);

        match snapshot_buf {
            Ok(mut buf) => {
                let mut src = AppStrips {
                    strip: &mut *self.strip,
                    app,
                };
                if let Err(e) = capture_snapshot(&mut src, self.epd.rotation(), &rs, &mut buf) {
                    warn!("display: capture snapshot failed: {}", e.as_str());
                    self.render_full(app, true);
                    self.partial_refreshes = 0;
                    return;
                }
                let mut snapshot = match Snapshot::new(&buf, &rs) {
                    Some(s) => s,
                    None => {
                        self.render_full(app, true);
                        self.partial_refreshes = 0;
                        return;
                    }
                };

                let begin = self.epd.begin_partial_refresh_state(&rs, &mut snapshot);
                let started_ok = match begin {
                    Ok(()) => true,
                    Err(e) => {
                        warn!("display: begin partial failed: {}", e.as_str());
                        false
                    }
                };

                let mut deferred_transition: Option<super::app::Transition<A::Id>> = None;
                let mut success = false;

                if started_ok {
                    let mut prefetched = false;
                    let timeout_ms = self.epd.busy_timeout_ms() as u64;
                    let t0 = now_ms();

                    loop {
                        match self.epd.check_busy() {
                            Ok(false) => {
                                success = true;
                                break;
                            }
                            Ok(true) => {}
                            Err(e) => {
                                warn!("display: busy wait failed: {}", e.as_str());
                                break;
                            }
                        }

                        if now_ms().saturating_sub(t0) >= timeout_ms {
                            warn!("display: busy wait timed out");
                            self.epd.abort_pending();
                            break;
                        }

                        if !prefetched {
                            let mut handle = self.handle();
                            app.prefetch(&mut handle);
                            prefetched = true;
                        }

                        let hw_ev = match embassy_time::with_timeout(
                            embassy_time::Duration::from_millis(super::timing::TICK_MS),
                            tasks::INPUT_EVENTS.receive(),
                        )
                        .await
                        {
                            Ok(ev) => Some(ev),
                            Err(_) => None,
                        };

                        if let Some(ev) = hw_ev {
                            let _ = tasks::IDLE_SLEEP_DUE.try_take();
                            let suppressed = app.suppress_deferred_input();
                            if !suppressed && deferred_transition.is_none() {
                                let t = app.dispatch_event(ev, &mut *self.bm_cache);
                                if t != super::app::Transition::None {
                                    deferred_transition = Some(t);
                                    tasks::request_hold_reset();
                                }
                            }
                        }
                    }

                    if success {
                        if let Err(e) = self.epd.finish_partial_refresh(&mut snapshot) {
                            warn!("display: finish partial refresh failed: {}", e.as_str());
                            success = false;
                        }
                    }
                }

                if success {
                    self.hw.display.on_success();
                    self.partial_refreshes += 1;
                    info!(
                        "display: partial refresh {}x{} at ({}, {}) in {} ms (partial count {})",
                        region.w,
                        region.h,
                        region.x,
                        region.y,
                        now_ms() - started,
                        self.partial_refreshes
                    );
                } else {
                    self.epd.abort_pending();
                    match self.hw.display.on_failure(0) {
                        pulp_board_logic::lifecycle::RefreshStep::ReinitAndRetry => {
                            if reinit_display(&mut self.epd, &mut self.hw.power) {
                                app.prepare_render(&mut self.handle());
                                let mut src = AppStrips {
                                    strip: &mut *self.strip,
                                    app,
                                };
                                // The partial was attempt 0; this full refresh
                                // is the final attempt of the same frame.
                                if !retry_refresh(
                                    &mut self.hw.display,
                                    &mut EpdRefresher {
                                        epd: &mut self.epd,
                                        power: &mut self.hw.power,
                                        src: &mut src,
                                        kind: FullKind::Clean,
                                    },
                                ) {
                                    error!(
                                        "display: giving up on this frame, redraw on next input"
                                    );
                                }
                                self.partial_refreshes = 0;
                            } else {
                                error!("display: reinit failed after error");
                                self.hw.display.on_failure(1);
                            }
                        }
                        pulp_board_logic::lifecycle::RefreshStep::GiveUp => {
                            error!("display: giving up on this frame");
                        }
                    }
                }

                if let Some(transition) = deferred_transition {
                    app.apply_transition(transition, &mut self.handle());
                }
            }
            Err(_) => {
                let mut src = AppStrips {
                    strip: &mut *self.strip,
                    app,
                };
                let mut refresher = EpdPartialRefresher {
                    epd: &mut self.epd,
                    power: &mut self.hw.power,
                    src: &mut src,
                    region,
                    full: false,
                };
                let shown = run_refresh(&mut self.hw.display, &mut refresher);
                let full = refresher.full;
                if !shown {
                    error!("display: giving up on this frame, redraw on next input");
                    return;
                }
                if full {
                    self.partial_refreshes = 0;
                } else {
                    self.partial_refreshes += 1;
                }
                info!(
                    "display: {} refresh {}x{} at ({}, {}) in {} ms (partial count {})",
                    if full { "full" } else { "partial" },
                    region.w,
                    region.h,
                    region.x,
                    region.y,
                    now_ms() - started,
                    self.partial_refreshes
                );
            }
        }
    }

    // `needs_clean`: the refresh exists to clear ghosting. Otherwise it only
    // paints and may run the quick waveform (`fast-full-refresh`; see FullKindPolicy)
    fn render_full<A: AppLayer>(&mut self, app: &mut A, needs_clean: bool) {
        let kind = self
            .hw
            .full_kind
            .next(needs_clean || !cfg!(feature = "fast-full-refresh"));
        app.prepare_render(&mut self.handle());
        let started = now_ms();
        let mut src = AppStrips {
            strip: &mut *self.strip,
            app,
        };
        let shown = refresh_with_recovery(
            &mut self.epd,
            &mut self.hw.power,
            &mut self.hw.display,
            &mut src,
            kind,
        );
        info!(
            "display: full refresh kind {:?} {} in {} ms",
            kind,
            if shown { "ok" } else { "FAILED" },
            now_ms() - started
        );
    }

    // earliest time `poll_card` / `poll_battery` have something to do. A
    // settled card-detect switch is looked at every CD_IDLE_POLL_MS; once the
    // pin disagrees with the settled state the debounce runs at its own
    // CD_SAMPLE_INTERVAL_MS cadence
    pub(super) fn c61_poll_deadline(&self) -> Instant {
        let settled = self.hw.card.pin.state() == self.hw.card.detect.state();
        let card_ms = if settled {
            now_ms() + u64::from(sd::CD_IDLE_POLL_MS)
        } else {
            self.hw.card_due.next_ms()
        };
        let next_ms = card_ms.min(self.hw.battery_due.next_ms());
        Instant::from_millis(next_ms)
    }

    // card detect and the 30 s battery measurement; called every loop
    // iteration, cheap when nothing is due
    pub(super) async fn poll_card<A: AppLayer>(&mut self, app_mgr: &mut A) {
        if !self.hw.card_due.due(now_ms()) {
            return;
        }
        let level = self.hw.card.pin.level_high();
        match self.hw.card.detect.sample(level) {
            Some(CardEvent::Removed) => {
                self.hw.card.health.on_card_event(CardEvent::Removed);
                // dropping the volume discards FAT state: a file that was open
                // for writing is not flushed (nothing can be written anyway)
                let _ = self.hw.power.card_removed();
                self.replace_storage(SdStorage::empty(), false, app_mgr);
                warn!(
                    "sd: card removed ({})",
                    self.hw.card.health.status().message()
                );
            }
            Some(CardEvent::Inserted) => {
                if self.hw.card.health.on_card_event(CardEvent::Inserted) {
                    let up = sd::bring_up(
                        &mut self.hw.power,
                        self.hw.card.pin.state(),
                        self.hw.card.spi.clone(),
                    )
                    .await;
                    if self.hw.card.control.speed_up().is_err() {
                        warn!("sd: could not restore the operating clock");
                    }
                    self.hw.card.health = up.health;
                    let sd_ok = up.storage.probe_ok();
                    if sd_ok && let Err(e) = storage::ensure_pulp_dir_async(&up.storage).await {
                        warn!("sd: _PULP dir: {}", e);
                    }
                    self.replace_storage(up.storage, sd_ok, app_mgr);
                    info!(
                        "sd: card inserted ({})",
                        self.hw.card.health.status().message()
                    );
                }
            }
            None => {}
        }
    }

    // blocking (~30 ms settle + 16 conversions) but only every 30 s; a failed
    // read keeps the previous value (never 0 mV)
    pub(super) fn poll_battery(&mut self) {
        if !self.hw.battery_due.due(now_ms()) {
            return;
        }
        let Some(monitor) = self.hw.battery.as_mut() else {
            return;
        };
        match monitor.measure() {
            Ok(r) => {
                self.cached_battery_mv = r.cell_mv;
                info!(
                    "battery: cell {} mV, {}% (periodic)",
                    r.cell_mv,
                    r.percent()
                );
            }
            Err(e) => warn!("battery: sample failed: {:?} (charging resumed)", e),
        }
    }

    // the 4 gray wallpaper through the gray waveform (~4 s). A fresh controller
    // init first, so nothing of the last black-and-white frame is assumed; false
    // when anything failed, and the caller draws the text screen instead (its
    // full refresh reloads the black-and-white waveform from OTP)
    #[cfg(feature = "gray-wallpaper")]
    fn show_gray_wallpaper(&mut self, image: &sleep_wallpaper::GrayWallpaper) -> bool {
        if !reinit_display(&mut self.epd, &mut self.hw.power) {
            return false;
        }
        let rotation = self.epd.rotation();
        let mut src = sleep_wallpaper::GrayStrips {
            strip: &mut *self.strip,
            image,
        };
        match pulp_board_logic::gray::show_planes(self.epd.port_mut(), rotation, &mut src) {
            Ok(t) => {
                info!(
                    "sleep: 4 gray wallpaper shown (planes {} ms, update {} ms)",
                    t.write_ms, t.update_ms
                );
                true
            }
            Err(e) => {
                warn!("sleep: gray wallpaper failed: {}", e.as_str());
                false
            }
        }
    }

    // flush, sleep screen, then the deep-sleep sequence. Returns only when the
    // sequence aborted before anything irreversible (wake key still held, rail
    // not in a shutdown-able state): the device is then fully usable, so the
    // frame is redrawn and the idle timer restarts
    pub(super) async fn sleep_with_session<A: AppLayer>(&mut self, app_mgr: &mut A, reason: &str) {
        info!("{}: entering sleep...", reason);

        // the wake level is already present while the key is down: the sequence
        // would refuse to arm it, after the sleep screen had been drawn
        if self.hw.sleep.wake.is_pressed() {
            warn!("sleep: WAKE key is down, staying awake");
            tasks::IDLE_RESET.signal(());
            return;
        }

        if self.bm_cache.is_dirty() {
            self.bm_cache.flush(&self.sd);
        }

        let mut state = SessionState::home();
        app_mgr.collect_session(&mut state);
        state.wake_count = self.hw.wake_count.wrapping_add(1);

        // wallpaper (SLEEP.BMP) decoded now, while the SD card is still idle
        // and before the panel starts; None draws the text screen
        #[cfg(not(feature = "gray-wallpaper"))]
        let wallpaper = sleep_wallpaper::load(&self.sd);
        #[cfg(feature = "gray-wallpaper")]
        let gray_wallpaper = sleep_wallpaper::load_gray(&self.sd);
        #[cfg(feature = "gray-wallpaper")]
        let gray_shown = gray_wallpaper
            .as_ref()
            .is_some_and(|w| self.show_gray_wallpaper(w));
        #[cfg(not(feature = "gray-wallpaper"))]
        let gray_shown = false;

        // sleep screen (best effort; a failed refresh must not block sleeping)
        if !gray_shown {
            #[cfg(not(feature = "gray-wallpaper"))]
            let image = wallpaper.as_ref().map(|w| w.image());
            #[cfg(feature = "gray-wallpaper")]
            let image: Option<&[u8]> = None;
            let mut src = FnStrips {
                strip: &mut *self.strip,
                draw: |strip: &mut StripBuffer| match image {
                    Some(img) => wallpaper::draw_strip(strip, img),
                    None => draw_sleep_screen(strip),
                },
            };
            let _ = refresh_with_recovery(
                &mut self.epd,
                &mut self.hw.power,
                &mut self.hw.display,
                &mut src,
                FullKind::Clean,
            );
        }
        #[cfg(not(feature = "gray-wallpaper"))]
        drop(wallpaper);
        #[cfg(feature = "gray-wallpaper")]
        drop(gray_wallpaper);

        let mut store = SdSessionStore::new(&self.sd);
        let hw = &mut self.hw;
        let abort = sleep::enter_deep_sleep(
            &mut hw.power,
            StoreSaver {
                store: &mut store,
                state: &state,
            },
            &mut self.epd,
            &self.sd,
            &mut hw.battery,
            &mut hw.sleep.lines,
            &mut hw.sleep.wake,
            &mut hw.sleep.entry,
        );

        // still awake
        warn!("sleep: not entered ({:?}), continuing", abort.reason);
        tasks::IDLE_RESET.signal(());
        app_mgr.request_full_redraw();
    }
}

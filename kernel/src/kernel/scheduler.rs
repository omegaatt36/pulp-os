// scheduler: main event loop, render pipeline, housekeeping, sleep
//
// EPD and SD share a single SPI bus via CriticalSectionDevice;
// during normal operation, all SD I/O completes before render()
// touches the EPD; during the DU/GC waveform (~400ms), the EPD
// charge pump drives pixels with no SPI commands, so the bus is
// free for SD I/O - busy_wait_with_background exploits this
// window to run background caching and housekeeping
//
// handle_input and poll_housekeeping are synchronous; they return
// a bool flag when the caller should enter_sleep (which is async
// because it renders a sleep screen via the EPD)
//
// sd_card_sleep sends cmd0 before deep sleep to reduce sd card
// idle current from ~150 uA to ~10 uA

use embassy_futures::select::{Either, Either3, select, select3};
use embassy_time::{Duration, Instant, Ticker, Timer};
use log::info;

use super::app::{AppLayer, Transition};
use crate::drivers::battery;
use crate::drivers::input::Event;
use crate::kernel::tasks;

use crate::ui::{free_stack_bytes, stack_high_water_mark};

use super::timing;

impl super::Kernel {
    // event-driven main loop; never returns
    //
    // two genuine async suspension points in steady state:
    //   1. the wait at the top of the loop: a TICK_MS ticker while the app
    //      layer has background work pending, otherwise a park until input,
    //      a task wake-up or the next deadline (see `deadline`)
    //   2. EPD busy pin wait inside render()
    // everything between them is synchronous function calls
    pub async fn run<A: AppLayer>(&mut self, app_mgr: &mut A) -> ! {
        // Some only while polling; a ticker left over from before a park
        // would fire for every period it missed
        let mut work_ticker: Option<Ticker> = None;

        loop {
            if app_mgr.needs_special_mode() {
                self.handle_special_mode(app_mgr).await;
                continue;
            }

            // async point 1: wait for input, tick or deadline
            let hw_event = if app_mgr.background_pending() || app_mgr.ctx_mut().render_ready() {
                let ticker = work_ticker
                    .get_or_insert_with(|| Ticker::every(Duration::from_millis(timing::TICK_MS)));
                match select(tasks::INPUT_EVENTS.receive(), ticker.next()).await {
                    Either::First(ev) => Some(ev),
                    Either::Second(_) => None,
                }
            } else {
                work_ticker = None;
                let at = self.deadline(app_mgr);
                match select3(
                    tasks::INPUT_EVENTS.receive(),
                    tasks::LOOP_WAKE.wait(),
                    Timer::at(at),
                )
                .await
                {
                    Either3::First(ev) => Some(ev),
                    Either3::Second(()) | Either3::Third(()) => None,
                }
            };

            if let Some(ev) = hw_event {
                if matches!(ev, Event::LongPress(_)) {
                    info!("scheduler: received {:?}", ev);
                }
                if self.handle_input(ev, app_mgr) {
                    self.sleep_with_session(app_mgr, "power held").await;
                    continue;
                }
            }

            if app_mgr.needs_special_mode() {
                continue;
            }

            // SPI bus sharing invariant
            //
            // the EPD and SD card share a single SPI2 bus via
            // CriticalSectionDevice (RefCell under the hood).
            //
            //   1. background SD I/O runs here, before EPD access;
            //      interruptible by input so the user can navigate
            //      away during long-running caching operations
            //   2. poll_housekeeping may do SD I/O, also before render
            //   3. render() touches the EPD; during the waveform window
            //      busy_wait_with_background runs SD I/O because the
            //      EPD charge pump is driving pixels with no SPI commands
            //   4. prepare_render may read font packs immediately before
            //      each draw; no other SD I/O happens outside these sites
            //   5. a long CJK glyph preparation (fonts::cjk) is cut into
            //      slices, one per run_background call. A slice is plain
            //      synchronous code that reads the pack inside one
            //      `with_pulp_subdir_file` call. At every suspension point
            //      (the yield that ends it, the select above, render), the
            //      storage borrow and SD transactions have ended. Storage
            //      may retain the RawFile for reuse; it holds no SPI bus
            //      borrow. A panel refresh therefore never overlaps a read:
            //      `render` blocks until the controller is idle before the
            //      next slice can start
            //
            // when input arrives during run_background, the background
            // future is dropped. this is safe: partial chapter cache
            // writes leave ch_cached=false so the chapter is recached
            // on the next attempt
            let bg_input = {
                let mut handle = self.handle();
                match select(
                    app_mgr.run_background(&mut handle),
                    tasks::INPUT_EVENTS.receive(),
                )
                .await
                {
                    Either::First(()) => None,
                    Either::Second(ev) => Some(ev),
                }
            };

            if let Some(ev) = bg_input {
                if self.handle_input(ev, app_mgr) {
                    self.sleep_with_session(app_mgr, "power held").await;
                    continue;
                }

                if app_mgr.needs_special_mode() {
                    continue;
                }
            }

            self.poll_card(app_mgr).await;

            // the WAKE key was held and released (see tasks::SLEEP_REQUESTED)
            if tasks::SLEEP_REQUESTED.try_take().is_some() {
                self.sleep_with_session(app_mgr, "wake key held").await;
                continue;
            }

            if self.poll_housekeeping(app_mgr) {
                self.sleep_with_session(app_mgr, "idle timeout").await;
                continue;
            }

            if app_mgr.ctx_mut().render_ready() {
                let redraw = app_mgr.take_redraw();
                if self.render(app_mgr, redraw).await {
                    self.sleep_with_session(app_mgr, "power held").await;
                    continue;
                }
            }
        }
    }

    // when a parked loop must run again without input or a task wake-up: the
    // end of a redraw coalescing window, and on the C61 the card-detect and
    // battery polls (those are owned by this loop, see scheduler_c61.rs)
    fn deadline<A: AppLayer>(&mut self, app_mgr: &mut A) -> Instant {
        let mut at = Instant::now() + Duration::from_secs(timing::PARK_MAX_SECS);
        if let Some(t) = app_mgr.ctx_mut().render_deadline() {
            at = at.min(t);
        }
        {
            at = at.min(self.c61_poll_deadline());
        }
        at
    }

    // delegate to app layer for modes that bypass normal dispatch
    // (e.g. wifi upload); kernel passes hardware resources through
    async fn handle_special_mode<A: AppLayer>(&mut self, app_mgr: &mut A) {
        app_mgr
            .run_special_mode(&mut self.epd, self.strip, &mut self.delay, &self.sd)
            .await;

        app_mgr.apply_transition(Transition::Pop, &mut self.handle());
        app_mgr.request_full_redraw();
    }

    // returns true if caller should call enter_sleep
    //
    // note: the original async version called enter_sleep inline
    // on power-long-press and then fell through to dispatch_event
    // if sleep_deep somehow returned; this version correctly returns
    // early so the caller can enter_sleep and continue the loop
    fn handle_input<A: AppLayer>(&mut self, hw_event: Event, app_mgr: &mut A) -> bool {
        let _ = tasks::IDLE_SLEEP_DUE.try_take();

        // the previous frame was given up (BUSY timeout / bus error); the
        // app already consumed its redraw request, so ask for a fresh one now
        if self.hw.display.redraw_on_input() {
            app_mgr.request_full_redraw();
        }

        let suppressed_before = app_mgr.suppress_deferred_input();
        let transition = app_mgr.dispatch_event(hw_event, &mut *self.bm_cache);

        if transition != Transition::None {
            app_mgr.apply_transition(transition, &mut self.handle());
            tasks::request_hold_reset();
        } else if app_mgr.suppress_deferred_input() != suppressed_before {
            // quick menu opened/closed
            tasks::request_hold_reset();
        }

        false
    }

    // shared housekeeping body: battery, sd probe, bookmark flush, stats
    fn poll_housekeeping_inner<A: AppLayer>(&mut self, app_mgr: &A) {
        if let Some(mv) = tasks::BATTERY_MV.try_take() {
            self.cached_battery_mv = mv;
        }

        // the battery monitor is owned by the kernel (see board_c61::hw)
        self.poll_battery();

        if tasks::SD_CHECK_DUE.try_take().is_some() {
            self.sd_ok = self.sd.probe_ok();
        }

        if tasks::BOOKMARK_FLUSH_DUE.try_take().is_some() && self.bm_cache.is_dirty() {
            self.bm_cache.flush(&self.sd);
        }

        if tasks::STATUS_DUE.try_take().is_some() {
            self.log_stats();
            if app_mgr.settings_loaded() {
                tasks::set_idle_timeout(app_mgr.system_settings().sleep_timeout);
            }
        }
    }

    fn poll_housekeeping<A: AppLayer>(&mut self, app_mgr: &A) -> bool {
        self.poll_housekeeping_inner(app_mgr);
        tasks::IDLE_SLEEP_DUE.try_take().is_some()
    }

    pub fn log_stats(&self) {
        let stats = esp_alloc::HEAP.stats();
        let bat_pct = battery::battery_percentage(self.cached_battery_mv);
        let uptime = super::uptime_secs();
        let mins = (uptime / 60) % 60;
        let hrs = uptime / 3600;

        info!(
            "stats: heap {}/{}K peak {}K | stack free {}K hwm {}K | bat {}% {}.{}V | up {}:{:02} | SD:{}",
            stats.current_usage / 1024,
            stats.size / 1024,
            stats.max_usage / 1024,
            free_stack_bytes() / 1024,
            stack_high_water_mark() / 1024,
            bat_pct,
            self.cached_battery_mv / 1000,
            (self.cached_battery_mv % 1000) / 100,
            hrs,
            mins,
            if self.sd_ok { "ok" } else { "--" },
        );
        crate::board_c61::memory::log_usage();
    }
}

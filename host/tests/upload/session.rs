// Upload session contract : stage order, cancellation by `back`, failure
// paths, and ownership release / re-entry . Expected values come
// from the requirements and the contract text only.
//
// The fake context `C` models the `Interface::try_station()` singleton: one
// shared ownership token. `acquire` fails with `RadioUnavailable` while the
// token is held; `C::drop` hands it back and records the event order. Stage
// closures hold a counting `Guard` so the tests can tell whether a stage
// future was dropped before `run` returned. Everything runs under
// `embassy_futures::block_on` inside a worker thread guarded by a wall-clock
// timeout, so a regression that never returns fails instead of hanging.

use std::cell::{Cell, RefCell};
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc;
use std::task::{Context, Poll};
use std::thread;
use std::time::{Duration as StdDuration, Instant as StdInstant};

use embassy_futures::block_on;
use embassy_time::{Duration, Timer, with_timeout};
use pulp_host::apps::upload_connect::{ConnectError, Limits};
use pulp_host::apps::upload_session::{self as session, Phase, SessionEnd};

// Outer insurance: far above any scenario below, still bounded.
const WALL_GUARD: StdDuration = StdDuration::from_secs(60);
const INNER_GUARD: Duration = Duration::from_secs(10);
// Generous upper slack on measured elapsed time to absorb CI jitter.
const SLACK: StdDuration = StdDuration::from_millis(1500);

const SSID: &[u8] = b"home";
const PASSWORD: &[u8] = b"correct horse";
const DHCP_VALUE: u32 = 0xC0A8_0102;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn std_ms(n: u64) -> StdDuration {
    StdDuration::from_millis(n)
}

fn limits(associate: u64, dhcp: u64) -> Limits {
    Limits {
        associate: ms(associate),
        dhcp: ms(dhcp),
    }
}

fn bounded<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(WALL_GUARD) {
        Ok(v) => {
            handle.join().unwrap();
            v
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("exceeded the {WALL_GUARD:?} wall-clock guard")
        }
        // sender dropped without a value: the worker panicked; re-raise it
        Err(mpsc::RecvTimeoutError::Disconnected) => match handle.join() {
            Err(p) => std::panic::resume_unwind(p),
            Ok(()) => panic!("worker ended without a result"),
        },
    }
}

// ------------------------------------------------------------- fakes

#[derive(Default)]
struct World {
    events: RefCell<Vec<&'static str>>,
    // the "Interface::try_station()" singleton
    token: Cell<bool>,
    force_acquire_fail: Cell<bool>,
    created: Cell<u32>,
    dropped: Cell<u32>,
    acquire_calls: Cell<u32>,
    associate_calls: Cell<u32>,
    dhcp_calls: Cell<u32>,
    serve_calls: Cell<u32>,
    guards_live: Cell<i32>,
    associate_started: Cell<bool>,
    dhcp_started: Cell<bool>,
    serve_started: Cell<bool>,
    serve_arg: Cell<Option<u32>>,
    back_polls: Cell<u32>,
    stage_polls: Cell<u32>,
}

impl World {
    fn log(&self, ev: &'static str) {
        self.events.borrow_mut().push(ev);
    }

    fn events(&self) -> Vec<&'static str> {
        self.events.borrow().clone()
    }

    // Fresh per-run state; the ownership token and created/dropped totals
    // deliberately carry over so re-entry is exercised.
    fn begin_run(&self) {
        self.events.borrow_mut().clear();
        self.acquire_calls.set(0);
        self.associate_calls.set(0);
        self.dhcp_calls.set(0);
        self.serve_calls.set(0);
        self.associate_started.set(false);
        self.dhcp_started.set(false);
        self.serve_started.set(false);
        self.serve_arg.set(None);
        self.back_polls.set(0);
        self.stage_polls.set(0);
    }

    fn acquire(&self) -> Result<C<'_>, ConnectError> {
        self.acquire_calls.set(self.acquire_calls.get() + 1);
        self.log("acquire");
        if self.force_acquire_fail.get() || self.token.get() {
            return Err(ConnectError::RadioUnavailable);
        }
        self.token.set(true);
        self.created.set(self.created.get() + 1);
        Ok(C { world: self })
    }
}

struct C<'w> {
    world: &'w World,
}

impl Drop for C<'_> {
    fn drop(&mut self) {
        self.world.token.set(false);
        self.world.dropped.set(self.world.dropped.get() + 1);
        self.world.log("C dropped");
    }
}

struct Guard<'w> {
    world: &'w World,
    on_drop: &'static str,
}

impl<'w> Guard<'w> {
    fn new(world: &'w World, on_drop: &'static str) -> Self {
        world.guards_live.set(world.guards_live.get() + 1);
        Self { world, on_drop }
    }
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.world.guards_live.set(self.world.guards_live.get() - 1);
        self.world.log(self.on_drop);
    }
}

// Never completes; counts every poll so "polled after cancellation" is visible.
fn never<'w, T>(world: &'w World) -> impl Future<Output = T> + 'w {
    core::future::poll_fn(move |_| {
        world.stage_polls.set(world.stage_polls.get() + 1);
        Poll::Pending
    })
}

#[derive(Clone, Copy)]
enum BackKind {
    Never,
    Immediate,
    AfterAssociateStarted,
    AfterDhcpStarted,
    AfterServeStarted,
}

struct BackFut<'w> {
    world: &'w World,
    kind: BackKind,
}

impl Future for BackFut<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        let w = self.world;
        w.back_polls.set(w.back_polls.get() + 1);
        let ready = match self.kind {
            BackKind::Never => false,
            BackKind::Immediate => true,
            BackKind::AfterAssociateStarted => w.associate_started.get(),
            BackKind::AfterDhcpStarted => w.dhcp_started.get(),
            BackKind::AfterServeStarted => w.serve_started.get(),
        };
        if ready {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl Drop for BackFut<'_> {
    fn drop(&mut self) {
        self.world.log("back dropped");
    }
}

#[derive(Clone, Copy)]
enum Assoc {
    Ok,
    Err,
    Pending,
}

#[derive(Clone, Copy)]
enum Dh {
    Value(u32),
    Pending,
}

#[derive(Clone, Copy)]
struct Scenario {
    ssid: &'static [u8],
    password: &'static [u8],
    limits: Limits,
    associate: Assoc,
    dhcp: Dh,
    back: BackKind,
}

impl Scenario {
    fn new(associate: Assoc, dhcp: Dh, back: BackKind) -> Self {
        Self {
            ssid: SSID,
            password: PASSWORD,
            limits: Limits::DEFAULT,
            associate,
            dhcp,
            back,
        }
    }
}

struct Outcome {
    end: SessionEnd,
    elapsed: StdDuration,
}

fn exec<'w>(w: &'w World, s: Scenario) -> Outcome {
    w.begin_run();
    let start = StdInstant::now();
    let fut = session::run(
        s.ssid,
        s.password,
        s.limits,
        || w.acquire(),
        async |_c: &mut C<'w>| -> Result<(), &'static str> {
            w.associate_calls.set(w.associate_calls.get() + 1);
            w.associate_started.set(true);
            w.log("associate");
            let _g = Guard::new(w, "associate guard dropped");
            match s.associate {
                Assoc::Ok => Ok(()),
                Assoc::Err => Err("auth failed"),
                Assoc::Pending => never(w).await,
            }
        },
        async |_c: &mut C<'w>| -> u32 {
            w.dhcp_calls.set(w.dhcp_calls.get() + 1);
            w.dhcp_started.set(true);
            w.log("dhcp");
            let _g = Guard::new(w, "dhcp guard dropped");
            match s.dhcp {
                Dh::Value(v) => v,
                Dh::Pending => never(w).await,
            }
        },
        async |_c: &mut C<'w>, t: u32| -> Infallible {
            w.serve_calls.set(w.serve_calls.get() + 1);
            w.serve_arg.set(Some(t));
            w.serve_started.set(true);
            w.log("serve");
            let _g = Guard::new(w, "serve guard dropped");
            never(w).await
        },
        BackFut {
            world: w,
            kind: s.back,
        },
    );
    let end =
        block_on(with_timeout(INNER_GUARD, fut)).expect("run exceeded the inner guard timeout");
    let elapsed = start.elapsed();
    w.log("run returned");
    Outcome { end, elapsed }
}

// invariant after every run: every guard (stage future) and C was
// dropped, in the order stage future -> C -> run returned; token handed back.
fn assert_released(w: &World) {
    let ev = w.events();
    let ret = ev
        .iter()
        .rposition(|e| *e == "run returned")
        .expect("run returned event");
    assert_eq!(ret, ev.len() - 1, "events after run returned: {ev:?}");
    assert_eq!(
        w.guards_live.get(),
        0,
        "stage future alive after run: {ev:?}"
    );
    assert!(
        !w.token.get(),
        "ownership token still held after run: {ev:?}"
    );
    assert_eq!(
        w.created.get(),
        w.dropped.get(),
        "C created/dropped mismatch: {ev:?}"
    );
    let c_dropped = ev.iter().position(|e| *e == "C dropped");
    for (i, e) in ev.iter().enumerate() {
        if e.ends_with("guard dropped") {
            let c = c_dropped.unwrap_or_else(|| panic!("stage ran but C never dropped: {ev:?}"));
            assert!(i < c, "stage future dropped after C: {ev:?}");
        }
    }
    if let Some(c) = c_dropped {
        assert!(c < ret, "C dropped after run returned: {ev:?}");
    }
    let b = ev
        .iter()
        .position(|e| *e == "back dropped")
        .unwrap_or_else(|| panic!("back future not dropped before return: {ev:?}"));
    assert!(b < ret, "back dropped after run returned: {ev:?}");
}

fn assert_calls(w: &World, associate: u32, dhcp: u32, serve: u32) {
    assert_eq!(w.associate_calls.get(), associate, "associate calls");
    assert_eq!(w.dhcp_calls.get(), dhcp, "dhcp calls");
    assert_eq!(w.serve_calls.get(), serve, "serve calls");
}

// Events without the "back dropped" marker (its position relative to the
// other drops is not part of the contract).
fn trace(w: &World) -> Vec<&'static str> {
    w.events()
        .into_iter()
        .filter(|e| *e != "back dropped")
        .collect()
}

// ------------------------------------------------------------- 1. order / data flow

#[test]
fn stages_run_in_order_once_and_dhcp_value_reaches_serve() {
    bounded(|| {
        let w = World::default();
        let out = exec(
            &w,
            Scenario::new(
                Assoc::Ok,
                Dh::Value(DHCP_VALUE),
                BackKind::AfterServeStarted,
            ),
        );
        assert_eq!(out.end, SessionEnd::Exited(Phase::Serving));
        assert_calls(&w, 1, 1, 1);
        assert_eq!(w.acquire_calls.get(), 1);
        assert_eq!(w.serve_arg.get(), Some(DHCP_VALUE));
        assert_eq!(
            trace(&w),
            [
                "acquire",
                "associate",
                "associate guard dropped",
                "dhcp",
                "dhcp guard dropped",
                "serve",
                "serve guard dropped",
                "C dropped",
                "run returned",
            ]
        );
        assert_released(&w);
        assert!(out.elapsed < SLACK, "took {:?}", out.elapsed);
    });
}

// ------------------------------------------------------------- 2. cancellation per phase

#[test]
fn back_while_associating_cancels_and_releases() {
    bounded(|| {
        let w = World::default();
        let out = exec(
            &w,
            Scenario::new(
                Assoc::Pending,
                Dh::Value(1),
                BackKind::AfterAssociateStarted,
            ),
        );
        assert_eq!(out.end, SessionEnd::Exited(Phase::Associating));
        assert_calls(&w, 1, 0, 0);
        assert_eq!(
            trace(&w),
            [
                "acquire",
                "associate",
                "associate guard dropped",
                "C dropped",
                "run returned",
            ]
        );
        assert_released(&w);
        // cancellation must not wait for the (default, 20 s) associate limit
        assert!(out.elapsed < SLACK, "took {:?}", out.elapsed);
    });
}

#[test]
fn back_while_waiting_for_dhcp_cancels_and_releases() {
    bounded(|| {
        let w = World::default();
        let out = exec(
            &w,
            Scenario::new(Assoc::Ok, Dh::Pending, BackKind::AfterDhcpStarted),
        );
        assert_eq!(out.end, SessionEnd::Exited(Phase::Dhcp));
        assert_calls(&w, 1, 1, 0);
        assert_eq!(
            trace(&w),
            [
                "acquire",
                "associate",
                "associate guard dropped",
                "dhcp",
                "dhcp guard dropped",
                "C dropped",
                "run returned",
            ]
        );
        assert_released(&w);
        assert!(out.elapsed < SLACK, "took {:?}", out.elapsed);
    });
}

#[test]
fn back_while_serving_cancels_and_releases() {
    bounded(|| {
        let w = World::default();
        let out = exec(
            &w,
            Scenario::new(
                Assoc::Ok,
                Dh::Value(DHCP_VALUE),
                BackKind::AfterServeStarted,
            ),
        );
        assert_eq!(out.end, SessionEnd::Exited(Phase::Serving));
        assert_calls(&w, 1, 1, 1);
        assert_released(&w);
        assert!(w.events().contains(&"serve guard dropped"));
    });
}

// ------------------------------------------------------------- 3. credentials

fn assert_rejected_before_acquire(
    ssid: &'static [u8],
    password: &'static [u8],
    expected: ConnectError,
) {
    bounded(move || {
        let w = World::default();
        let mut s = Scenario::new(Assoc::Ok, Dh::Value(1), BackKind::Never);
        s.ssid = ssid;
        s.password = password;
        let out = exec(&w, s);
        assert_eq!(out.end, SessionEnd::Failed(expected));
        assert_eq!(w.acquire_calls.get(), 0, "acquire must not be called");
        assert_calls(&w, 0, 0, 0);
        assert_eq!(w.created.get(), 0);
        assert_released(&w);
    });
}

#[test]
fn missing_credentials_fail_before_acquire() {
    // missing credentials -> connection error; acquire never called
    assert_rejected_before_acquire(b"", PASSWORD, ConnectError::MissingCredentials);
    assert_rejected_before_acquire(b"", b"", ConnectError::MissingCredentials);
}

#[test]
fn invalid_credentials_fail_before_acquire() {
    // invalid credentials -> connection error; acquire never called
    assert_rejected_before_acquire(&[b's'; 33], PASSWORD, ConnectError::InvalidCredentials);
    assert_rejected_before_acquire(SSID, b"short", ConnectError::InvalidCredentials);
    assert_rejected_before_acquire(SSID, &[b'p'; 64], ConnectError::InvalidCredentials);
}

#[test]
fn empty_password_fails_before_acquire() {
    // invalid credentials (SSID present, password length 0) -> connection
    // error; acquire never called
    assert_rejected_before_acquire(SSID, b"", ConnectError::InvalidCredentials);
}

// ------------------------------------------------------------- 4. acquire failure

#[test]
fn acquire_failure_is_reported_and_nothing_else_runs() {
    bounded(|| {
        let w = World::default();
        w.force_acquire_fail.set(true);
        let out = exec(&w, Scenario::new(Assoc::Ok, Dh::Value(1), BackKind::Never));
        assert_eq!(out.end, SessionEnd::Failed(ConnectError::RadioUnavailable));
        assert_eq!(w.acquire_calls.get(), 1);
        assert_calls(&w, 0, 0, 0);
        assert_eq!(w.created.get(), 0);
        assert_released(&w);
        assert!(out.elapsed < SLACK, "took {:?}", out.elapsed);
    });
}

#[test]
fn acquire_while_token_held_fails_without_panic_and_leaves_owner_intact() {
    bounded(|| {
        let w = World::default();
        // another owner (e.g. a stale session) holds the interface
        let held = w.acquire().unwrap_or_else(|_| panic!("first acquire"));
        assert!(w.token.get());

        let out = exec(&w, Scenario::new(Assoc::Ok, Dh::Value(1), BackKind::Never));
        assert_eq!(out.end, SessionEnd::Failed(ConnectError::RadioUnavailable));
        assert_calls(&w, 0, 0, 0);
        // the session must not have released somebody else's token
        assert!(w.token.get(), "run released a token it never owned");
        assert_eq!(w.dropped.get(), 0);

        // once the owner lets go, entering again works
        drop(held);
        assert!(!w.token.get());
        let out = exec(
            &w,
            Scenario::new(Assoc::Ok, Dh::Value(1), BackKind::AfterServeStarted),
        );
        assert_eq!(out.end, SessionEnd::Exited(Phase::Serving));
        assert_calls(&w, 1, 1, 1);
        assert!(!w.token.get());
        assert_eq!(w.created.get(), w.dropped.get());
    });
}

// ------------------------------------------------------------- 5. failure-path release

#[test]
fn association_error_releases_and_skips_later_stages() {
    bounded(|| {
        let w = World::default();
        let out = exec(&w, Scenario::new(Assoc::Err, Dh::Value(1), BackKind::Never));
        assert_eq!(out.end, SessionEnd::Failed(ConnectError::AssociationFailed));
        assert_calls(&w, 1, 0, 0);
        assert_eq!(
            trace(&w),
            [
                "acquire",
                "associate",
                "associate guard dropped",
                "C dropped",
                "run returned",
            ]
        );
        assert_released(&w);
        // an immediate Err must not wait for the limit
        assert!(out.elapsed < SLACK, "took {:?}", out.elapsed);
    });
}

#[test]
fn association_timeout_releases_within_bound() {
    bounded(|| {
        let w = World::default();
        let mut s = Scenario::new(Assoc::Pending, Dh::Value(1), BackKind::Never);
        s.limits = limits(60, 5_000);
        let out = exec(&w, s);
        assert_eq!(
            out.end,
            SessionEnd::Failed(ConnectError::AssociationTimeout)
        );
        assert_calls(&w, 1, 0, 0);
        assert_eq!(
            trace(&w),
            [
                "acquire",
                "associate",
                "associate guard dropped",
                "C dropped",
                "run returned",
            ]
        );
        assert_released(&w);
        assert!(out.elapsed >= std_ms(60), "too early: {:?}", out.elapsed);
        assert!(
            out.elapsed < std_ms(60) + SLACK,
            "too late: {:?}",
            out.elapsed
        );
    });
}

#[test]
fn dhcp_timeout_releases_within_bound() {
    bounded(|| {
        let w = World::default();
        let mut s = Scenario::new(Assoc::Ok, Dh::Pending, BackKind::Never);
        s.limits = limits(5_000, 60);
        let out = exec(&w, s);
        assert_eq!(out.end, SessionEnd::Failed(ConnectError::DhcpTimeout));
        assert_calls(&w, 1, 1, 0);
        assert_eq!(
            trace(&w),
            [
                "acquire",
                "associate",
                "associate guard dropped",
                "dhcp",
                "dhcp guard dropped",
                "C dropped",
                "run returned",
            ]
        );
        assert_released(&w);
        assert!(out.elapsed >= std_ms(60), "too early: {:?}", out.elapsed);
        assert!(
            out.elapsed < std_ms(60) + SLACK,
            "too late: {:?}",
            out.elapsed
        );
    });
}

#[test]
fn dhcp_budget_is_independent_of_association_time() {
    // associate takes 80 ms (well under its limit); dhcp limit 80 ms and
    // never completes -> at least 160 ms in total, still DhcpTimeout, released.
    bounded(|| {
        let w = World::default();
        w.begin_run();
        let start = StdInstant::now();
        let end = block_on(with_timeout(
            INNER_GUARD,
            session::run(
                SSID,
                PASSWORD,
                limits(5_000, 80),
                || w.acquire(),
                async |_c: &mut C<'_>| -> Result<(), ()> {
                    Timer::after(ms(80)).await;
                    Ok(())
                },
                async |_c: &mut C<'_>| -> u32 { never(&w).await },
                async |_c: &mut C<'_>, _t: u32| -> Infallible { never(&w).await },
                BackFut {
                    world: &w,
                    kind: BackKind::Never,
                },
            ),
        ))
        .expect("run exceeded the inner guard timeout");
        let elapsed = start.elapsed();
        w.log("run returned");
        assert_eq!(end, SessionEnd::Failed(ConnectError::DhcpTimeout));
        assert!(elapsed >= std_ms(160), "too early: {elapsed:?}");
        assert!(elapsed < std_ms(160) + SLACK, "too late: {elapsed:?}");
        assert_released(&w);
    });
}

// "設定期限": the associate and dhcp limits of `Limits` each decide their
// own stage's timeout latency (a short and a long limit give different waits).
fn timeout_latency(associate: Assoc, dhcp: Dh, lim: Limits, expected: ConnectError) -> StdDuration {
    bounded(move || {
        let w = World::default();
        let mut s = Scenario::new(associate, dhcp, BackKind::Never);
        s.limits = lim;
        let out = exec(&w, s);
        assert_eq!(out.end, SessionEnd::Failed(expected));
        assert_released(&w);
        out.elapsed
    })
}

#[test]
fn timeout_duration_follows_configured_limits() {
    let short = limits(40, 40);
    let long = limits(400, 400);

    let short_assoc = timeout_latency(
        Assoc::Pending,
        Dh::Value(1),
        short,
        ConnectError::AssociationTimeout,
    );
    let long_assoc = timeout_latency(
        Assoc::Pending,
        Dh::Value(1),
        long,
        ConnectError::AssociationTimeout,
    );
    assert!(short_assoc >= std_ms(40), "{short_assoc:?}");
    assert!(short_assoc < std_ms(40) + SLACK, "{short_assoc:?}");
    assert!(long_assoc >= std_ms(400), "{long_assoc:?}");
    assert!(long_assoc < std_ms(400) + SLACK, "{long_assoc:?}");
    assert!(
        long_assoc >= short_assoc + std_ms(150),
        "short {short_assoc:?} vs long {long_assoc:?}"
    );

    let short_dhcp = timeout_latency(Assoc::Ok, Dh::Pending, short, ConnectError::DhcpTimeout);
    let long_dhcp = timeout_latency(Assoc::Ok, Dh::Pending, long, ConnectError::DhcpTimeout);
    assert!(short_dhcp >= std_ms(40), "{short_dhcp:?}");
    assert!(short_dhcp < std_ms(40) + SLACK, "{short_dhcp:?}");
    assert!(long_dhcp >= std_ms(400), "{long_dhcp:?}");
    assert!(long_dhcp < std_ms(400) + SLACK, "{long_dhcp:?}");
    assert!(
        long_dhcp >= short_dhcp + std_ms(150),
        "short {short_dhcp:?} vs long {long_dhcp:?}"
    );
}

// ------------------------------------------------------------- 6. back already ready

#[test]
fn back_already_ready_returns_promptly_exited_associating() {
    bounded(|| {
        let w = World::default();
        let out = exec(
            &w,
            Scenario::new(Assoc::Pending, Dh::Value(1), BackKind::Immediate),
        );
        assert_eq!(out.end, SessionEnd::Exited(Phase::Associating));
        // whether acquire/associate happened at all is up to the
        // implementation; whatever was built must be released
        assert_calls_at_most_associate_only(&w);
        assert_released(&w);
        assert!(out.elapsed < SLACK, "took {:?}", out.elapsed);
    });
}

fn assert_calls_at_most_associate_only(w: &World) {
    assert!(w.associate_calls.get() <= 1);
    assert_eq!(w.dhcp_calls.get(), 0, "dhcp must not run");
    assert_eq!(w.serve_calls.get(), 0, "serve must not run");
}

#[test]
fn back_already_ready_with_pending_dhcp_stage_is_bounded_and_released() {
    // back ready at once, stage futures never complete: still bounded, and
    // nothing leaks (phase is not asserted: only Associating is contractual
    // for "back already complete before acquire").
    bounded(|| {
        let w = World::default();
        let out = exec(
            &w,
            Scenario::new(Assoc::Ok, Dh::Pending, BackKind::Immediate),
        );
        assert!(
            matches!(out.end, SessionEnd::Exited(_)),
            "got {:?}",
            out.end
        );
        assert_released(&w);
        assert!(out.elapsed < SLACK, "took {:?}", out.elapsed);
    });
}

// ------------------------------------------------------------- 7. re-entry

#[derive(Clone, Copy, Debug)]
enum Path {
    SuccessThenBack,
    BackAtAssociate,
    BackAtDhcp,
    AssociateErr,
    AssociateTimeout,
    DhcpTimeout,
    MissingCredentials,
    InvalidCredentials,
    AcquireFails,
    BackImmediate,
}

const PATHS: [Path; 10] = [
    Path::SuccessThenBack,
    Path::BackAtAssociate,
    Path::BackAtDhcp,
    Path::AssociateErr,
    Path::AssociateTimeout,
    Path::DhcpTimeout,
    Path::MissingCredentials,
    Path::InvalidCredentials,
    Path::AcquireFails,
    Path::BackImmediate,
];

#[test]
fn one_hundred_consecutive_sessions_never_see_a_stale_owner() {
    bounded(|| {
        let w = World::default();
        let mut expected_created = 0u32;
        for i in 0..100usize {
            let path = PATHS[i % PATHS.len()];
            w.force_acquire_fail.set(matches!(path, Path::AcquireFails));
            let mut s = Scenario::new(Assoc::Ok, Dh::Value(DHCP_VALUE), BackKind::Never);
            // (expected end, whether acquire must succeed)
            let (expected, acquires): (SessionEnd, bool) = match path {
                Path::SuccessThenBack => {
                    s.back = BackKind::AfterServeStarted;
                    (SessionEnd::Exited(Phase::Serving), true)
                }
                Path::BackAtAssociate => {
                    s.associate = Assoc::Pending;
                    s.back = BackKind::AfterAssociateStarted;
                    (SessionEnd::Exited(Phase::Associating), true)
                }
                Path::BackAtDhcp => {
                    s.dhcp = Dh::Pending;
                    s.back = BackKind::AfterDhcpStarted;
                    (SessionEnd::Exited(Phase::Dhcp), true)
                }
                Path::AssociateErr => {
                    s.associate = Assoc::Err;
                    (SessionEnd::Failed(ConnectError::AssociationFailed), true)
                }
                Path::AssociateTimeout => {
                    s.associate = Assoc::Pending;
                    s.limits = limits(5, 5_000);
                    (SessionEnd::Failed(ConnectError::AssociationTimeout), true)
                }
                Path::DhcpTimeout => {
                    s.dhcp = Dh::Pending;
                    s.limits = limits(5_000, 5);
                    (SessionEnd::Failed(ConnectError::DhcpTimeout), true)
                }
                Path::MissingCredentials => {
                    s.ssid = b"";
                    (SessionEnd::Failed(ConnectError::MissingCredentials), false)
                }
                Path::InvalidCredentials => {
                    s.password = b"short";
                    (SessionEnd::Failed(ConnectError::InvalidCredentials), false)
                }
                Path::AcquireFails => (SessionEnd::Failed(ConnectError::RadioUnavailable), false),
                Path::BackImmediate => {
                    s.associate = Assoc::Pending;
                    s.back = BackKind::Immediate;
                    (SessionEnd::Exited(Phase::Associating), true)
                }
            };
            let before = w.created.get();
            let out = exec(&w, s);
            assert_eq!(out.end, expected, "iteration {i} path {path:?}");
            // A stale token would surface as RadioUnavailable (or as acquire
            // not creating a C) on a path whose acquire must succeed.
            if acquires {
                // BackImmediate may legitimately not reach acquire
                if !matches!(path, Path::BackImmediate) {
                    assert_eq!(w.created.get(), before + 1, "iteration {i} path {path:?}");
                }
            } else {
                assert_eq!(w.created.get(), before, "iteration {i} path {path:?}");
            }
            expected_created = w.created.get();
            assert_released(&w);
        }
        assert!(!w.token.get());
        assert_eq!(w.created.get(), w.dropped.get());
        assert!(
            expected_created >= 70,
            "too few sessions acquired: {expected_created}"
        );
    });
}

// ------------------------------------------------------------- 8. no leaked work

#[test]
fn cancelled_serve_future_is_dropped_and_never_polled_again() {
    bounded(|| {
        let w = World::default();
        let out = exec(
            &w,
            Scenario::new(
                Assoc::Ok,
                Dh::Value(DHCP_VALUE),
                BackKind::AfterServeStarted,
            ),
        );
        assert_eq!(out.end, SessionEnd::Exited(Phase::Serving));
        // serve's future was alive (guard) and has been dropped
        assert_eq!(w.guards_live.get(), 0);
        let ev = w.events();
        let serve_dropped = ev
            .iter()
            .position(|e| *e == "serve guard dropped")
            .expect("serve dropped");
        let ret = ev.iter().position(|e| *e == "run returned").unwrap();
        assert!(serve_dropped < ret);

        // no pending work survives: neither back nor any stage is polled
        // again, however long we keep the executor spinning afterwards
        let back_polls = w.back_polls.get();
        let stage_polls = w.stage_polls.get();
        let events_len = ev.len();
        assert!(stage_polls >= 1, "serve was never actually pending");
        block_on(Timer::after(ms(50)));
        assert_eq!(
            w.back_polls.get(),
            back_polls,
            "back polled after run returned"
        );
        assert_eq!(
            w.stage_polls.get(),
            stage_polls,
            "stage polled after run returned"
        );
        assert_eq!(
            w.events().len(),
            events_len,
            "late activity after run returned"
        );
    });
}

#[test]
fn cancelled_pending_stages_are_dropped_in_every_phase() {
    bounded(|| {
        for (scenario, phase) in [
            (
                Scenario::new(
                    Assoc::Pending,
                    Dh::Value(1),
                    BackKind::AfterAssociateStarted,
                ),
                Phase::Associating,
            ),
            (
                Scenario::new(Assoc::Ok, Dh::Pending, BackKind::AfterDhcpStarted),
                Phase::Dhcp,
            ),
            (
                Scenario::new(Assoc::Ok, Dh::Value(1), BackKind::AfterServeStarted),
                Phase::Serving,
            ),
        ] {
            let w = World::default();
            let out = exec(&w, scenario);
            assert_eq!(out.end, SessionEnd::Exited(phase));
            assert_eq!(w.guards_live.get(), 0, "{phase:?}: stage future leaked");
            let polls = (w.back_polls.get(), w.stage_polls.get());
            block_on(Timer::after(ms(20)));
            assert_eq!(
                (w.back_polls.get(), w.stage_polls.get()),
                polls,
                "{phase:?}"
            );
        }
    });
}

// ------------------------------------------------------------- 9. error text

#[test]
fn radio_unavailable_has_displayable_distinct_text() {
    let lines = ConnectError::RadioUnavailable.lines();
    assert!(!lines.is_empty());
    for line in lines {
        assert!(!line.is_empty(), "empty line in {lines:?}");
    }
    assert_eq!(lines[0], "WiFi init failed!");
    for other in [
        ConnectError::MissingCredentials,
        ConnectError::InvalidCredentials,
        ConnectError::AssociationFailed,
        ConnectError::AssociationTimeout,
        ConnectError::DhcpTimeout,
    ] {
        assert_ne!(
            lines[0],
            other.lines()[0],
            "shares first line with {other:?}"
        );
    }
}

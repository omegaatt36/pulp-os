//! Exercise the production handle ownership and borrow guard, not the host
//! storage writer stand-in. The fake models FAT's release-on-flush-error rule.
use std::cell::RefCell;
use std::rc::Rc;

use pulp_host::util::{CloseCell, CloseError, CloseHandle, CloseToken};

const LIMIT: usize = 8;

#[derive(Default)]
struct Events {
    closes: Vec<u8>,
    live: usize,
    cache_discards: usize,
}

struct Manager {
    open: [bool; LIMIT],
    events: Rc<RefCell<Events>>,
    fail_open: bool,
    fail_flush: bool,
    dirty_cache: bool,
}

impl Manager {
    fn open(&mut self) -> Result<u8, &'static str> {
        if self.fail_open {
            return Err("open failed");
        }
        let slot = self
            .open
            .iter()
            .position(|open| !open)
            .ok_or("files full")?;
        self.open[slot] = true;
        self.events.borrow_mut().live += 1;
        Ok(slot as u8)
    }

    fn write(&mut self, file: u8) {
        assert!(self.open[usize::from(file)], "write used a closed handle");
        self.dirty_cache = true;
    }
}

impl CloseHandle for Manager {
    type Handle = u8;
    type Error = &'static str;

    fn close_handle(&mut self, file: u8) -> Result<(), Self::Error> {
        assert!(self.open[usize::from(file)], "double close");
        self.open[usize::from(file)] = false;
        let mut events = self.events.borrow_mut();
        events.closes.push(file);
        events.live -= 1;
        if self.fail_flush {
            self.dirty_cache = false;
            events.cache_discards += 1;
            Err("flush failed")
        } else {
            Ok(())
        }
    }
}

type Storage = CloseCell<Manager, LIMIT>;

fn storage() -> (Storage, Rc<RefCell<Events>>) {
    let events = Rc::new(RefCell::new(Events::default()));
    let manager = Manager {
        open: [false; LIMIT],
        events: events.clone(),
        fail_open: false,
        fail_flush: false,
        dirty_cache: false,
    };
    (CloseCell::new(manager), events)
}

fn open_writer(storage: &Storage) -> Result<CloseToken<'_, Manager, LIMIT>, &'static str> {
    let mut inner = storage.borrow_mut();
    let mut token = storage.reserve().ok_or("writer slots full")?;
    let file = inner.open()?;
    token.attach(file).expect("fresh reservation");
    Ok(token)
}

#[test]
fn drop_during_callback_closes_once_when_outer_borrow_returns() {
    let (storage, events) = storage();
    let writer = open_writer(&storage).unwrap();
    {
        let mut callback_borrow = storage.borrow_mut();
        callback_borrow.write(writer.handle().unwrap());
        drop(writer);
        assert_eq!(events.borrow().live, 1);
        assert!(events.borrow().closes.is_empty());
        assert!(storage.try_borrow_mut().is_none());
    }
    assert_eq!(events.borrow().live, 0);
    assert_eq!(events.borrow().closes, [0]);
    drop(storage.borrow_mut());
    assert_eq!(events.borrow().closes, [0]);
}

#[test]
fn explicit_close_during_callback_reports_deferral_and_retains_handle() {
    let (storage, events) = storage();
    let mut writer = open_writer(&storage).unwrap();
    let callback_borrow = storage.borrow_mut();
    assert_eq!(writer.close(), Err(CloseError::Deferred));
    assert_eq!(writer.handle(), None);
    assert_eq!(writer.close(), Ok(()));
    drop(writer);
    assert!(events.borrow().closes.is_empty());
    drop(callback_borrow);
    assert_eq!(events.borrow().closes, [0]);
    assert_eq!(events.borrow().live, 0);
}

#[test]
fn all_pending_slots_drain_and_later_opens_and_writes_do_not_leak() {
    let (storage, events) = storage();
    let writers: Vec<_> = (0..LIMIT).map(|_| open_writer(&storage).unwrap()).collect();
    let callback_borrow = storage.borrow_mut();
    drop(writers);
    assert_eq!(events.borrow().live, LIMIT);
    assert!(
        storage.reserve().is_none(),
        "pending slots must keep ownership"
    );
    assert!(events.borrow().closes.is_empty());
    drop(callback_borrow);
    assert_eq!(events.borrow().closes, (0..LIMIT as u8).collect::<Vec<_>>());
    assert_eq!(events.borrow().live, 0);
    for _ in 0..LIMIT * 3 {
        let mut writer = open_writer(&storage).unwrap();
        storage.borrow_mut().write(writer.handle().unwrap());
        writer.close().unwrap();
    }
    assert_eq!(events.borrow().live, 0);
    assert_eq!(events.borrow().closes.len(), LIMIT * 4);
}

#[test]
fn immediate_flush_failure_is_reported_and_never_retried() {
    let (storage, events) = storage();
    let mut writer = open_writer(&storage).unwrap();
    {
        let mut inner = storage.borrow_mut();
        inner.write(writer.handle().unwrap());
        inner.fail_flush = true;
    }
    assert_eq!(writer.close(), Err(CloseError::Failed("flush failed")));
    drop(writer);
    assert_eq!(events.borrow().live, 0);
    assert_eq!(events.borrow().closes, [0]);
    assert_eq!(events.borrow().cache_discards, 1);
    let mut inner = storage.borrow_mut();
    assert!(!inner.dirty_cache);
    inner.fail_flush = false;
    drop(inner);
    drop(open_writer(&storage).unwrap());
    assert_eq!(events.borrow().closes, [0, 0]);
}

#[test]
fn deferred_flush_failure_releases_handle_and_cleans_cache() {
    let (storage, events) = storage();
    let mut writer = open_writer(&storage).unwrap();
    let mut callback_borrow = storage.borrow_mut();
    callback_borrow.write(writer.handle().unwrap());
    callback_borrow.fail_flush = true;
    assert_eq!(writer.close(), Err(CloseError::Deferred));
    drop(writer);
    drop(callback_borrow);
    assert_eq!(events.borrow().live, 0);
    assert_eq!(events.borrow().closes, [0]);
    assert_eq!(events.borrow().cache_discards, 1);
    let mut inner = storage.borrow_mut();
    assert!(!inner.dirty_cache);
    inner.fail_flush = false;
    drop(inner);
    let writer = open_writer(&storage).unwrap();
    storage.borrow_mut().write(writer.handle().unwrap());
    drop(writer);
    assert_eq!(events.borrow().live, 0);
    assert_eq!(events.borrow().closes, [0, 0]);
}

#[test]
fn reservation_exhaustion_and_failed_open_do_not_abandon_raw_handles() {
    let (storage, events) = storage();
    let reservations: Vec<_> = (0..LIMIT).map(|_| storage.reserve().unwrap()).collect();
    assert_eq!(open_writer(&storage).err(), Some("writer slots full"));
    assert_eq!(events.borrow().live, 0);
    drop(reservations);
    storage.borrow_mut().fail_open = true;
    for _ in 0..LIMIT * 2 {
        assert_eq!(open_writer(&storage).err(), Some("open failed"));
    }
    storage.borrow_mut().fail_open = false;
    let writers: Vec<_> = (0..LIMIT).map(|_| open_writer(&storage).unwrap()).collect();
    assert_eq!(events.borrow().live, LIMIT);
    drop(writers);
    assert_eq!(events.borrow().live, 0);
    assert_eq!(events.borrow().closes.len(), LIMIT);
}

#[test]
fn pending_handles_stay_with_their_original_manager() {
    let (first, first_events) = storage();
    let (second, second_events) = storage();
    let first_writer = open_writer(&first).unwrap();
    let second_writer = open_writer(&second).unwrap();
    let first_callback = first.borrow_mut();
    drop(first_writer);
    drop(second_writer);
    assert_eq!(first_events.borrow().live, 1);
    assert_eq!(second_events.borrow().live, 0);
    drop(first_callback);
    assert_eq!(first_events.borrow().closes, [0]);
    assert_eq!(second_events.borrow().closes, [0]);
}

#[test]
fn callback_error_path_still_drains_before_returning() {
    let (storage, events) = storage();
    let writer = open_writer(&storage).unwrap();
    let callback = || -> Result<(), &'static str> {
        let _borrow = storage.borrow_mut();
        drop(writer);
        Err("read failed")
    };
    assert_eq!(callback(), Err("read failed"));
    assert_eq!(events.borrow().live, 0);
    assert_eq!(events.borrow().closes, [0]);
}

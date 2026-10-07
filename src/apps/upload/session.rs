// One upload connection session: credentials, radio acquisition, bounded
// association and DHCP, then serving until BACK. Radio-free (the firmware
// passes its radio/network context in as `C`) so the host exercises the stage
// order, the cancellation and the ownership release.
//
// Ownership: `C` lives in `run` and every stage future borrows it, so whichever
// way the session ends (BACK in any stage, a failed or timed-out stage, an
// `acquire` error) the stage future is dropped first, then `C`, and only then
// does `run` return.

use core::cell::Cell;
use core::convert::Infallible;
use core::future::Future;

use embassy_futures::select::{Either, select};

use super::connect::{ConnectError, Limits, check_credentials, within};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Associating,
    Dhcp,
    Serving,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionEnd {
    /// `back` completed first, while in this phase.
    Exited(Phase),
    Failed(ConnectError),
}

#[allow(clippy::too_many_arguments)]
pub async fn run<C, E, T>(
    ssid: &[u8],
    password: &[u8],
    limits: Limits,
    acquire: impl FnOnce() -> Result<C, ConnectError>,
    associate: impl AsyncFnOnce(&mut C) -> Result<(), E>,
    dhcp: impl AsyncFnOnce(&mut C) -> T,
    serve: impl AsyncFnOnce(&mut C, T) -> Infallible,
    back: impl Future<Output = ()>,
) -> SessionEnd {
    if let Err(e) = check_credentials(ssid, password) {
        return SessionEnd::Failed(e);
    }
    // Acquired before anything awaits (it is synchronous), so a `back` that is
    // already complete still finds `c` to release; `c` is declared before the
    // stage futures, hence dropped after them.
    let mut c = match acquire() {
        Ok(c) => c,
        Err(e) => return SessionEnd::Failed(e),
    };

    let phase = Cell::new(Phase::Associating);
    // `serve` never returns: the `Ok` below only gives the block its type.
    #[allow(unreachable_code)]
    let stages = async {
        within(limits.associate, associate(&mut c))
            .await
            .map_err(|_| ConnectError::AssociationTimeout)?
            .map_err(|_| ConnectError::AssociationFailed)?;
        phase.set(Phase::Dhcp);
        let t = within(limits.dhcp, dhcp(&mut c))
            .await
            .map_err(|_| ConnectError::DhcpTimeout)?;
        phase.set(Phase::Serving);
        Ok::<Infallible, ConnectError>(serve(&mut c, t).await)
    };

    // `back` is polled first. The select future, and with it `stages` and the
    // running stage future, is dropped at the end of this statement.
    let ended = select(back, stages).await;
    match ended {
        Either::First(()) => SessionEnd::Exited(phase.get()),
        Either::Second(Err(e)) => SessionEnd::Failed(e),
        Either::Second(Ok(never)) => match never {},
    }
}

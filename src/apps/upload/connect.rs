// Station bring-up contract: credential check, stage limits, the error type and
// its screen text, and the bounded wait `session::run` applies to each stage.
// Radio-free so the logic is exercised on the host.

use core::future::Future;

use embassy_time::{Duration, TimeoutError, with_timeout};

pub const ASSOCIATE_TIMEOUT: Duration = Duration::from_secs(20);
pub const DHCP_TIMEOUT: Duration = Duration::from_secs(15);

const SSID_MAX: usize = 32;
const PASSWORD_LEN: core::ops::RangeInclusive<usize> = 8..=63;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub associate: Duration,
    pub dhcp: Duration,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        associate: ASSOCIATE_TIMEOUT,
        dhcp: DHCP_TIMEOUT,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectError {
    MissingCredentials,
    InvalidCredentials,
    AssociationFailed,
    AssociationTimeout,
    DhcpTimeout,
    RadioUnavailable,
    OutOfMemory,
}

impl ConnectError {
    pub fn lines(self) -> &'static [&'static str] {
        match self {
            Self::MissingCredentials => &[
                "No WiFi credentials!",
                "Set wifi_ssid in",
                "_PULP/SETTINGS.TXT",
            ],
            Self::InvalidCredentials => &[
                "WiFi config error!",
                "SSID: 1-32 bytes",
                "Password: 8-63 bytes",
            ],
            Self::AssociationFailed => &["Connection failed!", "Check SSID and password"],
            Self::AssociationTimeout => &["Connection timed out!", "Router not responding"],
            Self::DhcpTimeout => &["No IP address!", "DHCP timed out"],
            Self::RadioUnavailable => &["WiFi init failed!", "Radio not available"],
            Self::OutOfMemory => &["Out of memory!", "Restart and retry"],
        }
    }
}

pub fn check_credentials(ssid: &[u8], password: &[u8]) -> Result<(), ConnectError> {
    if ssid.is_empty() {
        Err(ConnectError::MissingCredentials)
    } else if ssid.len() > SSID_MAX || !PASSWORD_LEN.contains(&password.len()) {
        Err(ConnectError::InvalidCredentials)
    } else {
        Ok(())
    }
}

// The clock reads whole ticks (floor), so a deadline of `now + limit` can fire
// up to one tick before `limit` has really elapsed; one extra tick makes the
// limit a lower bound as well as the upper one.
const fn at_least(limit: Duration) -> Duration {
    Duration::from_ticks(limit.as_ticks().saturating_add(1))
}

// `fut` bounded by `limit` as a lower and upper bound (see `at_least`); used by
// `session::run` for the association and DHCP stages.
pub(super) async fn within<F: Future>(limit: Duration, fut: F) -> Result<F::Output, TimeoutError> {
    with_timeout(at_least(limit), fut).await
}

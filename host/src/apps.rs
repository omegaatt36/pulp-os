// Production apps and manager over the host board/storage seams. Upload is
// firmware-only; AppId and the aliases mirror the offline firmware surface.
#[path = "../../src/apps/files.rs"]
#[allow(unexpected_cfgs)]
pub mod files;
#[path = "../../src/apps/home.rs"]
#[allow(unexpected_cfgs)]
pub mod home;
#[path = "../../src/apps/manager.rs"]
#[allow(unexpected_cfgs)]
pub mod manager;
#[path = "../../src/apps/reader/mod.rs"]
#[allow(dead_code, unused_imports)]
pub mod reader;
#[path = "apps/settings.rs"]
#[allow(dead_code, unused_imports)]
pub mod settings;
#[path = "../../src/apps/widgets/mod.rs"]
#[allow(dead_code)]
pub mod widgets;

// read-only view of ReaderApp's pub(super) state for crate::reader::Rig
pub(crate) mod probe;

use crate::kernel::app::AppIdType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppId {
    Home,
    Files,
    Reader,
    Settings,
}

impl AppIdType for AppId {
    const HOME: Self = Self::Home;
}

pub type Transition = crate::kernel::app::Transition<AppId>;
pub type NavEvent = crate::kernel::app::NavEvent<AppId>;
pub type Launcher = crate::kernel::app::Launcher<AppId>;

pub use crate::kernel::StorageError;
pub use crate::kernel::app::{App, AppContext, PendingSetting, RECENT_FILE, Redraw};
pub use crate::kernel::{Error, ErrorKind, Result, ResultExt};

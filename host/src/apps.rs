// host stand-in for src/apps/mod.rs: the reader, the settings app and the
// widgets are the real files (settings is pure UI + config, no board deps);
// the other apps are firmware-only (SD / display drivers). The AppId enum and
// the aliases below are the only lines taken over from the real file.
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

// Whole-device flows on the host: hardware keys in, apps and card out.
//
// Run: cargo test-host --test e2e

#[path = "e2e/device.rs"]
mod device;
#[path = "e2e/epub.rs"]
mod epub;
#[path = "e2e/library.rs"]
mod library;
#[path = "e2e/reading.rs"]
mod reading;
#[path = "e2e/session.rs"]
mod session;
#[path = "e2e/settings.rs"]
mod settings;

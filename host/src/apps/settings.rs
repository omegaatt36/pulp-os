include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/apps/settings.rs"));

// Read-only host probe of the production settings save flag.
pub(crate) fn is_dirty(app: &SettingsApp) -> bool {
    app.save_needed
}

// pulp-host's font data comes from the firmware's own generator: the root
// build.rs is included as a module (not copied), so the glyph tables the host
// measures and renders with are exactly the ones the firmware links. Only its
// font generator runs here; its `main` (esp-hal linker scripts) is not used.
#[allow(dead_code)]
#[path = "../build.rs"]
mod firmware;

fn main() {
    firmware::generate_bitmap_fonts_in(std::path::Path::new("../assets/fonts"));
    firmware::generate_flash_fonts();
}

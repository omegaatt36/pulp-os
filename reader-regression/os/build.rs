// The real root build.rs (font rasterisation), included verbatim: only its font
// generator is used, pointed at the repository's assets/fonts.
#[allow(dead_code, unused)]
mod real {
    include!("../../build.rs");
    pub fn run() {
        generate_bitmap_fonts_in(std::path::Path::new("../../assets/fonts"));
    }
}

fn main() {
    real::run();
}

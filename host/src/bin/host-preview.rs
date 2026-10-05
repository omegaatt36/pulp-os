use std::path::PathBuf;

use pulp_host::fixtures::{standard, standard_card};
use pulp_host::fonts::FONT_SIZE_COUNT;
use pulp_host::kernel::config::{DEFAULT_FONT_SIZE_IDX, DEFAULT_READING_THEME, NUM_READING_THEMES};
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::render::{render_full, render_stitched};

struct Options {
    output: PathBuf,
    fixture: Option<String>,
    page: usize,
    font: u8,
    theme: u8,
}

fn number(value: &str, option: &str) -> Result<usize, String> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("{option} requires a nonnegative integer: {value}"));
    }
    value
        .parse()
        .map_err(|_| format!("{option} value is too large: {value}"))
}

fn options() -> Result<Option<Options>, String> {
    let mut args = std::env::args().skip(1);
    let mut output = None;
    let mut fixture = None;
    let mut page = 0;
    let mut font = DEFAULT_FONT_SIZE_IDX as usize;
    let mut theme = DEFAULT_READING_THEME as usize;
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!(
                "Usage: scripts/host-preview.sh --output-dir DIR [--fixture NAME] [--page N] [--font N] [--theme N]\n\nDefaults: all standard fixtures, page 0, font {DEFAULT_FONT_SIZE_IDX}, theme {DEFAULT_READING_THEME}.\n--page is a zero-based ordinal across EPUB chapters.\n--font accepts 0..{}; --theme accepts 0..{}.\nArtifacts: 480x800 binary P4 PBM.\nFixtures: {}",
                FONT_SIZE_COUNT - 1,
                NUM_READING_THEMES - 1,
                standard()
                    .iter()
                    .map(|f| f.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return Ok(None);
        }
        if !matches!(
            arg.as_str(),
            "--output-dir" | "--fixture" | "--page" | "--font" | "--theme"
        ) {
            return Err(format!("unknown argument: {arg}; use --help"));
        }
        let value = args
            .next()
            .filter(|v| !v.starts_with("--") && !v.is_empty())
            .ok_or_else(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--output-dir" => output = Some(PathBuf::from(value)),
            "--fixture" => fixture = Some(value),
            "--page" => page = number(&value, &arg)?,
            "--font" => font = number(&value, &arg)?,
            "--theme" => theme = number(&value, &arg)?,
            _ => unreachable!(),
        }
    }
    if font >= FONT_SIZE_COUNT {
        return Err(format!("--font must be below {FONT_SIZE_COUNT}"));
    }
    if theme >= NUM_READING_THEMES as usize {
        return Err(format!("--theme must be below {NUM_READING_THEMES}"));
    }
    Ok(Some(Options {
        output: output.ok_or("--output-dir is required; use --help")?,
        fixture,
        page,
        font: font as u8,
        theme: theme as u8,
    }))
}

fn ready(rig: &mut Rig, name: &str) -> Result<(), String> {
    for _ in 0..4000 {
        if rig.phase() != Phase::Ready {
            return Err(format!(
                "{name}: reader phase {:?}, error {:?}",
                rig.phase(),
                rig.error_kind()
            ));
        }
        if !rig.has_bg_work() {
            return Ok(());
        }
        rig.idle(1);
    }
    Err(format!(
        "{name}: background work did not finish in 4000 ticks"
    ))
}

fn run(o: Options) -> Result<(), String> {
    let fixtures = standard();
    if let Some(name) = &o.fixture {
        if !fixtures.iter().any(|f| f.name == name) {
            return Err(format!("unknown fixture: {name}; use --help"));
        }
    }
    std::fs::create_dir_all(&o.output).map_err(|e| format!("{}: {e}", o.output.display()))?;
    for f in fixtures
        .iter()
        .filter(|f| o.fixture.as_ref().is_none_or(|name| name == f.name))
    {
        let mut rig = Rig::new(standard_card());
        rig.configure(o.font, o.theme);
        rig.open(f.name);
        ready(&mut rig, f.name)?;
        for ordinal in 0..o.page {
            let before = (rig.chapter(), rig.page());
            rig.press(Action::Next);
            ready(&mut rig, f.name)?;
            if before == (rig.chapter(), rig.page()) {
                return Err(format!(
                    "{}: --page {} is out of range (last ordinal {ordinal})",
                    f.name, o.page
                ));
            }
        }
        let full = render_full(&|strip| rig.draw(strip)).frame;
        let stitched = render_stitched(&|strip| rig.draw(strip)).frame;
        if full.to_pbm() != stitched.to_pbm() {
            return Err(format!("{}: full/stitched pixels differ", f.name));
        }
        let path = o.output.join(format!(
            "{}-page{}-font{}-theme{}.pbm",
            f.name, o.page, o.font, o.theme
        ));
        full.write_pbm(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}

fn main() {
    let result = options().and_then(|o| o.map_or(Ok(()), run));
    if let Err(error) = result {
        eprintln!("host-preview: {error}");
        std::process::exit(1);
    }
}

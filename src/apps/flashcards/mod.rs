// flash cards: kana, or a vocabulary deck the user puts on the SD card

pub mod deck;
pub mod pitch;
pub mod progress;
pub mod table;
pub mod vocab;

use alloc::vec::Vec;
use core::fmt::Write as _;

use embedded_graphics::{pixelcolor::BinaryColor, prelude::*, primitives::PrimitiveStyle};

use crate::apps::{App, AppContext, AppId, Transition};
use crate::board::action::{Action, ActionEvent};
use crate::board::{SCREEN_H, SCREEN_W};
use crate::drivers::strip::StripBuffer;
use crate::fonts::{
    self, Style,
    cjk::{PreparedFonts, SurfaceFonts, VisibleText},
};
use crate::kernel::KernelHandle;
use crate::ui::InfallibleExt;
use crate::ui::{
    Alignment, BUTTON_BAR_H, BitmapDynLabel, BitmapLabel, CONTENT_TOP, FULL_CONTENT_W,
    LARGE_MARGIN, Region, SECTION_GAP, StackFmt, TITLE_Y, wrap_next, wrap_prev,
};
use deck::{CHART_COLS, Card, Deck, Direction, Groups, Script, Set, chart_position};
use progress::{FILE_LEN, Progress, SLOTS, Status};
use vocab::{Entry, Vocab};

pub const PROGRESS_FILE: &str = "KANA.BIN";
pub const VOCAB_DIR: &str = "VOCAB";
// the decks looked for, in menu order; the card has no directory listing to
// ask, and a fixed set keeps the names 8.3
pub const VOCAB_DECKS: [&str; 6] = ["N5", "N4", "N3", "N2", "N1", "CUSTOM"];

const READ_CHUNK: usize = 32 * 1024;

// The card glyph is the largest body size, shown at this magnification. Body,
// not heading: an auxiliary surface prepares only its body role.
const CARD_FONT_IDX: u8 = 4;
const CARD_SCALE: u16 = 4;
const WORD_SCALE: u16 = 2;
const BODY_PX: u16 = fonts::cjk::BODY_PIXELS[CARD_FONT_IDX as usize];
const TEXT_LINE_H: u16 = BODY_PX + 8;
const MAX_GLOSS_LINES: usize = 7;
// one pitch contour: a high and a low line, each 2 px thick
const PITCH_H: u16 = 20;
const PITCH_HIGH: u16 = 2;
const PITCH_LOW: u16 = 12;

const ROW_H: u16 = 52;
const ROW_GAP: u16 = 8;
const ROW_STRIDE: u16 = ROW_H + ROW_GAP;
const LABEL_W: u16 = 160;
const VALUE_X: u16 = LARGE_MARGIN + LABEL_W + 8;
const VALUE_W: u16 = FULL_CONTENT_W - LABEL_W - 8;
const ROW_SOURCE: usize = 0;
const ROW_SCRIPT: usize = 1;
const ROW_SET: usize = 2;
const ROW_GROUP: usize = 3;
const ROW_DIRECTION: usize = 4;
const ROW_FOCUS: usize = 5;
const ROW_OVERVIEW: usize = 6;
const ROW_RESET: usize = 7;
const MENU_ROWS: usize = 8;

// the gojuon chart of the overview
const CELL_H: u16 = 48;
const COUNT_W: u16 = 120;

const CONTENT_REGION: Region = Region::new(0, CONTENT_TOP, SCREEN_W, SCREEN_H - CONTENT_TOP);
const GLYPH_REGION: Region = Region::new(0, 180, SCREEN_W, 260);
const ROMAJI_REGION: Region = Region::new(0, 470, SCREEN_W, 80);
const WORD_REGION: Region = Region::new(0, 140, SCREEN_W, 180);
const WORD_BACK_REGION: Region = Region::new(0, 330, SCREEN_W, 380);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Source {
    Kana,
    Vocab(usize),
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Self::Kana => "Kana",
            Self::Vocab(i) => VOCAB_DECKS[i],
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Kana => 0,
            Self::Vocab(i) => i + 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Open,
    Read { size: u32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Menu,
    Loading(Stage),
    Card { slot: usize, flipped: bool },
    Overview,
}

pub struct FlashcardsApp {
    state: State,
    row: usize,
    confirm_reset: bool,
    notice: Option<&'static str>,
    source: Source,
    available: [bool; VOCAB_DECKS.len()],
    script: Script,
    set: Set,
    // 0 for every row of the set, else the nth row key of `Groups::of(set)`
    group: usize,
    direction: Direction,
    weak_only: bool,
    deck: Deck,
    progress: Progress,
    kana_loaded: bool,
    playing: Source,
    vocab: Vocab,
    vocab_slots: Vec<u16>,
    vocab_progress: Progress,
    vocab_deck: Option<usize>,
    kana_dirty: bool,
    vocab_dirty: bool,
    reset_vocab: Option<usize>,
    release_vocab: bool,
    seed: u32,
    ui_fonts: fonts::UiFonts,
    card_fonts: SurfaceFonts,
}

impl Default for FlashcardsApp {
    fn default() -> Self {
        Self::new()
    }
}

impl FlashcardsApp {
    pub fn new() -> Self {
        let mut card_fonts = SurfaceFonts::new();
        card_fonts.set_size(CARD_FONT_IDX);
        let (script, set) = (Script::Hiragana, Set::Basic);
        Self {
            state: State::Menu,
            row: 0,
            confirm_reset: false,
            notice: None,
            source: Source::Kana,
            available: [false; VOCAB_DECKS.len()],
            script,
            set,
            group: 0,
            direction: Direction::Forward,
            weak_only: false,
            deck: Deck::build(script, set, Direction::Forward, None),
            progress: Progress::kana(),
            kana_loaded: false,
            playing: Source::Kana,
            vocab: Vocab::new(),
            vocab_slots: Vec::new(),
            vocab_progress: Progress::new(0),
            vocab_deck: None,
            kana_dirty: false,
            vocab_dirty: false,
            reset_vocab: None,
            release_vocab: false,
            seed: 0x9e37_79b9,
            ui_fonts: fonts::UiFonts::for_size(0),
            card_fonts,
        }
    }

    pub fn set_ui_font_size(&mut self, idx: u8) {
        self.ui_fonts = fonts::UiFonts::for_size(idx);
    }

    pub fn reset_storage(&mut self) {
        self.on_exit();
        self.state = State::Menu;
        self.progress = Progress::kana();
        self.kana_loaded = false;
        self.kana_dirty = false;
        self.vocab_dirty = false;
        self.reset_vocab = None;
        self.drop_vocab();
    }

    fn group_key(&self) -> Option<&'static str> {
        let index = self.group.checked_sub(1)?;
        Groups::of(self.set).keys().get(index).copied()
    }

    fn rebuild_deck(&mut self) {
        self.deck = Deck::build(self.script, self.set, self.direction, self.group_key());
    }

    fn step_group(&mut self, forward: bool) {
        let count = Groups::of(self.set).keys().len() + 1;
        self.group = if forward {
            wrap_next(self.group, count)
        } else {
            wrap_prev(self.group, count)
        };
    }

    fn drop_vocab(&mut self) {
        self.vocab.clear();
        self.vocab_slots = Vec::new();
        self.vocab_progress = Progress::new(0);
        self.vocab_deck = None;
        self.release_vocab = false;
    }

    fn load_kana(&mut self, k: &mut KernelHandle<'_>) {
        let mut buf = [0u8; FILE_LEN];
        self.progress = match k.read_app_data_start(PROGRESS_FILE, &mut buf) {
            Ok((_, n)) => Progress::decode(&buf[..n]),
            Err(_) => None,
        }
        .unwrap_or_else(Progress::kana);
        self.kana_loaded = true;
    }

    fn find_decks(&mut self, k: &mut KernelHandle<'_>) {
        for (i, deck) in VOCAB_DECKS.iter().enumerate() {
            let mut name = StackFmt::<16>::new();
            let _ = write!(name, "{deck}.TSV");
            self.available[i] = matches!(
                k.optional_file_size_app_subdir(VOCAB_DIR, name.as_str()),
                Ok(Some(n)) if n > 0
            );
        }
        if let Source::Vocab(i) = self.source
            && !self.available[i]
        {
            self.source = Source::Kana;
        }
    }

    fn save(&mut self, k: &mut KernelHandle<'_>) {
        if self.kana_dirty {
            let mut buf = [0u8; FILE_LEN];
            self.progress.encode(&mut buf);
            match k.write_app_data(PROGRESS_FILE, &buf) {
                Ok(()) => self.kana_dirty = false,
                Err(e) => log::error!("flashcards: save failed: {}", e),
            }
        }
        if self.vocab_dirty {
            self.vocab_dirty = false;
            if let Some(deck) = self.vocab_deck {
                let mut name = StackFmt::<16>::new();
                let _ = write!(name, "{}.PRG", VOCAB_DECKS[deck]);
                match self.vocab_progress.words_file(self.vocab.hashes()) {
                    Some(bytes) => {
                        if let Err(e) = k.write_app_subdir(VOCAB_DIR, name.as_str(), &bytes) {
                            log::error!("flashcards: save failed: {}", e);
                        }
                    }
                    None => log::error!("flashcards: save failed: out of memory"),
                }
            }
        }
        if let Some(deck) = self.reset_vocab.take() {
            let mut name = StackFmt::<16>::new();
            let _ = write!(name, "{}.PRG", VOCAB_DECKS[deck]);
            let _ = k.delete_app_subdir(VOCAB_DIR, name.as_str());
            if self.vocab_deck == Some(deck) {
                self.vocab_progress.reset();
            }
        }
    }

    fn fail(&mut self, notice: &'static str, ctx: &mut AppContext) {
        self.notice = Some(notice);
        self.vocab.clear();
        self.state = State::Menu;
        ctx.mark_dirty(CONTENT_REGION);
    }

    fn load_step(
        &mut self,
        stage: Stage,
        deck: usize,
        ctx: &mut AppContext,
        k: &mut KernelHandle<'_>,
    ) {
        let mut name = StackFmt::<16>::new();
        let _ = write!(name, "{}.TSV", VOCAB_DECKS[deck]);
        match stage {
            Stage::Open => match k.file_size_app_subdir(VOCAB_DIR, name.as_str()) {
                Ok(size) if size > 0 => {
                    if self.vocab.reserve(size as usize) {
                        self.state = State::Loading(Stage::Read { size });
                    } else {
                        self.fail("Deck too big for memory", ctx);
                    }
                }
                _ => self.fail("Cannot read the deck", ctx),
            },
            Stage::Read { size } => {
                let offset = self.vocab.loaded_bytes() as u32;
                let mut failed = false;
                let got = self.vocab.fill(READ_CHUNK, |buf| {
                    match k.read_app_subdir_chunk(VOCAB_DIR, name.as_str(), offset, buf) {
                        Ok(n) => n,
                        Err(_) => {
                            failed = true;
                            0
                        }
                    }
                });
                if failed {
                    self.fail("Cannot read the deck", ctx);
                } else if got == 0 || offset + got as u32 >= size {
                    self.finish_load(deck, ctx, k);
                }
            }
        }
    }

    fn finish_load(&mut self, deck: usize, ctx: &mut AppContext, k: &mut KernelHandle<'_>) {
        self.vocab.index();
        if self.vocab.is_empty() {
            self.fail("No words in the deck", ctx);
            return;
        }
        let mut name = StackFmt::<16>::new();
        let _ = write!(name, "{}.PRG", VOCAB_DECKS[deck]);
        let saved = match k.optional_file_size_app_subdir(VOCAB_DIR, name.as_str()) {
            Ok(Some(size)) if size as usize <= vocab::MAX_BYTES => {
                read_whole(k, name.as_str(), size as usize)
            }
            _ => None,
        };
        let words = self.vocab.len();
        self.vocab_progress = saved
            .and_then(|bytes| Progress::from_words(&bytes, self.vocab.hashes()))
            .unwrap_or_else(|| Progress::new(words));
        self.vocab_slots = (0..words as u16).collect();
        self.vocab_deck = Some(deck);
        self.playing = Source::Vocab(deck);
        match self.next_card(None) {
            Some(state) => {
                self.state = state;
                ctx.mark_dirty(CONTENT_REGION);
            }
            None => self.fail(self.empty_notice(), ctx),
        }
    }

    fn romaji_font() -> &'static fonts::bitmap::BitmapFont {
        fonts::heading_font(fonts::max_size_idx())
    }

    fn row_region(&self, row: usize) -> Region {
        Region::new(
            LARGE_MARGIN,
            self.rows_top() + row as u16 * ROW_STRIDE,
            FULL_CONTENT_W,
            ROW_H,
        )
    }

    fn rows_top(&self) -> u16 {
        TITLE_Y + self.ui_fonts.heading.line_height + SECTION_GAP * 3
    }

    fn summary_region(&self) -> Region {
        Region::new(
            LARGE_MARGIN,
            self.rows_top() + MENU_ROWS as u16 * ROW_STRIDE + SECTION_GAP,
            FULL_CONTENT_W,
            self.ui_fonts.body.line_height,
        )
    }

    fn notice_region(&self) -> Region {
        let r = self.summary_region();
        Region::new(r.x, r.y + r.h + SECTION_GAP, r.w, r.h)
    }

    fn playing_state(&self) -> (&Progress, &[u16]) {
        match self.playing {
            Source::Kana => (&self.progress, self.deck.slots()),
            Source::Vocab(_) => (&self.vocab_progress, &self.vocab_slots),
        }
    }

    fn playing_progress(&self) -> &Progress {
        self.playing_state().0
    }

    fn known(&self) -> usize {
        let (progress, slots) = self.playing_state();
        progress.known_count(slots)
    }

    fn total(&self) -> usize {
        self.playing_state().1.len()
    }

    fn next_card(&mut self, avoid: Option<usize>) -> Option<State> {
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223)
            ^ (embassy_time::Instant::now().as_ticks() as u32);
        let (progress, slots) = self.playing_state();
        progress
            .pick(slots, self.seed, avoid, self.weak_only)
            .map(|slot| State::Card {
                slot,
                flipped: false,
            })
    }

    fn show_next(&mut self, avoid: Option<usize>) {
        match self.next_card(avoid) {
            Some(state) => self.state = state,
            None => {
                self.notice = Some(self.empty_notice());
                self.state = State::Menu;
                self.card_fonts.clear();
            }
        }
    }

    fn empty_notice(&self) -> &'static str {
        if self.weak_only {
            "No weak cards to drill"
        } else {
            "No cards in this selection"
        }
    }

    fn step_source(&mut self, forward: bool) {
        let count = VOCAB_DECKS.len() + 1;
        let mut i = self.source.index();
        for _ in 0..count {
            i = if forward {
                wrap_next(i, count)
            } else {
                wrap_prev(i, count)
            };
            if i == 0 || self.available[i - 1] {
                break;
            }
        }
        self.source = if i == 0 {
            Source::Kana
        } else {
            Source::Vocab(i - 1)
        };
    }

    fn start(&mut self, ctx: &mut AppContext) {
        self.notice = None;
        match self.source {
            Source::Kana => {
                self.playing = Source::Kana;
                self.show_next(None);
            }
            Source::Vocab(deck) if self.vocab_deck == Some(deck) && !self.vocab.is_empty() => {
                self.playing = self.source;
                self.show_next(None);
            }
            Source::Vocab(_) => {
                self.drop_vocab();
                self.state = State::Loading(Stage::Open);
            }
        }
        ctx.mark_dirty(CONTENT_REGION);
    }

    fn on_event_menu(&mut self, event: ActionEvent, ctx: &mut AppContext) -> Transition {
        // the release of the OK that asked must not cancel the ask
        if matches!(event, ActionEvent::Release(_)) {
            return Transition::None;
        }
        let was_confirming = core::mem::take(&mut self.confirm_reset);
        if was_confirming {
            ctx.mark_dirty(self.row_region(ROW_RESET));
        }
        match event {
            ActionEvent::Press(Action::Back) => Transition::Pop,
            ActionEvent::LongPress(Action::Back) => Transition::Home,
            ActionEvent::Press(Action::Next) | ActionEvent::Press(Action::Prev) => {
                let forward = matches!(event, ActionEvent::Press(Action::Next));
                ctx.mark_dirty(self.row_region(self.row));
                self.row = if forward {
                    wrap_next(self.row, MENU_ROWS)
                } else {
                    wrap_prev(self.row, MENU_ROWS)
                };
                ctx.mark_dirty(self.row_region(self.row));
                Transition::None
            }
            ActionEvent::Press(Action::NextJump) | ActionEvent::Press(Action::PrevJump)
                if self.row != ROW_RESET =>
            {
                let forward = matches!(event, ActionEvent::Press(Action::NextJump));
                match (self.row, self.source) {
                    (ROW_SOURCE, _) => {
                        self.step_source(forward);
                        self.notice = None;
                    }
                    (ROW_SCRIPT, Source::Kana) => self.script = self.script.step(forward),
                    (ROW_SET, Source::Kana) => {
                        self.set = self.set.step(forward);
                        self.group = 0;
                    }
                    (ROW_GROUP, Source::Kana) => self.step_group(forward),
                    (ROW_DIRECTION, Source::Kana) => self.direction = self.direction.step(forward),
                    (ROW_FOCUS, _) => self.weak_only = !self.weak_only,
                    _ => return Transition::None,
                }
                self.rebuild_deck();
                ctx.mark_dirty(CONTENT_REGION);
                Transition::None
            }
            ActionEvent::Press(Action::Select) if self.row == ROW_RESET => {
                if was_confirming {
                    match self.source {
                        Source::Kana => {
                            self.progress.reset();
                            self.kana_dirty = true;
                        }
                        Source::Vocab(deck) => {
                            self.reset_vocab = Some(deck);
                            self.vocab_dirty = false;
                        }
                    }
                    ctx.mark_dirty(self.summary_region());
                } else {
                    self.confirm_reset = true;
                }
                ctx.mark_dirty(self.row_region(ROW_RESET));
                Transition::None
            }
            ActionEvent::Press(Action::Select) if self.row == ROW_OVERVIEW => {
                self.state = State::Overview;
                ctx.mark_dirty(CONTENT_REGION);
                Transition::None
            }
            ActionEvent::Press(Action::Select) => {
                self.start(ctx);
                Transition::None
            }
            _ => Transition::None,
        }
    }

    fn on_event_overview(&mut self, event: ActionEvent, ctx: &mut AppContext) -> Transition {
        match event {
            ActionEvent::Press(Action::Back) | ActionEvent::Press(Action::Select) => {
                self.state = State::Menu;
                self.card_fonts.clear();
                ctx.mark_dirty(CONTENT_REGION);
                Transition::None
            }
            ActionEvent::LongPress(Action::Back) => Transition::Home,
            _ => Transition::None,
        }
    }

    fn on_event_card(
        &mut self,
        slot: usize,
        flipped: bool,
        event: ActionEvent,
        ctx: &mut AppContext,
    ) -> Transition {
        match event {
            ActionEvent::Press(Action::Back) => {
                self.state = State::Menu;
                self.card_fonts.clear();
                ctx.mark_dirty(CONTENT_REGION);
                Transition::None
            }
            ActionEvent::LongPress(Action::Back) => Transition::Home,
            ActionEvent::Press(Action::Select) if !flipped => {
                self.state = State::Card {
                    slot,
                    flipped: true,
                };
                ctx.mark_dirty(CONTENT_REGION);
                Transition::None
            }
            ActionEvent::Press(Action::Select)
            | ActionEvent::Press(Action::Next)
            | ActionEvent::Press(Action::NextJump)
                if flipped =>
            {
                self.answer(slot, true, ctx)
            }
            ActionEvent::Press(Action::Prev) | ActionEvent::Press(Action::PrevJump) if flipped => {
                self.answer(slot, false, ctx)
            }
            _ => Transition::None,
        }
    }

    fn answer(&mut self, slot: usize, got_it: bool, ctx: &mut AppContext) -> Transition {
        match self.playing {
            Source::Kana => {
                self.progress.answer(slot, got_it);
                self.kana_dirty = true;
            }
            Source::Vocab(_) => {
                self.vocab_progress.answer(slot, got_it);
                self.vocab_dirty = true;
            }
        }
        self.show_next(Some(slot));
        ctx.mark_dirty(CONTENT_REGION);
        Transition::None
    }

    fn draw_menu(&self, strip: &mut StripBuffer) {
        let title = Region::new(
            LARGE_MARGIN,
            TITLE_Y,
            FULL_CONTENT_W,
            self.ui_fonts.heading.line_height,
        );
        BitmapLabel::new(title, "Flash cards", self.ui_fonts.heading)
            .draw(strip)
            .unwrap_infallible();

        let kana = self.source == Source::Kana;
        let mut rows_value = StackFmt::<16>::new();
        let _ = match (kana, self.group_key()) {
            (false, _) => rows_value.write_str("-"),
            (true, None) => rows_value.write_str("All"),
            (true, Some("")) => rows_value.write_str("Vowels"),
            (true, Some(key)) => write!(rows_value, "{key}-"),
        };
        let reset_value = if self.confirm_reset {
            "OK to confirm"
        } else {
            "Reset"
        };
        for (i, (label, value)) in [
            ("Source", self.source.label()),
            ("Script", if kana { self.script.label() } else { "-" }),
            ("Cards", if kana { self.set.label() } else { "-" }),
            ("Rows", rows_value.as_str()),
            ("Direction", if kana { self.direction.label() } else { "-" }),
            ("Focus", if self.weak_only { "Weak only" } else { "All" }),
            ("Overview", "Open"),
            ("Progress", reset_value),
        ]
        .into_iter()
        .enumerate()
        {
            let row = self.row_region(i);
            let selected = i == self.row;
            let label_region = Region::new(row.x, row.y, LABEL_W, ROW_H);
            let value_region = Region::new(VALUE_X, row.y, VALUE_W, ROW_H);
            BitmapLabel::new(label_region, label, self.ui_fonts.body)
                .inverted(selected)
                .draw(strip)
                .unwrap_infallible();
            BitmapLabel::new(value_region, value, self.ui_fonts.body)
                .alignment(Alignment::Center)
                .inverted(selected)
                .draw(strip)
                .unwrap_infallible();
        }

        let mut summary = BitmapDynLabel::<64>::new(self.summary_region(), self.ui_fonts.body)
            .alignment(Alignment::TopLeft);
        match self.source {
            Source::Kana => {
                let _ = write!(
                    summary,
                    "{} cards, {} known",
                    self.deck.slots().len(),
                    self.progress.known_count(self.deck.slots())
                );
            }
            Source::Vocab(deck) if self.vocab_deck == Some(deck) => {
                let _ = write!(
                    summary,
                    "{} words, {} known",
                    self.vocab_slots.len(),
                    self.vocab_progress.known_count(&self.vocab_slots)
                );
            }
            Source::Vocab(_) => {
                let _ = write!(summary, "Vocabulary deck from the card");
            }
        }
        summary.draw(strip).unwrap_infallible();

        let note = match (self.notice, self.available.iter().any(|&a| a)) {
            (Some(notice), _) => notice,
            (None, false) => "Decks: _PULP/VOCAB/N5.TSV ...",
            (None, true) => "",
        };
        BitmapLabel::new(self.notice_region(), note, self.ui_fonts.body)
            .draw(strip)
            .unwrap_infallible();

        self.draw_hint(strip, "Left/Right: change   OK: start");
    }

    fn draw_hint(&self, strip: &mut StripBuffer, text: &str) {
        let hint = Region::new(
            LARGE_MARGIN,
            SCREEN_H - BUTTON_BAR_H - self.ui_fonts.body.line_height - SECTION_GAP,
            FULL_CONTENT_W,
            self.ui_fonts.body.line_height,
        );
        BitmapLabel::new(hint, text, self.ui_fonts.body)
            .alignment(Alignment::Center)
            .draw(strip)
            .unwrap_infallible();
    }

    fn draw_loading(&self, strip: &mut StripBuffer) {
        let mut text = StackFmt::<32>::new();
        let name = match self.source {
            Source::Vocab(i) => VOCAB_DECKS[i],
            Source::Kana => "",
        };
        let _ = write!(text, "Loading {name} ...");
        let region = Region::new(0, 300, SCREEN_W, self.ui_fonts.heading.line_height);
        BitmapLabel::new(region, text.as_str(), self.ui_fonts.heading)
            .alignment(Alignment::Center)
            .draw(strip)
            .unwrap_infallible();
        self.draw_hint(strip, "Back: cancel");
    }

    fn draw_header(&self, strip: &mut StripBuffer, title: &str) {
        self.draw_header_counts(strip, title, self.known(), self.total());
    }

    fn draw_header_counts(&self, strip: &mut StripBuffer, title: &str, known: usize, total: usize) {
        let header_y = CONTENT_TOP + SECTION_GAP;
        let line_h = self.ui_fonts.body.line_height;
        let title_region = Region::new(LARGE_MARGIN, header_y, FULL_CONTENT_W - COUNT_W, line_h);
        BitmapLabel::new(title_region, title, self.ui_fonts.body)
            .draw(strip)
            .unwrap_infallible();
        let count_region =
            Region::new(SCREEN_W - LARGE_MARGIN - COUNT_W, header_y, COUNT_W, line_h);
        let mut count = StackFmt::<16>::new();
        let _ = write!(count, "{known}/{total}");
        BitmapLabel::new(count_region, count.as_str(), self.ui_fonts.body)
            .alignment(Alignment::CenterRight)
            .draw(strip)
            .unwrap_infallible();
    }

    fn draw_hint_for(&self, strip: &mut StripBuffer, slot: usize, flipped: bool) {
        if !flipped {
            let hint = match self.playing {
                Source::Kana if Card::from_slot(slot).reverse() => "OK: show kana",
                _ => "OK: show reading",
            };
            self.draw_hint(strip, hint);
            return;
        }
        let left = self.playing_progress().hits_to_known(slot);
        let mut hint = StackFmt::<48>::new();
        if left == 0 {
            let _ = write!(hint, "<< missed   OK: got it   known");
        } else {
            let _ = write!(hint, "<< missed   OK: got it   {left} to known");
        }
        self.draw_hint(strip, hint.as_str());
    }

    fn draw_card(&self, strip: &mut StripBuffer, slot: usize, card: Card, flipped: bool) {
        let mut title = StackFmt::<40>::new();
        let _ = write!(title, "{} - {}", card.script().label(), self.set.label());
        self.draw_header(strip, title.as_str());

        clear(strip, GLYPH_REGION);
        if card.reverse() && !flipped {
            BitmapLabel::new(GLYPH_REGION, card.romaji(), Self::romaji_font())
                .alignment(Alignment::Center)
                .draw(strip)
                .unwrap_infallible();
        } else {
            let view = self.card_fonts.view();
            let glyph = card.glyph();
            let width = text_width(&view, glyph, CARD_SCALE);
            let em = BODY_PX * CARD_SCALE;
            let top = GLYPH_REGION.y + GLYPH_REGION.h.saturating_sub(em) / 2;
            draw_text(
                strip,
                &view,
                glyph,
                CARD_SCALE,
                SCREEN_W.saturating_sub(width) / 2,
                top,
            );
        }

        let text = if flipped { card.romaji() } else { "" };
        BitmapLabel::new(ROMAJI_REGION, text, Self::romaji_font())
            .alignment(Alignment::Center)
            .draw(strip)
            .unwrap_infallible();
        self.draw_hint_for(strip, slot, flipped);
    }

    fn draw_word(&self, strip: &mut StripBuffer, slot: usize, entry: Entry<'_>, flipped: bool) {
        let deck = match self.playing {
            Source::Vocab(i) => VOCAB_DECKS[i],
            Source::Kana => "",
        };
        self.draw_header(strip, deck);

        let view = self.card_fonts.view();
        let max_w = SCREEN_W - 2 * LARGE_MARGIN;
        let head = entry.headword();
        let scale = if text_width(&view, head, WORD_SCALE) <= max_w {
            WORD_SCALE
        } else {
            1
        };
        clear(strip, WORD_REGION);
        let width = text_width(&view, head, scale).min(max_w);
        let top = WORD_REGION.y + WORD_REGION.h.saturating_sub(BODY_PX * scale) / 2;
        draw_text(strip, &view, head, scale, (SCREEN_W - width) / 2, top);

        clear(strip, WORD_BACK_REGION);
        if flipped {
            let mut top = WORD_BACK_REGION.y + SECTION_GAP;
            let mut spans = [pitch::Span::default(); pitch::MAX_MORAE];
            let morae = pitch::spans(
                entry.kana,
                |ch| view.advance(ch, Style::Regular),
                &mut spans,
            );
            let mut drops = [0u8; pitch::MAX_ACCENTS];
            let shown = pitch::accents(entry.pitch, morae, &mut drops);
            if entry.has_reading() || shown > 0 {
                let width = text_width(&view, entry.kana, 1).min(max_w);
                let left = (SCREEN_W - width) / 2;
                for (row, &drop) in drops[..shown].iter().enumerate() {
                    // the first accent sits nearest the reading
                    let up = (shown - 1 - row) as u16;
                    draw_pitch(strip, left, top + up * PITCH_H, &spans[..morae], drop);
                }
                top += shown as u16 * PITCH_H;
                draw_text(strip, &view, entry.kana, 1, left, top);
                top += TEXT_LINE_H + SECTION_GAP * 2;
            }
            let mut lines = [""; MAX_GLOSS_LINES];
            let n = wrap(&view, entry.gloss, max_w, &mut lines);
            for line in &lines[..n] {
                let width = text_width(&view, line, 1);
                draw_text(strip, &view, line, 1, (SCREEN_W - width) / 2, top);
                top += TEXT_LINE_H;
            }
        }
        self.draw_hint_for(strip, slot, flipped);
    }

    fn chart_glyph(&self, kana: &table::Kana) -> &'static str {
        if self.script == Script::Katakana {
            kana.katakana
        } else {
            kana.hiragana
        }
    }

    // How a chart cell stands in the chosen direction; mixed counts a kana as
    // known only when both faces are.
    fn chart_status(&self, face: usize) -> Status {
        let forward = self.progress.status(face);
        let reverse = self.progress.status(face + SLOTS);
        match self.direction {
            Direction::Forward => forward,
            Direction::Reverse => reverse,
            Direction::Mixed if forward == reverse => forward,
            Direction::Mixed => Status::Learning,
        }
    }

    fn draw_overview(&self, strip: &mut StripBuffer) {
        if let Some(error) = self.card_fonts.error {
            let region = Region::new(
                8,
                CONTENT_TOP,
                SCREEN_W - 16,
                self.ui_fonts.body.line_height,
            );
            crate::apps::widgets::bitmap_label::draw_surface_error(
                strip,
                region,
                self.ui_fonts.body,
                error,
            );
            return;
        }
        let line_h = self.ui_fonts.body.line_height;
        let body_top = CONTENT_TOP + SECTION_GAP * 2 + line_h;
        match self.source {
            Source::Kana => {
                let (new, learning, known) = self.progress.tally(self.deck.slots());
                let title = match self.script {
                    Script::Katakana => "Katakana",
                    _ => "Hiragana",
                };
                self.draw_header_counts(strip, title, known, self.deck.slots().len());
                self.draw_chart(strip, body_top);
                let mut line = StackFmt::<48>::new();
                let _ = write!(line, "new {new}   learning {learning}   known {known}");
                let y = body_top + CELL_H * chart_rows() + SECTION_GAP;
                BitmapLabel::new(
                    Region::new(LARGE_MARGIN, y, FULL_CONTENT_W, line_h),
                    line.as_str(),
                    self.ui_fonts.body,
                )
                .alignment(Alignment::Center)
                .draw(strip)
                .unwrap_infallible();
                self.draw_hint(strip, "Dot: new   Boxed: known   Back: close");
            }
            Source::Vocab(deck) => {
                let loaded = self.vocab_deck == Some(deck);
                let (new, learning, known) = self.vocab_progress.tally(&self.vocab_slots);
                self.draw_header_counts(strip, VOCAB_DECKS[deck], known, self.vocab_slots.len());
                if loaded {
                    let rows = [("New", new), ("Learning", learning), ("Known", known)];
                    for (i, (label, n)) in rows.into_iter().enumerate() {
                        let y = body_top + SECTION_GAP * 2 + i as u16 * ROW_STRIDE;
                        let mut count = StackFmt::<16>::new();
                        let _ = write!(count, "{n}");
                        BitmapLabel::new(
                            Region::new(LARGE_MARGIN, y, LABEL_W, ROW_H),
                            label,
                            self.ui_fonts.body,
                        )
                        .draw(strip)
                        .unwrap_infallible();
                        BitmapLabel::new(
                            Region::new(VALUE_X, y, VALUE_W, ROW_H),
                            count.as_str(),
                            self.ui_fonts.body,
                        )
                        .alignment(Alignment::CenterRight)
                        .draw(strip)
                        .unwrap_infallible();
                    }
                    let bar = Region::new(
                        LARGE_MARGIN,
                        body_top + SECTION_GAP * 2 + 3 * ROW_STRIDE + SECTION_GAP,
                        FULL_CONTENT_W,
                        24,
                    );
                    let total = self.vocab_slots.len().max(1);
                    let filled = (usize::from(bar.w) * known / total) as u16;
                    bar.to_rect()
                        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                        .draw(strip)
                        .unwrap_infallible();
                    Region::new(bar.x, bar.y, filled, bar.h)
                        .to_rect()
                        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                        .draw(strip)
                        .unwrap_infallible();
                } else {
                    BitmapLabel::new(
                        Region::new(LARGE_MARGIN, body_top, FULL_CONTENT_W, line_h),
                        "Start the deck once to see it",
                        self.ui_fonts.body,
                    )
                    .draw(strip)
                    .unwrap_infallible();
                }
                self.draw_hint(strip, "Back: close");
            }
        }
    }

    fn draw_chart(&self, strip: &mut StripBuffer, top: u16) {
        let view = self.card_fonts.view();
        let cell_w = FULL_CONTENT_W / CHART_COLS as u16;
        let katakana = usize::from(self.script == Script::Katakana);
        for (i, kana) in table::KANA.iter().enumerate() {
            let Some((row, col)) = chart_position(kana) else {
                continue;
            };
            let cell = Region::new(
                LARGE_MARGIN + col as u16 * cell_w,
                top + row as u16 * CELL_H,
                cell_w,
                CELL_H,
            );
            let status = self.chart_status((i << 1) | katakana);
            if status == Status::New {
                Region::new(cell.x + cell.w / 2 - 3, cell.y + cell.h / 2 - 3, 6, 6)
                    .to_rect()
                    .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                    .draw(strip)
                    .unwrap_infallible();
                continue;
            }
            let glyph = self.chart_glyph(kana);
            let width = text_width(&view, glyph, 1);
            draw_text(
                strip,
                &view,
                glyph,
                1,
                cell.x + cell.w.saturating_sub(width) / 2,
                cell.y + cell.h.saturating_sub(BODY_PX) / 2,
            );
            if status == Status::Known {
                Region::new(cell.x + 3, cell.y + 2, cell.w - 6, cell.h - 4)
                    .to_rect()
                    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                    .draw(strip)
                    .unwrap_infallible();
            }
        }
    }
}

const fn chart_rows() -> u16 {
    deck::CHART_ROWS as u16
}

// A file of `size` bytes from the deck directory; None on a short read or when
// the memory is not there (reserved first: a refusal must not abort).
#[allow(clippy::slow_vector_initialization)]
fn read_whole(k: &mut KernelHandle<'_>, name: &str, size: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).ok()?;
    bytes.resize(size, 0);
    let n = k
        .read_app_subdir_chunk(VOCAB_DIR, name, 0, &mut bytes)
        .ok()?;
    (n == size).then_some(bytes)
}

fn clear(strip: &mut StripBuffer, region: Region) {
    region
        .to_rect()
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
        .draw(strip)
        .unwrap_infallible();
}

fn magnified(view: &PreparedFonts<'_>, ch: char) -> bool {
    !view.font(Style::Regular).has_glyph(ch)
}

// The contour of accent `drop` over the morae `spans` (from x = 0), its top at
// `top`: a line at the high or low level over each mora, a stroke wherever the
// level changes. An accent on the last mora also falls after it, which is how
// a word that drops on its particle (尾高) differs from a flat one.
fn draw_pitch(strip: &mut StripBuffer, left: u16, top: u16, spans: &[pitch::Span], drop: u8) {
    let level = |high: bool| top + if high { PITCH_HIGH } else { PITCH_LOW };
    let mut bar = |x: u16, y: u16, w: u16, h: u16| {
        Region::new(left + x, y, w, h)
            .to_rect()
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(strip)
            .unwrap_infallible();
    };
    for (i, span) in spans.iter().enumerate() {
        let high = pitch::is_high(i, drop);
        bar(span.x0, level(high), span.x1 - span.x0, 2);
        let next = match spans.get(i + 1) {
            Some(_) => Some(pitch::is_high(i + 1, drop)),
            None if usize::from(drop) == spans.len() && high => Some(false),
            None => None,
        };
        if next.is_some_and(|n| n != high) {
            bar(
                span.x1.saturating_sub(1),
                level(true),
                2,
                PITCH_LOW - PITCH_HIGH + 2,
            );
        }
    }
}

fn text_width(view: &PreparedFonts<'_>, text: &str, scale: u16) -> u16 {
    text.chars()
        .map(|ch| view.advance(ch, Style::Regular) * if magnified(view, ch) { scale } else { 1 })
        .fold(0u16, u16::saturating_add)
}

fn draw_text(
    strip: &mut StripBuffer,
    view: &PreparedFonts<'_>,
    text: &str,
    scale: u16,
    left: u16,
    top: u16,
) {
    let mut x = i32::from(left);
    let baseline = i32::from(top + BODY_PX * scale * 7 / 8);
    for ch in text.chars() {
        x += if magnified(view, ch) && scale > 1 {
            i32::from(view.draw_char_scaled(
                strip,
                ch,
                Style::Regular,
                BinaryColor::On,
                x,
                baseline,
                scale,
            ))
        } else {
            i32::from(view.draw_char_fg(strip, ch, Style::Regular, BinaryColor::On, x, baseline))
        };
    }
}

fn wrap<'a>(view: &PreparedFonts<'_>, text: &'a str, max_w: u16, out: &mut [&'a str]) -> usize {
    let (mut n, mut start, mut width) = (0, 0, 0u16);
    for (i, ch) in text.char_indices() {
        let advance = view.advance(ch, Style::Regular);
        if width + advance > max_w && i > start {
            if n == out.len() {
                return n;
            }
            out[n] = &text[start..i];
            n += 1;
            start = i;
            width = 0;
        }
        width += advance;
    }
    if start < text.len() && n < out.len() {
        out[n] = &text[start..];
        n += 1;
    }
    n
}

impl App<AppId> for FlashcardsApp {
    fn on_enter(&mut self, ctx: &mut AppContext, k: &mut KernelHandle<'_>) {
        ctx.clear_message();
        if !self.kana_loaded {
            self.load_kana(k);
        }
        self.find_decks(k);
        self.release_vocab = false;
        self.state = State::Menu;
        self.confirm_reset = false;
        self.notice = None;
        self.row = 0;
        ctx.mark_dirty(CONTENT_REGION);
    }

    fn on_exit(&mut self) {
        self.card_fonts.clear();
        self.release_vocab = true;
    }

    fn on_event(&mut self, event: ActionEvent, ctx: &mut AppContext) -> Transition {
        match self.state {
            State::Menu => self.on_event_menu(event, ctx),
            State::Loading(_) => {
                if event == ActionEvent::Press(Action::Back) {
                    self.drop_vocab();
                    self.state = State::Menu;
                    ctx.mark_dirty(CONTENT_REGION);
                }
                Transition::None
            }
            State::Overview => self.on_event_overview(event, ctx),
            State::Card { slot, flipped } => self.on_event_card(slot, flipped, event, ctx),
        }
    }

    fn prepare_render(&mut self, _ctx: &mut AppContext, k: &mut KernelHandle<'_>) {
        let mut labels = VisibleText::new();
        let latin = fonts::body_font(CARD_FONT_IDX);
        if self.state == State::Overview && self.source == Source::Kana {
            for kana in table::KANA.iter().filter(|k| chart_position(k).is_some()) {
                labels.add(self.chart_glyph(kana), latin, false);
            }
        }
        if let State::Card { slot, .. } = self.state {
            match self.playing {
                Source::Kana => labels.add(Card::from_slot(slot).glyph(), latin, false),
                Source::Vocab(_) => {
                    if let Some(entry) = self.vocab.entry(slot) {
                        labels.add(entry.headword(), latin, false);
                        labels.add(entry.kana, latin, false);
                        labels.add(entry.gloss, latin, false);
                    }
                }
            }
        }
        self.card_fonts.prepare(k, &labels);
    }

    async fn background(&mut self, ctx: &mut AppContext, k: &mut KernelHandle<'_>) {
        if let (State::Loading(stage), Source::Vocab(deck)) = (self.state, self.source) {
            self.load_step(stage, deck, ctx, k);
        }
        self.save(k);
    }

    fn background_pending(&self) -> bool {
        matches!(self.state, State::Loading(_))
            || self.kana_dirty
            || self.vocab_dirty
            || self.reset_vocab.is_some()
    }

    fn has_background_when_suspended(&self) -> bool {
        self.kana_dirty || self.vocab_dirty || self.reset_vocab.is_some() || self.release_vocab
    }

    fn background_suspended(&mut self, k: &mut KernelHandle<'_>) {
        self.save(k);
        if self.release_vocab {
            self.drop_vocab();
        }
    }

    fn draw(&self, strip: &mut StripBuffer) {
        if let Some(error) = self.card_fonts.error {
            crate::apps::widgets::bitmap_label::draw_surface_error(
                strip,
                Region::new(
                    8,
                    CONTENT_TOP,
                    SCREEN_W - 16,
                    self.ui_fonts.body.line_height,
                ),
                self.ui_fonts.body,
                error,
            );
            return;
        }
        match (self.state, self.playing) {
            (State::Card { slot, flipped }, Source::Kana) => {
                self.draw_card(strip, slot, Card::from_slot(slot), flipped)
            }
            (State::Card { slot, flipped }, Source::Vocab(_)) => {
                if let Some(entry) = self.vocab.entry(slot) {
                    self.draw_word(strip, slot, entry, flipped);
                }
            }
            (State::Loading(_), _) => self.draw_loading(strip),
            (State::Overview, _) => self.draw_overview(strip),
            (State::Menu, _) => self.draw_menu(strip),
        }
    }
}

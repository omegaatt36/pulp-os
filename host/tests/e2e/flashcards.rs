// The flash cards, reached from Home and driven by keys; the Leitner state
// lands on the card.
use pulp_host::apps::AppId;
use pulp_host::apps::flashcards::PROGRESS_FILE;
use pulp_host::apps::flashcards::deck::{
    CHART_COLS, CHART_ROWS, Card, Deck, Direction, Groups, Script, Set, chart_position, group_key,
};
use pulp_host::apps::flashcards::progress::{FILE_LEN, PROGRESS_SLOTS, Progress, SLOTS, Status};
use pulp_host::apps::flashcards::table::KANA;
use pulp_host::storage::VirtualStorage;

use crate::cjk_support::pack_of;
use crate::device::{Device, Key, serial};

const CARD_PX: u16 = 35;

// a body-size pack holding every kana of the table, the way the SD pack does
// for the real font
fn card() -> VirtualStorage {
    let mut chars: Vec<char> = KANA
        .iter()
        .flat_map(|k| k.hiragana.chars().chain(k.katakana.chars()))
        .collect();
    chars.sort_unstable();
    chars.dedup();
    let card = VirtualStorage::memory_with(&[]);
    card.ensure_pulp_dir().unwrap();
    card.ensure_pulp_subdir("FONTS").unwrap();
    card.write_in_pulp_subdir(
        "FONTS",
        &format!("F{CARD_PX:05}.PFN"),
        &pack_of(CARD_PX, false, chars),
    )
    .unwrap();
    card
}

// Home -> the last entry (no Continue, no Upload on the host)
fn open_cards(d: &mut Device) {
    d.taps(&[Key::Next, Key::Next, Key::Next, Key::Enter]);
    assert_eq!(d.active(), AppId::Flashcards);
}

fn saved(d: &Device) -> Option<Progress> {
    Progress::decode(&d.pulp_file(PROGRESS_FILE)?)
}

fn sizes(script: Script, set: Set) -> usize {
    Deck::build(script, set, Direction::Forward, None)
        .slots()
        .len()
}

#[test]
fn home_lists_the_kana_cards_and_back_returns() {
    let _serial = serial();
    let mut d = Device::boot(card());
    open_cards(&mut d);
    d.tap(Key::Back);
    assert_eq!(d.active(), AppId::Home);
}

#[test]
fn ok_flips_a_card_and_an_answer_turns_to_the_next() {
    let _serial = serial();
    let mut d = Device::boot(card());
    open_cards(&mut d);
    let menu = d.screen().to_pbm();

    d.tap(Key::Enter);
    let front = d.screen().to_pbm();
    assert_ne!(front, menu, "starting shows the first card");

    d.tap(Key::Next);
    assert_eq!(
        d.screen().to_pbm(),
        front,
        "an unflipped card ignores the side keys"
    );

    d.tap(Key::Enter);
    let back = d.screen().to_pbm();
    assert_ne!(back, front, "OK shows the reading");

    d.tap(Key::Enter);
    let next = d.screen().to_pbm();
    assert_ne!(next, back, "an answer turns to the next card");
    assert_ne!(next, front, "the next card is another kana");

    d.tap(Key::Back);
    assert_eq!(d.screen().to_pbm().len(), menu.len());
    d.tap(Key::Back);
    assert_eq!(d.active(), AppId::Home);
}

#[test]
fn the_menu_changes_the_deck() {
    let _serial = serial();
    let mut d = Device::boot(card());
    open_cards(&mut d);
    let basic = d.screen().to_pbm();
    d.tap(Key::Right);
    assert_eq!(
        d.screen().to_pbm(),
        basic,
        "no deck on the card: Kana stays"
    );
    d.taps(&[Key::Next, Key::Right]);
    assert_ne!(d.screen().to_pbm(), basic, "the script row shows Katakana");
    d.taps(&[Key::Next, Key::Right]);
    assert_ne!(d.screen().to_pbm(), basic, "the cards row shows Voiced");
}

#[test]
fn answers_are_saved_and_survive_a_reboot() {
    let _serial = serial();
    let mut d = Device::boot(card());
    open_cards(&mut d);
    assert!(saved(&d).is_none(), "nothing is saved before an answer");

    d.taps(&[Key::Enter, Key::Enter, Key::Enter]); // start, flip, got it
    d.taps(&[Key::Enter, Key::Left]); // flip, missed
    let progress = saved(&d).expect("the answers are on the card");
    assert_eq!(progress.answers(), 2);
    assert_eq!(d.pulp_file(PROGRESS_FILE).map(|b| b.len()), Some(FILE_LEN));

    let mut d = d.reboot();
    open_cards(&mut d);
    d.taps(&[Key::Enter, Key::Enter, Key::Enter]);
    assert_eq!(saved(&d).unwrap().answers(), 3, "the count carries on");
}

#[test]
fn reset_asks_twice_and_only_then_wipes() {
    let _serial = serial();
    let mut d = Device::boot(card());
    open_cards(&mut d);
    d.taps(&[Key::Enter, Key::Enter, Key::Enter, Key::Back]);
    assert_eq!(saved(&d).unwrap().answers(), 1);

    menu_row(&mut d, RESET_ROW);
    d.tap(Key::Enter);
    assert_eq!(saved(&d).unwrap().answers(), 1, "one OK only asks");
    // any other key cancels the ask, so the next OK asks again
    d.taps(&[Key::Next, Key::Prev, Key::Enter]);
    assert_eq!(saved(&d).unwrap().answers(), 1, "the ask was cancelled");

    d.tap(Key::Enter);
    assert_eq!(saved(&d).unwrap().answers(), 0, "the second OK resets");
}

#[test]
fn a_damaged_progress_file_starts_over() {
    let _serial = serial();
    let card = card();
    card.write_in_pulp(PROGRESS_FILE, b"junk").unwrap();
    let mut d = Device::boot(card);
    open_cards(&mut d);
    d.taps(&[Key::Enter, Key::Enter, Key::Enter]);
    assert_eq!(saved(&d).unwrap().answers(), 1);
}

#[test]
fn deck_sizes_follow_the_table() {
    assert_eq!(sizes(Script::Hiragana, Set::Basic), 46);
    assert_eq!(sizes(Script::Katakana, Set::Basic), 46);
    assert_eq!(sizes(Script::Both, Set::Basic), 92);
    assert_eq!(sizes(Script::Hiragana, Set::Voiced), 25);
    assert_eq!(sizes(Script::Hiragana, Set::Contracted), 33);
    assert_eq!(sizes(Script::Hiragana, Set::Loanword), 23);
    assert_eq!(sizes(Script::Hiragana, Set::All), 104);
    assert_eq!(sizes(Script::Katakana, Set::All), 127);
    assert_eq!(sizes(Script::Both, Set::All), 104 + 127);
}

#[test]
fn hiragana_decks_hold_no_katakana() {
    for &slot in Deck::build(Script::Hiragana, Set::All, Direction::Forward, None).slots() {
        let g = pulp_host::apps::flashcards::deck::Card::from_slot(usize::from(slot)).glyph();
        assert!(
            g.chars().all(|c| ('\u{3041}'..='\u{3096}').contains(&c)),
            "{g}"
        );
    }
}

fn all_slots() -> Vec<u16> {
    Deck::build(Script::Both, Set::All, Direction::Forward, None)
        .slots()
        .to_vec()
}

#[test]
fn new_cards_come_in_a_seeded_order_without_repeating_the_last() {
    let slots = all_slots();
    let p = Progress::kana();
    let (a, b) = (
        p.pick(&slots, 1, None, false),
        p.pick(&slots, 2, None, false),
    );
    assert!(a.is_some() && b.is_some());
    assert_ne!(a, b, "other seeds pick other new cards");
    assert_eq!(a, p.pick(&slots, 1, None, false));
    assert_ne!(
        p.pick(&slots, 1, a, false),
        a,
        "the card just shown is avoided"
    );
    assert_eq!(
        p.pick(&slots[..1], 1, Some(usize::from(slots[0])), false),
        Some(usize::from(slots[0]))
    );
    assert_eq!(p.pick(&[], 1, None, false), None);
}

#[test]
fn a_missed_card_returns_before_the_new_ones() {
    let slots = all_slots();
    let mut p = Progress::kana();
    let missed = p.pick(&slots, 1, None, false).unwrap();
    p.answer(missed, false);
    // box 0 comes back after 4 answers; until then new cards are picked
    let mut shown = Vec::new();
    for seed in 0..4 {
        let s = p.pick(&slots, seed, Some(missed), false).unwrap();
        assert_ne!(s, missed);
        shown.push(s);
        p.answer(s, true);
    }
    assert_eq!(
        p.pick(&slots, 9, None, false),
        Some(missed),
        "due again after 4 answers"
    );
}

#[test]
fn hits_climb_the_boxes_and_a_miss_drops_to_zero() {
    let mut p = Progress::kana();
    let s = 5;
    assert!(!p.is_known(s));
    for _ in 0..2 {
        p.answer(s, true);
    }
    assert!(!p.is_known(s), "boxes 1 and 2 are still being learnt");
    p.answer(s, true);
    assert!(p.is_known(s), "the third hit reaches box 3");
    p.answer(s, false);
    assert!(!p.is_known(s));
}

#[test]
fn the_file_round_trips_and_refuses_what_is_not_its_own() {
    let mut p = Progress::kana();
    for s in [0, 7, 201] {
        p.answer(s, true);
    }
    p.answer(7, false);
    let mut out = [0u8; FILE_LEN];
    p.encode(&mut out);
    let q = Progress::decode(&out).unwrap();
    let mut again = [0u8; FILE_LEN];
    q.encode(&mut again);
    assert_eq!(out, again);
    assert_eq!(q.answers(), 4);

    assert!(Progress::decode(&out[..FILE_LEN - 1]).is_none(), "short");
    let mut bad = out;
    bad[0] = b'X';
    assert!(Progress::decode(&bad).is_none(), "magic");
    let mut bad = out;
    bad[8] = 6;
    assert!(Progress::decode(&bad).is_none(), "box out of range");
    assert_eq!(SLOTS, KANA.len() * 2);
    assert_eq!(PROGRESS_SLOTS, SLOTS * 2);
}

#[test]
fn an_old_progress_file_keeps_its_forward_cards() {
    // KNP1: magic, answers, SLOTS boxes, SLOTS dues
    let mut old = vec![0u8; 8 + 5 * SLOTS];
    old[..4].copy_from_slice(b"KNP1");
    old[4..8].copy_from_slice(&7u32.to_le_bytes());
    old[8..8 + SLOTS].fill(0xff);
    old[8 + 5] = 3;
    let p = Progress::decode(&old).expect("the old file is still read");
    assert_eq!(p.answers(), 7);
    assert_eq!(p.status(5), Status::Known);
    assert_eq!(
        p.status(5 + SLOTS),
        Status::New,
        "the reverse faces start empty"
    );

    let mut out = [0u8; FILE_LEN];
    p.encode(&mut out);
    assert_eq!(&out[..4], b"KNP2");
    assert_eq!(Progress::decode(&out).unwrap().status(5), Status::Known);
    old[8 + 5] = 6;
    assert!(Progress::decode(&old).is_none(), "box out of range");
}

#[test]
fn reverse_cards_sit_above_the_forward_ones() {
    let build = |d| Deck::build(Script::Hiragana, Set::Basic, d, None);
    let (fwd, rev, mix) = (
        build(Direction::Forward),
        build(Direction::Reverse),
        build(Direction::Mixed),
    );
    assert_eq!(
        (fwd.slots().len(), rev.slots().len(), mix.slots().len()),
        (46, 46, 92)
    );
    assert!(fwd.slots().iter().all(|&s| usize::from(s) < SLOTS));
    assert!(rev.slots().iter().all(|&s| usize::from(s) >= SLOTS));
    for (&f, &r) in fwd.slots().iter().zip(rev.slots()) {
        let (f, r) = (
            Card::from_slot(usize::from(f)),
            Card::from_slot(usize::from(r)),
        );
        assert!(!f.reverse() && r.reverse());
        assert_eq!((f.glyph(), f.romaji()), (r.glyph(), r.romaji()));
    }

    let mut p = Progress::kana();
    for _ in 0..3 {
        p.answer(5, true);
    }
    assert_eq!(p.status(5), Status::Known);
    assert_eq!(
        p.status(5 + SLOTS),
        Status::New,
        "the two ways are kept apart"
    );
}

#[test]
fn rows_split_a_set_without_losing_or_repeating_a_kana() {
    let basic = Groups::of(Set::Basic);
    assert_eq!(
        basic.keys(),
        ["", "k", "s", "t", "n", "h", "m", "y", "r", "w"],
        "ん joins the na row, shi/chi/tsu/fu stay in theirs"
    );
    let rows = |set, key: &str| {
        Deck::build(Script::Hiragana, set, Direction::Forward, Some(key))
            .slots()
            .len()
    };
    assert_eq!((rows(Set::Basic, "s"), rows(Set::Basic, "n")), (5, 6));
    assert_eq!(rows(Set::Basic, "z"), 0, "a row outside the set is empty");

    for set in [
        Set::Basic,
        Set::Voiced,
        Set::Contracted,
        Set::Loanword,
        Set::All,
    ] {
        let groups = Groups::of(set);
        assert!(!groups.keys().is_empty());
        let total: usize = groups
            .keys()
            .iter()
            .map(|key| {
                Deck::build(Script::Katakana, set, Direction::Forward, Some(key))
                    .slots()
                    .len()
            })
            .sum();
        assert_eq!(total, sizes(Script::Katakana, set), "{set:?}");
    }
    assert!(KANA.iter().all(|k| group_key(k) != "shi"));
}

#[test]
fn the_chart_gives_each_basic_kana_a_cell_of_its_own() {
    let mut seen = std::collections::HashSet::new();
    for kana in &KANA {
        match chart_position(kana) {
            Some((row, col)) => {
                assert!(row < CHART_ROWS && col < CHART_COLS, "{}", kana.romaji);
                assert!(seen.insert((row, col)), "{} shares a cell", kana.romaji);
            }
            None => assert_ne!(kana.kind, pulp_host::apps::flashcards::table::Kind::Seion),
        }
    }
    assert_eq!(seen.len(), 46);
}

#[test]
fn weak_only_drills_what_was_missed_and_nothing_else() {
    let slots = all_slots();
    let mut p = Progress::kana();
    assert_eq!(
        p.pick(&slots, 1, None, true),
        None,
        "nothing seen, nothing weak"
    );

    p.answer(4, false);
    p.answer(9, false);
    for _ in 0..3 {
        p.answer(20, true);
    }
    for seed in 0..8 {
        let s = p.pick(&slots, seed, None, true).unwrap();
        assert!(s == 4 || s == 9, "a new or a known card came up: {s}");
    }
    assert_eq!(
        p.pick(&slots, 1, Some(4), true),
        Some(9),
        "the last one is skipped"
    );

    for _ in 0..3 {
        p.answer(9, true);
    }
    assert_eq!(
        p.pick(&slots, 1, Some(4), true),
        Some(4),
        "the last weak card is still offered"
    );
    for _ in 0..3 {
        p.answer(4, true);
    }
    assert_eq!(p.pick(&slots, 1, Some(4), true), None);
    assert_eq!(p.tally(&slots), (slots.len() - 3, 0, 3));
}

// rows of the menu: Source, Script, Cards, Rows, Direction, Focus, Overview, Progress
const RESET_ROW: usize = 7;

fn menu_row(d: &mut Device, row: usize) {
    for _ in 0..row {
        d.tap(Key::Next);
    }
}

#[test]
fn the_direction_row_asks_the_romaji_first() {
    let _serial = serial();
    let mut d = Device::boot(card());
    open_cards(&mut d);
    menu_row(&mut d, 4);
    d.tap(Key::Right); // Romaji > Kana
    d.taps(&[Key::Enter, Key::Enter, Key::Enter]); // start, flip, got it
    let p = saved(&d).expect("the answer is on the card");
    let slots: Vec<u16> = (0..PROGRESS_SLOTS as u16).collect();
    let (fwd, rev) = (slots[..SLOTS].to_vec(), slots[SLOTS..].to_vec());
    assert_eq!(p.tally(&fwd).1, 0, "no forward card was asked");
    assert_eq!(p.tally(&rev).1, 1, "one reverse card was answered");
}

#[test]
fn weak_only_ends_in_the_menu_when_nothing_is_left() {
    let _serial = serial();
    let mut d = Device::boot(card());
    open_cards(&mut d);
    d.taps(&[Key::Enter, Key::Enter, Key::Left]); // start, flip, missed
    d.tap(Key::Back);
    menu_row(&mut d, 5);
    d.tap(Key::Right); // Focus: Weak only
    d.tap(Key::Enter);
    for _ in 0..3 {
        d.taps(&[Key::Enter, Key::Enter]); // flip, got it: the one weak card climbs
    }
    d.tap(Key::Back);
    assert_eq!(
        d.active(),
        AppId::Home,
        "no weak card was left, so the session had already returned to the menu"
    );
}

#[test]
fn the_overview_opens_from_the_menu_and_closes_back_to_it() {
    let _serial = serial();
    let mut d = Device::boot(card());
    open_cards(&mut d);
    let menu = d.screen().to_pbm();
    menu_row(&mut d, 6);
    let selected = d.screen().to_pbm();
    d.tap(Key::Enter);
    let overview = d.screen().to_pbm();
    assert_ne!(overview, selected);
    assert_ne!(overview, menu);
    d.tap(Key::Back);
    assert_eq!(d.screen().to_pbm(), selected, "Back returns to the menu");
    d.tap(Key::Back);
    assert_eq!(d.active(), AppId::Home);
}

// ---- vocabulary decks: made-up words, the format is all the tests know ----

use pulp_host::apps::flashcards::vocab::{Vocab, parse_line};

const DECK: &str = "# a deck of the tester's own\n\
猫\tねこ\t[名]貓\n\
犬\tいぬ\t[名]狗\r\n\
\n\
水\tみず\t[名]水\textra\tcolumns\n\
ぱん\tぱん\t[名]麵包\n\
no tabs here\n\
鳥\t\tno reading\n\
本\tほん\t[名]書\n";

fn deck_card(deck: &str) -> VirtualStorage {
    let base = card();
    let mut chars: Vec<char> = KANA
        .iter()
        .flat_map(|k| k.hiragana.chars().chain(k.katakana.chars()))
        .chain(deck.chars().filter(|c| *c >= ' ' && !c.is_ascii()))
        .collect();
    chars.sort_unstable();
    chars.dedup();
    base.write_in_pulp_subdir(
        "FONTS",
        &format!("F{CARD_PX:05}.PFN"),
        &pack_of(CARD_PX, false, chars),
    )
    .unwrap();
    base.ensure_pulp_subdir("VOCAB").unwrap();
    base.write_in_pulp_subdir("VOCAB", "N5.TSV", deck.as_bytes())
        .unwrap();
    base
}

fn vocab_file(d: &Device, name: &str) -> Option<Vec<u8>> {
    let size = d.card().file_size_in_pulp_subdir("VOCAB", name).ok()? as usize;
    let mut bytes = vec![0; size];
    let n = d
        .card()
        .read_chunk_in_pulp_subdir("VOCAB", name, 0, &mut bytes)
        .ok()?;
    bytes.truncate(n);
    Some(bytes)
}

fn deck_hashes(deck: &str) -> Vec<u32> {
    deck.lines()
        .filter_map(parse_line)
        .map(|e| e.hash())
        .collect()
}

#[test]
fn lines_are_read_leniently_and_only_usable_ones_count() {
    let words: Vec<_> = DECK.lines().filter_map(parse_line).collect();
    let kanji: Vec<_> = words.iter().map(|w| w.kanji).collect();
    assert_eq!(kanji, ["猫", "犬", "水", "ぱん", "本"]);
    assert_eq!(words[1].gloss, "[名]狗", "a CRLF line ends clean");
    assert_eq!(
        words[2].gloss, "[名]水",
        "columns past the third are ignored"
    );
    assert!(parse_line("# comment").is_none());
    assert!(parse_line("").is_none());
    assert!(parse_line("only one column").is_none());
    assert!(parse_line("猫\t\t貓").is_none(), "a word needs a reading");
    let kana_only = parse_line("ぱん\tぱん\t麵包").unwrap();
    assert_eq!(kana_only.headword(), "ぱん");
    assert!(!kana_only.has_reading());
    assert!(words[0].has_reading());
}

#[test]
fn a_deck_is_indexed_around_bad_lines() {
    let mut text = DECK.as_bytes().to_vec();
    text.extend_from_slice(b"\xff\xfe\tbad\tutf8\n");
    text.extend_from_slice("葉\tは\t[名]葉".as_bytes()); // no final newline
    let mut v = Vocab::new();
    assert!(v.reserve(text.len()));
    // read in two windows, as the app does from the card
    let mut at = 0;
    for max in [8, text.len()] {
        at += v.fill(max, |b| {
            let n = b.len();
            b.copy_from_slice(&text[at..at + n]);
            n
        });
    }
    assert_eq!(at, text.len());
    v.index();
    assert_eq!(
        v.len(),
        6,
        "five good lines, the bad one skipped, the last unterminated"
    );
    assert_eq!(v.entry(5).unwrap().kanji, "葉");
    assert!(v.entry(6).is_none());
    assert_eq!(v.hashes().len(), 6);
    assert!(!v.reserve(vocab_too_big()), "over the cap is refused");
}

fn vocab_too_big() -> usize {
    pulp_host::apps::flashcards::vocab::MAX_BYTES + 1
}

#[test]
fn progress_follows_a_word_when_the_deck_is_edited() {
    let before = deck_hashes(DECK);
    let mut p = Progress::new(before.len());
    p.answer(1, true); // 犬
    p.answer(1, true);
    p.answer(1, true);
    assert!(p.is_known(1));
    let bytes = p.words_file(&before).unwrap();

    let edited = deck_hashes("新\tあたらしい\t[形]新\n犬\tいぬ\t[名]狗\n猫\tねこ\t[名]貓\n");
    let q = Progress::from_words(&bytes, &edited).unwrap();
    assert!(q.is_known(1), "犬 is now the second line and still known");
    assert!(!q.is_known(0) && !q.is_known(2));
    assert_eq!(q.answers(), 3);
    assert!(Progress::from_words(&bytes[..bytes.len() - 1], &edited).is_none());
    assert!(Progress::from_words(b"KNV1", &edited).is_none());
}

#[test]
fn a_deck_on_the_card_becomes_a_source_and_keeps_progress() {
    let _serial = serial();
    let mut d = Device::boot(deck_card(DECK));
    open_cards(&mut d);
    let menu = d.screen().to_pbm();
    d.tap(Key::Right);
    let source = d.screen().to_pbm();
    assert_ne!(source, menu, "N5 is offered as a source");

    d.tap(Key::Enter);
    let front = d.screen().to_pbm();
    assert_ne!(front, source, "the deck loaded and shows a word");
    d.tap(Key::Enter);
    let back = d.screen().to_pbm();
    assert_ne!(back, front, "OK shows the reading and the gloss");
    assert!(
        vocab_file(&d, "N5.PRG").is_none(),
        "nothing saved before an answer"
    );

    d.tap(Key::Enter); // got it
    let hashes = deck_hashes(DECK);
    let saved = Progress::from_words(&vocab_file(&d, "N5.PRG").unwrap(), &hashes).unwrap();
    assert_eq!(saved.answers(), 1);
    assert!(saved.known_count(&(0..5).collect::<Vec<u16>>()) == 0);

    // leave, come back: the count carries on from the file
    d.taps(&[Key::Back, Key::Back, Key::Back]);
    assert_eq!(d.active(), AppId::Home);
    let mut d = d.reboot();
    open_cards(&mut d);
    d.taps(&[Key::Right, Key::Enter, Key::Enter, Key::Left]);
    let saved = Progress::from_words(&vocab_file(&d, "N5.PRG").unwrap(), &hashes).unwrap();
    assert_eq!(saved.answers(), 2);
}

#[test]
fn vocabulary_progress_resets_apart_from_the_kana() {
    let _serial = serial();
    let mut d = Device::boot(deck_card(DECK));
    open_cards(&mut d);
    d.taps(&[Key::Enter, Key::Enter, Key::Enter, Key::Back]); // a kana answer
    d.taps(&[Key::Right, Key::Enter, Key::Enter, Key::Enter, Key::Back]); // a word answer
    assert!(vocab_file(&d, "N5.PRG").is_some());

    menu_row(&mut d, RESET_ROW);
    d.taps(&[Key::Enter, Key::Enter]);
    assert!(
        vocab_file(&d, "N5.PRG").is_none(),
        "the deck's progress is gone"
    );
    assert_eq!(saved(&d).unwrap().answers(), 1, "the kana's is not");
}

#[test]
fn an_empty_or_unreadable_deck_leaves_the_menu_standing() {
    let _serial = serial();
    let mut d = Device::boot(deck_card("# nothing but a comment\n"));
    open_cards(&mut d);
    d.tap(Key::Right);
    let source = d.screen().to_pbm();
    d.tap(Key::Enter);
    assert_eq!(d.active(), AppId::Flashcards);
    assert_ne!(d.screen().to_pbm(), source, "a notice says why");
    assert!(vocab_file(&d, "N5.PRG").is_none());
    d.tap(Key::Back);
    assert_eq!(d.active(), AppId::Home, "Back leaves from the menu");
}

// ---- pitch accent: the deck's fourth column ----

use pulp_host::apps::flashcards::pitch::{self, MAX_ACCENTS, MAX_MORAE, Span};

fn shape(morae: usize, drop: u8) -> String {
    (0..morae)
        .map(|i| if pitch::is_high(i, drop) { 'H' } else { 'L' })
        .collect()
}

#[test]
fn the_four_accent_types_have_their_contours() {
    assert_eq!(shape(3, 0), "LHH", "平板: low, then high without a fall");
    assert_eq!(shape(3, 1), "HLL", "頭高: high first, then down");
    assert_eq!(shape(4, 2), "LHLL", "中高: up, then down in the middle");
    assert_eq!(shape(3, 3), "LHH", "尾高: flat until the particle");
    assert_eq!(shape(2, 2), "LH", "橋 and 蛙 differ only in the fall after");
    assert_eq!(shape(1, 1), "H");
    assert_eq!(shape(1, 0), "L");
}

#[test]
fn small_kana_share_the_mora_before_them() {
    let advance = |ch: char| {
        if "ゃゅょァィゥェォ".contains(ch) {
            20
        } else {
            35
        }
    };
    let mut spans = [Span::default(); MAX_MORAE];
    let n = pitch::spans("きょうフォーク", advance, &mut spans);
    assert_eq!(n, 5, "きょ う フォ ー ク");
    assert_eq!((spans[0].x0, spans[0].x1), (0, 55), "きょ");
    assert_eq!(spans[1].x0, 55, "う follows きょ");
    assert_eq!(spans[3].x1 - spans[3].x0, 35, "ー is a mora of its own");

    let too_long = "あ".repeat(MAX_MORAE + 1);
    assert_eq!(pitch::spans(&too_long, advance, &mut spans), 0);
}

#[test]
fn only_accents_that_fit_the_reading_are_kept() {
    let mut out = [0; MAX_ACCENTS];
    assert_eq!(pitch::accents("0,2", 3, &mut out), 2);
    assert_eq!(&out[..2], [0, 2]);
    assert_eq!(
        pitch::accents("4,x, 1", 3, &mut out),
        1,
        "past the last mora, or not a number"
    );
    assert_eq!(out[0], 1);
    assert_eq!(pitch::accents("", 3, &mut out), 0);
    assert_eq!(
        pitch::accents("0,1,2,3", 4, &mut out),
        MAX_ACCENTS,
        "two contours at most"
    );
}

#[test]
fn the_pitch_is_the_fourth_column() {
    let e = parse_line("橋\tはし\t[名]橋\t2\tmore").unwrap();
    assert_eq!((e.gloss, e.pitch), ("[名]橋", "2"));
    assert_eq!(parse_line("橋\tはし\t[名]橋").unwrap().pitch, "");
}

fn flipped_word(deck: &str) -> Vec<u8> {
    let mut d = Device::boot(deck_card(deck));
    open_cards(&mut d);
    d.tap(Key::Right);
    d.taps(&[Key::Enter, Key::Enter]);
    d.screen().to_pbm()
}

#[test]
fn a_flipped_word_draws_its_pitch_over_the_reading() {
    let _serial = serial();
    let plain = flipped_word("橋\tはし\t[名]橋\n");
    let flat = flipped_word("橋\tはし\t[名]橋\t0\n");
    let tail = flipped_word("橋\tはし\t[名]橋\t2\n");
    assert_ne!(flat, plain, "the contour is drawn");
    assert_ne!(flat, tail, "0 and 2 differ on a two-mora word");
    assert_eq!(
        flipped_word("橋\tはし\t[名]橋\t9\n"),
        plain,
        "an accent past the last mora draws nothing"
    );
    assert_ne!(
        flipped_word("ぱん\tぱん\t[名]麵包\t1\n"),
        flipped_word("ぱん\tぱん\t[名]麵包\n"),
        "a kana-only word shows its reading when it has a pitch"
    );
}

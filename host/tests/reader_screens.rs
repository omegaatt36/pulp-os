use pulp_host::fixtures::{Spec, standard};
use pulp_host::reader::{Action, Phase, Rig};
use pulp_host::render::{render_full, render_stitched};
use pulp_host::storage::VirtualStorage;

fn screen(r: &Rig) -> Vec<u8> {
    let full = render_full(&|s| r.draw(s)).frame;
    let stitched = render_stitched(&|s| r.draw(s)).frame;
    let pbm = full.to_pbm();
    assert_eq!(pbm, stitched.to_pbm());
    let header = b"P4\n480 800\n";
    assert!(pbm.starts_with(header));
    assert_eq!(pbm.len(), header.len() + 480 * 800 / 8);
    assert!(full.black_count() > 0);
    assert!(full.black_count() < 480 * 800);
    assert_eq!(pbm, render_full(&|s| r.draw(s)).frame.to_pbm());
    pbm
}

#[test]
fn txt_and_epub_screens_are_deterministic_across_navigation_and_reopen() {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _serial = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixtures = standard();
    // One generated TXT and all six EPUB2/3 compression variants.
    let txt = fixtures.iter().find(|f| f.name == "PLAINLF.TXT").unwrap();
    for f in std::iter::once(txt).chain(fixtures.iter().filter(|f| matches!(f.spec, Spec::Epub(_)))) {
        let card = VirtualStorage::memory_with(&[(f.name, &f.bytes)]);
        card.ensure_pulp_dir().unwrap();
        let mut r = Rig::new(card);
        r.configure(2, 1);
        r.open(f.name);
        assert_eq!(r.phase(), Phase::Ready, "{}", f.name);
        for _ in 0..400 {
            if !r.has_bg_work() { break; }
            r.idle(1);
        }
        assert!(!r.has_bg_work(), "{} cache completes", f.name);
        r.exit();
        r.open(f.name);
        let first = screen(&r);
        r.press(Action::Next);
        assert_eq!(r.phase(), Phase::Ready);
        assert_ne!(screen(&r), first, "{} next page changes screen", f.name);
        r.press(Action::Prev);
        assert_eq!(r.phase(), Phase::Ready);
        assert_eq!(screen(&r), first);
        if matches!(f.spec, Spec::Epub(_)) {
            let mut saw_image = false;
            for _ in 0..3000 {
                if r.page_image().is_some() {
                    screen(&r);
                    saw_image = true;
                    break;
                }
                let pos = (r.chapter(), r.page());
                r.press(Action::Next);
                assert_eq!(r.phase(), Phase::Ready);
                if pos == (r.chapter(), r.page()) { break; }
            }
            assert!(saw_image, "{} has a rendered image page", f.name);
        }
        r.exit();
        r.open(f.name);
        assert_eq!(r.phase(), Phase::Ready);
        assert_eq!(screen(&r), first, "{} reopen", f.name);
    }
}

//! Production C61 metadata policy, exercised over the host allocation seam.
extern crate alloc;
extern crate pulp_host as pulp_kernel;
pub use pulp_host::kernel;
#[path = "../../src/apps/reader/metadata.rs"]
#[allow(dead_code)]
mod metadata;

use metadata::ClassZipStorage;
use pulp_board_logic::memory::{PsramFault, PsramStatus};
use smol_epub::epub::{self, EpubSpine, TocEntry, TocSource};
use smol_epub::zip::{ZipEntry, ZipIndex, ZipStorage};

fn directory(names: &[String]) -> Vec<u8> {
    let mut cd = Vec::new();
    for (idx, name) in names.iter().enumerate() {
        let mut header = [0u8; 46];
        header[..4].copy_from_slice(&0x02014b50u32.to_le_bytes());
        header[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        header[42..46].copy_from_slice(&(idx as u32).to_le_bytes());
        cd.extend_from_slice(&header);
        cd.extend_from_slice(name.as_bytes());
    }
    cd
}
fn entries(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("book/chapter{i}.xhtml")).collect()
}

#[test]
fn profile_capacity_requires_a_validated_large_chip() {
    for status in [
        PsramStatus::NotInitialised,
        PsramStatus::Degraded(PsramFault::NotDetected),
        PsramStatus::Ready {
            bytes: 2 * 1024 * 1024,
        },
        PsramStatus::Ready {
            bytes: 8 * 1024 * 1024 - 1,
        },
    ] {
        assert_eq!(metadata::metadata_capacity(status), 256);
    }
    assert_eq!(
        metadata::metadata_capacity(PsramStatus::Ready {
            bytes: 8 * 1024 * 1024
        }),
        512
    );
}

#[test]
fn c61_owner_is_small_lazy_and_releases_backing_on_reset() {
    assert!(core::mem::size_of::<ZipIndex<ClassZipStorage>>() < 128);
    let mut storage = ClassZipStorage::new(512);
    assert!(storage.entries().is_empty());
    assert!(storage.names().is_empty());
    storage.prepare(512, 1000).unwrap();
    assert_eq!(storage.entries().len(), 512);
    assert_eq!(storage.names().len(), 1000);
    storage.clear();
    assert!(storage.entries().is_empty());
    assert!(storage.names().is_empty());
}

#[test]
fn strict_capacity_refuses_overflow_and_forgets_old_source() {
    for capacity in [256, 512] {
        let mut index = ZipIndex::with_storage(ClassZipStorage::new(capacity));
        index
            .parse_central_directory(&directory(&entries(capacity)))
            .unwrap();
        assert_eq!(index.count(), capacity);
        assert_eq!(index.find_icase("BOOK/CHAPTER0.XHTML"), Some(0));
        assert_eq!(
            index.parse_central_directory(&directory(&entries(capacity + 1))),
            Err("zip: entry capacity exceeded")
        );
        assert_eq!(index.count(), 0);
        assert_eq!(index.find("book/chapter0.xhtml"), None);
        index
            .parse_central_directory(&directory(&["other.xhtml".into()]))
            .unwrap();
        assert_eq!(index.count(), 1);
        assert_eq!(index.entry_name(0), "other.xhtml");
        index.clear();
        assert_eq!(index.count(), 0);
    }
}

#[test]
fn names_stay_representable_and_malformed_archives_leave_no_index() {
    let mut index = ZipIndex::with_storage(ClassZipStorage::new(512));
    index
        .parse_central_directory(&directory(&["a".repeat(65535)]))
        .unwrap();
    assert_eq!(index.entry(0).name_len, u16::MAX);
    assert_eq!(
        index.parse_central_directory(&directory(&["a".repeat(65535), "b".into()])),
        Err("zip: name pool overflow")
    );
    assert_eq!(index.count(), 0);
    let valid = directory(&entries(2));
    for malformed in [
        valid[..valid.len() - 1].to_vec(),
        [valid.clone(), vec![0]].concat(),
        vec![0; 46],
    ] {
        index.parse_central_directory(&valid).unwrap();
        assert!(index.parse_central_directory(&malformed).is_err());
        assert_eq!(index.count(), 0);
    }
}

struct RefusingStorage {
    inner: ClassZipStorage,
    fail_names: bool,
}
impl ZipStorage for RefusingStorage {
    fn capacity(&self) -> usize {
        512
    }
    fn prepare(&mut self, entries: usize, _: usize) -> Result<(), &'static str> {
        if self.fail_names {
            self.inner.prepare(entries, 0)?;
        }
        Err("injected allocation failure")
    }
    fn entries(&self) -> &[ZipEntry] {
        self.inner.entries()
    }
    fn entries_mut(&mut self) -> &mut [ZipEntry] {
        self.inner.entries_mut()
    }
    fn names(&self) -> &[u8] {
        self.inner.names()
    }
    fn names_mut(&mut self) -> &mut [u8] {
        self.inner.names_mut()
    }
    fn clear(&mut self) {
        self.inner.clear();
    }
}
#[test]
fn entry_and_name_allocation_failures_never_publish_partial_indices() {
    for fail_names in [false, true] {
        let mut index = ZipIndex::with_storage(RefusingStorage {
            inner: ClassZipStorage::new(512),
            fail_names,
        });
        assert_eq!(
            index.parse_central_directory(&directory(&entries(2))),
            Err("injected allocation failure")
        );
        assert_eq!(index.count(), 0);
        assert!(index.find("book/chapter0.xhtml").is_none());
    }
}

#[test]
fn class_backed_toc_initialization_titles_and_limit_match_parsers() {
    let mut index = ZipIndex::with_storage(ClassZipStorage::new(512));
    index
        .parse_central_directory(&directory(&["book/chapter0.xhtml".into()]))
        .unwrap();
    let mut spine = EpubSpine::new();
    let mut meta = epub::EpubMeta::new();
    let opf = br#"<package><metadata><dc:title>Book Title</dc:title></metadata><manifest><item id="c" href="chapter0.xhtml"/></manifest><spine><itemref idref="c"/></spine></package>"#;
    epub::parse_opf(opf, "book", &index, &mut meta, &mut spine).unwrap();
    assert_eq!(meta.title_str(), "Book Title");
    assert_eq!(spine.len(), 1);
    for capacity in [256, 512] {
        let mut toc = metadata::class_toc(capacity).unwrap();
        assert!(core::mem::size_of_val(&toc) < 64);
        assert!(
            toc.entries
                .iter()
                .all(|entry| entry.spine_idx == TocEntry::EMPTY.spine_idx)
        );
        let mut nav = String::from("<nav epub:type=\"toc\"><ol>");
        for _ in 0..capacity + 1 {
            nav.push_str("<li><a href=\"chapter0.xhtml\">Chapter 臺</a></li>");
        }
        nav.push_str("</ol></nav>");
        epub::parse_toc(
            TocSource::Nav(0),
            nav.as_bytes(),
            "book",
            &spine,
            &index,
            &mut toc,
        );
        assert_eq!(toc.len(), capacity);
        assert_eq!(toc.entries[0].title_str(), "Chapter 臺");
        assert_eq!(toc.entries[0].spine_idx, 0);
        epub::parse_toc(
            TocSource::Nav(0),
            b"<nav epub:type=\"toc\"><a href=\"chapter0.xhtml\">Replacement</a></nav>",
            "book",
            &spine,
            &index,
            &mut toc,
        );
        assert_eq!(toc.len(), 1);
        assert_eq!(toc.entries[0].title_str(), "Replacement");
    }
}

#[test]
fn toc_allocation_refusal_is_recoverable() {
    assert!(metadata::class_toc(usize::MAX).is_err());
}

#[test]
fn reader_replacement_discards_old_metadata_and_rejects_stale_quick_actions() {
    use pulp_host::fixtures::{
        Block, Chapter, Compression, EpubSpec, EpubVersion, Run, TocItem, build_epub,
    };
    use pulp_host::reader::{Phase, QA_NEXT_CHAPTER, QA_PREV_CHAPTER, QA_TOC, Rig};
    use pulp_host::storage::VirtualStorage;
    let spec = EpubSpec {
        version: EpubVersion::V3,
        title: "First source".into(),
        author: "Author".into(),
        identifier: "metadata-reset".into(),
        chapters: vec![
            Chapter {
                title: "Chapter".into(),
                blocks: vec![Block::Paragraph(vec![Run::Text("Reader content.".into())])],
            },
            Chapter {
                title: "Second chapter".into(),
                blocks: vec![Block::Paragraph(vec![Run::Text(
                    "Second source chapter.".into(),
                )])],
            },
        ],
        toc: vec![TocItem {
            title: "First TOC".into(),
            chapter: 0,
            children: vec![],
        }],
        images: vec![],
        cover: None,
        compression: Compression::Stored,
        numeric_entities: false,
    };
    let first = build_epub(&spec).unwrap();
    let card = VirtualStorage::memory_with(&[("first.epub", &first), ("invalid.epub", b"bad")]);
    card.ensure_pulp_dir().unwrap();
    let mut reader = Rig::new(card);
    reader.open("first.epub");
    assert_eq!(reader.phase(), Phase::Ready);
    assert_eq!(reader.toc_entries(), vec![("First TOC".into(), 0)]);
    assert_eq!(reader.epub_title(), "First source");
    assert_eq!(reader.spine_len(), 2);
    assert!(reader.quick_action_ids().contains(&QA_NEXT_CHAPTER));
    reader.enter("invalid.epub"); // deliberately no on_exit or background step
    assert_eq!(reader.phase(), Phase::Loading);
    assert_eq!(reader.spine_len(), 0);
    assert_eq!(reader.epub_title(), "");
    assert_eq!(reader.epub_author(), "");
    assert!(reader.toc_entries().is_empty());
    for id in [QA_NEXT_CHAPTER, QA_PREV_CHAPTER, QA_TOC] {
        assert!(!reader.quick_action_ids().contains(&id));
    }
    reader.quick_trigger(QA_NEXT_CHAPTER); // retained menu ID before initialization
    assert_eq!(reader.phase(), Phase::Error);
    assert_eq!(reader.chapter(), 0);
    assert!(reader.toc_entries().is_empty());
    reader.idle(8);
    for id in [QA_NEXT_CHAPTER, QA_PREV_CHAPTER, QA_TOC] {
        assert!(!reader.quick_action_ids().contains(&id));
        reader.quick_trigger(id);
        assert_eq!(reader.phase(), Phase::Error);
        assert_eq!(reader.chapter(), 0);
        assert_eq!(reader.spine_len(), 0);
        assert_eq!(reader.epub_title(), "");
    }
    reader.enter("first.epub");
    reader.quick_trigger(QA_NEXT_CHAPTER); // valid replacement still starts at chapter 0
    assert_eq!(reader.phase(), Phase::Ready);
    assert_eq!(reader.chapter(), 0);
    assert_eq!(reader.epub_title(), "First source");
    assert_eq!(reader.toc_entries(), vec![("First TOC".into(), 0)]);
    reader.quick_trigger(QA_NEXT_CHAPTER);
    assert_eq!(reader.chapter(), 1);
    reader.exit();
    assert!(reader.toc_entries().is_empty());
}

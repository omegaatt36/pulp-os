use pulp_board_logic::ssd1677::{
    RenderState, Rotation, Snapshot, WindowSource, align_partial_region, capture_snapshot,
};
use pulp_board_logic::strip::StripCore;
use pulp_host::kernel::{BigBuf, BufClass};

struct CountingPattern {
    calls: usize,
    seed: u8,
    buf: Vec<u8>,
}

impl CountingPattern {
    fn new(seed: u8) -> Self {
        Self {
            calls: 0,
            seed,
            buf: Vec::new(),
        }
    }
}

impl WindowSource for CountingPattern {
    fn render_window(
        &mut self,
        _rotation: Rotation,
        _px: u16,
        _py: u16,
        pw: u16,
        rows: u16,
    ) -> &[u8] {
        self.calls += 1;
        let len = (pw as usize / 8) * rows as usize;
        self.buf.clear();
        for i in 0..len {
            self.buf.push(self.seed.wrapping_add(i as u8));
        }
        self.seed = self.seed.wrapping_add(len as u8);
        &self.buf
    }
}

#[test]
fn display_frame_snapshot_full_48000_bytes_equality() {
    // 800x480 full panel at 1bpp = 48,000 bytes
    let rs = RenderState {
        px: 0,
        py: 0,
        pw: 800,
        ph: 480,
        left_mask: 0,
        right_mask: 0,
    };
    let needed = (rs.pw as usize / 8) * (rs.ph as usize);
    assert_eq!(needed, 48_000);

    let mut buf = BigBuf::zeroed(BufClass::DisplayFrame, needed).unwrap();
    assert_eq!(buf.len(), 48_000);

    let mut pattern = CountingPattern::new(42);
    capture_snapshot(&mut pattern, Rotation::Deg270, &rs, &mut buf).unwrap();
    let initial_calls = pattern.calls;
    assert!(initial_calls > 0);

    let mut snapshot = Snapshot::new(&buf, &rs).unwrap();

    // Simulate phase 1 reading window slices
    let max_rows = StripCore::max_rows_for_width(rs.pw);
    let mut phase1_bytes = Vec::new();
    let mut y = rs.py;
    while y < rs.py + rs.ph {
        let rows = max_rows.min(rs.py + rs.ph - y);
        let data = snapshot.render_window(Rotation::Deg270, rs.px, y, rs.pw, rows);
        phase1_bytes.extend_from_slice(data);
        y += rows;
    }

    // Simulate phase 3 reading window slices
    let mut phase3_bytes = Vec::new();
    y = rs.py;
    while y < rs.py + rs.ph {
        let rows = max_rows.min(rs.py + rs.ph - y);
        let data = snapshot.render_window(Rotation::Deg270, rs.px, y, rs.pw, rows);
        phase3_bytes.extend_from_slice(data);
        y += rows;
    }

    // Pattern was NOT rendered again
    assert_eq!(pattern.calls, initial_calls);

    assert_eq!(phase1_bytes.len(), 48_000);
    assert_eq!(phase3_bytes.len(), 48_000);
    assert_eq!(phase1_bytes, phase3_bytes);
    assert_eq!(&phase1_bytes[..], &buf[..]);
}

#[test]
fn display_frame_snapshot_partial_window_slicing() {
    let rs = align_partial_region(Rotation::Deg270, 16, 10, 80, 50).unwrap();
    let needed = (rs.pw as usize / 8) * (rs.ph as usize);
    assert!(needed <= 48_000);

    let mut buf = BigBuf::zeroed(BufClass::DisplayFrame, needed).unwrap();
    let mut pattern = CountingPattern::new(7);
    capture_snapshot(&mut pattern, Rotation::Deg270, &rs, &mut buf).unwrap();

    let mut snapshot = Snapshot::new(&buf, &rs).unwrap();

    let max_rows = StripCore::max_rows_for_width(rs.pw);
    let mut p1 = Vec::new();
    let mut y = rs.py;
    while y < rs.py + rs.ph {
        let rows = max_rows.min(rs.py + rs.ph - y);
        let data = snapshot.render_window(Rotation::Deg270, rs.px, y, rs.pw, rows);
        p1.extend_from_slice(data);
        y += rows;
    }

    let mut p3 = Vec::new();
    y = rs.py;
    while y < rs.py + rs.ph {
        let rows = max_rows.min(rs.py + rs.ph - y);
        let data = snapshot.render_window(Rotation::Deg270, rs.px, y, rs.pw, rows);
        p3.extend_from_slice(data);
        y += rows;
    }

    assert_eq!(p1, p3);
    assert_eq!(&p1[..], &buf[..]);
}

#[test]
fn display_frame_oversized_allocation_failure_falls_back() {
    // Refuses oversized allocation gracefully without panic
    let err = BigBuf::zeroed(BufClass::DisplayFrame, usize::MAX);
    assert!(err.is_err());
}

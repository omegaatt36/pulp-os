// SSD1677 800x480 panel geometry shared by firmware and host tests
//
// physical coordinates are the controller's RAM layout (x across the
// 800 px source lines, y across the 480 gates); logical coordinates are
// what widgets draw in after rotation

pub const WIDTH: u16 = 800;
pub const HEIGHT: u16 = 480;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Rotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

// byte-aligned physical window of a partial refresh; left_mask and
// right_mask cover the padding bits added by alignment
#[derive(Clone, Copy, Debug)]
pub struct RenderState {
    pub px: u16,
    pub py: u16,
    pub pw: u16,
    pub ph: u16,
    pub left_mask: u8,
    pub right_mask: u8,
}

// logical region -> physical region, clipped to the panel first;
// a region with no visible area becomes (0, 0, 0, 0)
pub fn transform_region(
    rotation: Rotation,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
) -> (u16, u16, u16, u16) {
    let (logical_w, logical_h) = match rotation {
        Rotation::Deg0 | Rotation::Deg180 => (WIDTH, HEIGHT),
        Rotation::Deg90 | Rotation::Deg270 => (HEIGHT, WIDTH),
    };
    let x0 = x.min(logical_w);
    let y0 = y.min(logical_h);
    let x1 = (u32::from(x) + u32::from(w)).min(u32::from(logical_w)) as u16;
    let y1 = (u32::from(y) + u32::from(h)).min(u32::from(logical_h)) as u16;
    if x0 >= x1 || y0 >= y1 {
        return (0, 0, 0, 0);
    }
    let (x, y, w, h) = (x0, y0, x1 - x0, y1 - y0);

    match rotation {
        Rotation::Deg0 => (x, y, w, h),
        Rotation::Deg90 => (WIDTH - y - h, x, h, w),
        Rotation::Deg180 => (WIDTH - x - w, HEIGHT - y - h, w, h),
        Rotation::Deg270 => (y, HEIGHT - x - w, h, w),
    }
}

// logical region -> byte-aligned physical window for RAM writes;
// None when the region is empty after clamping to the panel
pub fn align_partial_region(
    rotation: Rotation,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
) -> Option<RenderState> {
    let (tx, ty, tw, th) = transform_region(rotation, x, y, w, h);

    let px = (tx & !7).min(WIDTH);
    let py = ty.min(HEIGHT);
    let pw = ((tw + (tx & 7) + 7) & !7).min(WIDTH - px);
    let ph = th.min(HEIGHT - py);

    if pw == 0 || ph == 0 {
        return None;
    }

    let lp = (tx - px) as u32;
    let rp = ((px + pw) - (tx + tw)) as u32;
    let left_mask: u8 = if lp > 0 { !((1u8 << (8 - lp)) - 1) } else { 0 };
    let right_mask: u8 = if rp > 0 { (1u8 << rp) - 1 } else { 0 };

    Some(RenderState {
        px,
        py,
        pw,
        ph,
        left_mask,
        right_mask,
    })
}

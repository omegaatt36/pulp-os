# T7 optional-font metadata failure contract

Optional pack absence is distinguished from inability to inspect an installed
pack. Definitive absence permits Ready text with the size-specific synthetic
missing box. A genuine OpenFile/OpenDir or other metadata I/O failure remains
a recoverable error with a cause. The optional-font metadata lookup may use a
narrow seam that returns NotFound only for definitive absence; it must not
reinterpret every OpenFile/OpenDir failure as absence.

One host test installs valid hand-packs, injects StorageOp::FileSize OpenFile
on the23px pack, verifies the injection fired and requires Phase::Error with
a cause. Error rendering remains drawable, full/stitched pixels agree and no
source read occurs during draw. The separate existing no-pack test still
requires Ready, synthetic boxes and real glyphs after install/reopen.

The legacy UTF-8 draw oracle retains native FontSet table advances for available
glyphs and U+FFFD. Unsupported valid scalars on these uninstalled-pack fixtures
use literal body advances16/19/23/28/35, derived from the approved missing-box
metrics. Row scans, ink assertions, tolerances and cases remain unchanged.

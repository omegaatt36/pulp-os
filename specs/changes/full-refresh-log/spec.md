# full-refresh-log — Requirements

### R1: Full refreshes are observable
WHEN a frame is displayed by a full refresh on the C61 (a `Redraw::Full` request or a ghost-clear promotion) THE SYSTEM SHALL log one line stating that a full refresh completed, its duration in milliseconds, and the partial count after the reset (0).
IF the full refresh fails or the frame is given up THEN THE SYSTEM SHALL NOT log the completion line (the existing failure logs remain the only output).

### R2: Counter reset stays consistent with the log
WHEN a full refresh completes THE SYSTEM SHALL reset the partial count to 0 and the next partial refresh log SHALL report count 1.

### R3: Partial paths are unchanged
THE SYSTEM SHALL keep chapter changes and returns to the previous screen (`nav.resume`) on the partial path, incrementing the partial count as today.
THE SYSTEM SHALL keep the `display: partial refresh …` and `display: promoted partial to full …` lines unchanged.

### R4: Acceptance criteria match the behavior
WHERE `specs/references/hardware-acceptance.md` defines P8 THE SYSTEM SHALL list only forward navigation (Home → Files/Reader/Settings) and opening a book as the full-refresh paths, and SHALL state that chapter changes and returns are partial with the count incremented.
WHERE the same file defines P9 THE SYSTEM SHALL name the R1 line as the evidence for the first full frame after wake.

Scenario (R1, R2): Given partial count 3 When Home → Files completes Then the log shows one full-refresh line with count 0, and the next list move logs `partial refresh … (partial count 1)`.

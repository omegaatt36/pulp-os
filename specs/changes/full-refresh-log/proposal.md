# full-refresh-log

## Why

成品機實測（`specs/hardware-records/2026-10-08-product.md` P8／F10／F11）：整頁全刷路徑 `render_full`（`kernel/src/kernel/scheduler_c61.rs:350`）不印任何 log，全刷的發生與 partial 計數歸零只能從前後 `partial count` 間接推算，所以 P8（整頁切換仍為全刷並歸零計數）、P9（喚醒首幀全刷）的判定列「log 為 `full refresh`」永遠不可能成立。另外 P8 判定列把「換章」當整頁切換，但實作上換章與翻頁相同，走 `mark_dirty(PAGE_REGION)` 的 partial，計數不歸零（`sleep16h.log` 17:12:11／17:12:31）。使用者決定**維持換章為 partial**，改判定，不改行為。

## Scope

- In：讓 `render_full` 成功時印一行 log（含耗時與歸零後的 partial 計數）；更新 `specs/references/hardware-acceptance.md` 的 P8（以及依賴同一 log 的 P9）判定列，換章與「返回上一個畫面」不再列為全刷路徑；實機重驗 P8、P9 並更新記錄檔。
- Out：換章、返回 Home 的重繪方式（維持 partial，含整頁 480×800 的差分刷新）；ghost clear 週期；`render_partial` 的 log 格式；失敗與重試路徑的行為；boot 映像。

## Impact

`kernel/src/kernel/scheduler_c61.rs`（`render_full`，約 10 行 log 與計時）；`specs/references/hardware-acceptance.md`（P8、P9 兩列）；`specs/hardware-records/2026-10-08-product.md`（P8 狀態與 F10／F11 的處置欄）。預設映像也會多一行 log，但只在整頁全刷時出現（頻率遠低於翻頁）。

## Risk

`render_full` 也被 ghost clear 升級與 `refresh_with_recovery` 使用：log 必須只在畫面確實顯示後才印（失敗路徑已有 `error!`／`warn!`，不重複）。計時沿用 `now_ms()`，不得在全刷內增加阻塞。

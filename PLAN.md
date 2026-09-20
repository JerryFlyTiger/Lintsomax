# Lintsomax 進度與交接

最後更新：2026-09-20

## 目標（已定案，不再討論）

- **首要**：故障隔離（一個驅動 bug 不該打垮整台機器）+ 記憶體安全
- **次要**：多租戶隔離、capability 權限模型
- 定位：**玩玩性質**，不是要取代 Linux。成功的定義是「在窄場景把痛點解得比 Linux 好，且有可執行的證據」

## 定案的技術決定（理由見 README.md）

| 項目 | 決定 |
|---|---|
| 架構 | 微核心，capability-based |
| 語言 | Rust `no_std`，nightly |
| 平台 | aarch64 · QEMU `virt` · Apple Silicon 上用 HVF 量測、TCG 除錯 |
| 硬體範圍 | 只支援 virtio，不碰裸機驅動 |
| 除錯器 | `aarch64-elf-gdb`（不是 lldb）。理由：系統暫存器 + Rust enum 支援 |
| `dev` profile | **`opt-level = 0` 不可改**。實測 1 會讓變數全變 `<optimized out>` |

## 里程碑

- [x] **M0** 開機 · EL1 · PL011 UART · `println!` · 例外向量表 · ESR/FAR 解碼 · semihosting 收工
- [x] **M1** 實體框配置器 · 四階頁表 · 恆等映射 · MMU 開啟 · 轉譯失敗正確解碼
- [ ] **M1.5** 向量表可回復化 ← **下一步，接手點在這裡**
- [ ] **M2** capability 表 · 高半部核心（TTBR1）遷移
- [ ] **M3** IPC
- [ ] **M4** 第一個隔離的 virtio 驅動 ← 故障隔離的第一個真實 demo

## 接手點：M1.5 向量表可回復化

**現況**：`src/exceptions.rs` 的 handler 印完報告就停機（`semihost::exit`）。
`__exception_common` 只把入口編號放進 x0 就 `bl`，沒有保存暫存器。

**要做的**：
1. `__exception_common` 保存 x0–x30 到堆疊形成 `ExceptionFrame`，把框的指標傳給 Rust handler
2. handler 改成回傳「停機」或「跳過這道指令繼續」的決定
3. 「跳過」的做法：`ELR_EL1 += 4`，還原暫存器，`eret`
4. demo 擴充成連續三個故障都活著走完：
   - 讀未映射位址 → 轉譯失敗 → 回復
   - 寫 `.rodata` → **權限不足**（驗證 M1 的唯讀映射真的生效）→ 回復
   - 最後一個不回復，正常停機

**為什麼重要**：這是 M4 驅動隔離的雛形。故障之後還活著，才叫隔離。
也是**唯一能驗證 M1 權限位元真的寫對**的方法——恆等映射下權限寫錯不會顯現。

## 審查記錄

### 第一輪冷讀（M0 + M1，830 行）

**通過的部分**：頁表描述子位元編碼（AttrIndx/AP/SH/AF/PXN/UXN 位置）、
TCR_EL1 欄位（含 TG0=0b00 vs TG1=0b10 這個編碼不同的陷阱）、MMU 開關屏障順序、
PL011 暫存器、向量表對齊、ESR 解碼表、`.bss` 清零邊界、semihosting 慣例——
逐位元核對後全部正確。

**已修**：

| # | 問題 | 修法 |
|---|---|---|
| 1 | `cargo clippy -- -D warnings` 編譯失敗（2 個 lint） | `paging.rs` 改用 `is_multiple_of`；移除 `(0b00 << 14)` 這個無效運算，改成註解 |
| 3 | `TCR_EL1.EPD1` 未設，TTBR1 未初始化卻仍啟用表走訪 | 補上 `1 << 23` |
| 4 | release build 會把 `debug_assert` 編掉，對齊假設違反時靜默發生 | `Cargo.toml` release profile 加 `debug-assertions = true` |
| 6 | README 停在 M0 | 已更新 |

**未修，連同理由**：

| # | 問題 | 為什麼不修 |
|---|---|---|
| 2 | 自我驗證對「權限位元寫錯」偵測力不足 | **M1.5 就是在修這個**。見下方 mutation 實測結果 |
| 5 | `paging.rs` 的 `table_ref()` 每次對同一 PA 生出新的 `&'static mut`，在 Rust 抽象機器層級是別名 UB | 單核心 + `opt-level=0` 下無可觀察影響；無法用 mutation 驗證（Miri 在此 target 跑不起來）。**M2 建立 per-task 位址空間時本來就要重構這塊，屆時改成 raw pointer 一併處理** |
| 7 | 非 core 0 停放時 `VBAR_EL1` 未設、`DAIF` 未遮罩 | `run.sh` 沒帶 `-smp`，QEMU 只開 1 個 vCPU，路徑不會執行到。多核心屬後續里程碑 |
| 8 | Device 記憶體用 `SH=0b00` 而非 Outer-shareable | 信心很低。Device-nGnRE 的排序語意不靠 SH 欄位保證，單核心無影響 |

### Mutation 實測（主對話執行，非 reviewer）

- **A（clippy）**：確認失敗，非猜測。已修。
- **B（`AP_RW` / `AP_RO` 對調）**：**reviewer 預測「輸出完全不變」，實測是錯的。**
  實際結果是核心在印出 `[6/6] MMU 已開啟` 之前就無聲死掉。
  注意 `device_rw()` 與 `normal_rw()` 共用同一個 `AP_RW` 常數，所以這條 mutation
  同時把堆疊**和** UART MMIO 都變成唯讀。從現象無法直接分辨哪個寫入先故障，
  但從流程可推斷是堆疊先——`println!` 在 `opt-level=0` 下每一層都有函式前言要寫堆疊，
  而 `put()` 是先讀 `FR`（不受影響）才寫 `DR`。
  不論哪個先，因果鏈都成立：堆疊唯讀 → 例外處理常式自己的函式前言也會故障
  → 遞迴故障 → 無聲鎖死。
  結論修正為：**偵測得到，但只表現為「什麼都不印就死掉」，是最難查的失敗模式**；
  而且只抓得到「資料區被誤設唯讀」，抓不到「`.text` 被誤設可寫」——沒有東西會去寫 `.text`。
  這正是 M1.5 要補的：一個會**主動去寫 `.rodata` 並預期收到權限故障**的測試。

### 第二輪冷讀（修復輪，尾段 diff）

**通過，沒有找到 bug。** 逐項確認：`is_multiple_of` 語意等價；移除 `(0b00 << 14)` 後
`tcr` 數值一字不差且沒有其他項填到 bits[15:14]；**bit 23 確實是 `EPD1`**（`A1` 是 bit22、
`IRGN1` 從 bit24 起，無重疊），設 1 的語意是停用 TTBR1 表走訪。

已排除的疑慮：`debug-assertions = true` **不會**連帶打開 `overflow-checks`（實測），
且本專案未用 `-Zbuild-std`，`core` 是 rustup 預編譯的 rlib，所以這個設定只影響
`lintsomax` 自己這顆 crate，不會讓 panic 路徑膨脹。

**新增的兩個覆蓋缺口（待辦，非缺陷）**：

- **`EPD1` 的防線目前是黑箱**。沒有任何程式碼存取上半部 VA，所以把這個修法還原，
  `cargo run` 輸出會一字不差。→ **M2 高半部遷移時補一個刻意存取高位 VA、
  預期拿到 level-0 轉譯失敗的測試。**
- **`debug-assertions` 同樣打不到測試路徑**。`mm.rs:22` 唯一那個 `debug_assert_eq!`
  在現有所有路徑下必定成立。→ 低優先，等 linker script 再動時一併處理。

## 審查狀態

- 專案**尚無任何 commit**，全部程式碼在 git index。
- 第一輪冷讀已完成，修復輪（上表 1/3/4/6）**尚未冷讀**。
- **還有未冷讀的改動就不准 commit。**

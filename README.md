# Lintsomax

一個為了解決 Linux 兩個結構性痛點而寫的核心：

1. **沒有故障隔離** —— 一個驅動程式的 bug 可以讓整台機器 panic
2. **記憶體不安全** —— 核心是 C 寫的

次要目標：多租戶隔離、capability 權限模型。

## 已定案的決定

| 項目 | 決定 | 理由 |
|---|---|---|
| 架構 | 微核心（capability-based） | monolithic 就是痛點本身；unikernel 沒有內部隔離；混合式一開始就妥協 |
| 語言 | Rust `no_std` | 痛點 2。`unsafe` 集中在少數模組 |
| 目標平台 | aarch64 · QEMU `virt` | 開發機是 Apple Silicon，HVF 可加速；aarch64 開機與記憶體模型乾淨 |
| 硬體範圍 | 只支援 virtio，不碰裸機驅動 | 驅動佔 Linux 原始碼 60–70%，繞開它成本掉一個數量級 |
| 記憶體模型順序 | 先 aarch64（弱序）再考慮 x86（強序） | 弱序→強序安全，反向會踩到抓不到的競爭條件 |

## 跑起來

```bash
cargo run            # 開機、跑 demo、自己收工
cargo run -- trace   # 加上 QEMU 例外追蹤，寫到 qemu-int.log
```

## 除錯

```bash
# 終端機 A
cargo run -- debug
# 終端機 B
./debug.sh
```

`.gdbinit` 會自動載入 Rust pretty-printer 並連上 `:1234`。

**注意 `opt-level`**：`dev` profile 必須維持 `opt-level = 0`。
實測 `opt-level = 1` 會讓所有區域變數變成 `<optimized out>`，
而且斷點會掉進被 inline 的 `core` 原始碼裡。

## 現況：M1 完成

- [x] **M0** aarch64 開機、停在 EL1、多核心只跑 core 0
- [x] **M0** PL011 UART + `println!`
- [x] **M0** 例外向量表（16 個入口）+ ESR/FAR 解碼報告
- [x] **M0** Rust panic handler、semihosting 收工
- [x] **M1** 實體框配置器（bump）
- [x] **M1** 四階頁表、恆等映射、MMU 開啟
- [x] **M1** `.text` 唯讀可執行、資料區不可執行
- [ ] **M1.5** 向量表可回復化 ← 下一步
- [ ] **M2** capability 表、高半部核心（TTBR1）遷移
- [ ] **M3** IPC
- [ ] **M4** 第一個隔離的 virtio 驅動 ← 故障隔離的第一個真實 demo

## 原始碼

```
src/boot.rs        _start：核心停放、堆疊、清 .bss
src/uart.rs        PL011 驅動 + print!/println!
src/exceptions.rs  向量表、ESR/FAR 解碼        ← 專案主題所在
src/mm.rs          實體記憶體框配置器（bump）
src/paging.rs      四階頁表、恆等映射、MMU 開關
src/panic.rs       Rust panic 落地
src/semihost.rs    讓核心能主動結束 QEMU
linker.ld          記憶體佈局（載入於 0x4010_0000）
```

## 已知問題（冷讀查出，暫不修）

見 `PLAN.md` 的「審查記錄」。最重要的一條：**目前的自我驗證對「頁表權限位元寫錯」
偵測力不足**——實測把 `AP_RW`/`AP_RO` 對調，核心會在 MMU 開啟時無聲死掉，
沒有任何診斷訊息。M1.5 的可回復例外就是為了補這個洞。

## 授權

**GPL-3.0-or-later，加上系統呼叫例外聲明。**

| 檔案 | 內容 |
|---|---|
| `LICENSE` | GNU GPL 第三版全文（未經修改，讓 GitHub 正確辨識） |
| `LICENSE-EXCEPTION` | 系統呼叫例外聲明（英文為準，附中文參考譯文） |

**對核心原始碼本身的修改**：完整適用 GPL-3.0-or-later。拿去改、拿去賣都可以，
但改動必須以同樣條款開源。

**跑在 Lintsomax 上的使用者空間程式不受影響**——無論是透過系統呼叫、IPC 訊息、
capability 呼用，或經由這些介面建立的共享記憶體。這些不視為衍生作品，作者可自行
選擇任何授權。

設立這項例外的原因是 Lintsomax 是微核心：**驅動程式、檔案系統、網路堆疊被刻意
設計成獨立的使用者空間程式**。沒有這段例外，系統的正常架構會讓每一個這類元件
都變成核心的衍生作品，那不是本專案的意圖。做法參考 Linux 的 `COPYING`。

每個原始碼檔案開頭都有 SPDX 標頭：

```
// SPDX-License-Identifier: GPL-3.0-or-later WITH Linux-syscall-note
```

`Linux-syscall-note` 是 SPDX 已登錄的例外識別碼，原本定義在 Linux 使用 GPL-2.0
的情境下。這行只是給工具讀的機器可辨識近似值，**實際生效的例外條款是
`LICENSE-EXCEPTION` 的文字**。

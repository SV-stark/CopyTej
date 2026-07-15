# CopyTej — High-Performance File Transfer Utility

<p align="center">
  <img src="public/logo.png" alt="CopyTej Logo" width="160" />
</p>

[![Build & Package](https://github.com/SV-stark/CopyTej/actions/workflows/build.yml/badge.svg)](https://github.com/SV-stark/CopyTej/actions/workflows/build.yml)
[![Download Nightly Setup](https://img.shields.io/badge/Download-Nightly%20Release-blueviolet?logo=github&style=flat-square)](https://github.com/SV-stark/CopyTej/releases/tag/nightly)
[![Webpage](https://img.shields.io/badge/Webpage-CopyTej%20Site-blue?logo=githubpages&style=flat-square)](https://sv-stark.github.io/CopyTej/)

CopyTej is a blazing-fast, single-instance file transfer utility built with **Tauri v2**, **React**, **TypeScript**, and **Rust**. Designed as a modern Windows Fluent interface alternative to TeraCopy and UltraCopier, it maximizes local and network storage transfer speeds while offering advanced automation and safety controls.

---

## 🚀 Key Features

* **NTFS/ReFS Block Cloning (Reflinks):** Leverages native Windows `DeviceIoControl` (`FSCTL_DUPLICATE_EXTENTS_TO_FILE`). Same-volume copies complete instantly (0ms) and consume zero extra storage space.
* **Same-Volume Parent Folder Move Optimization:** If moving a folder within the same drive volume, CopyTej instantly renames the parent directory in `0ms` instead of walking child structures and copying files individually.
* **Centralized Global Rate Limiter:** A thread-safe token-bucket rate limiter throttles bandwidth collectively across all running parallel copy jobs in real time.
* **Asynchronous File Auto-Retry & Resume:** Automatically retries transient network or I/O failures (up to 3 times, waiting 5 seconds between attempts). It resumes writing from the **exact byte offset** of the failure to prevent restarting long copies.
* **File Size Validation on Resume:** When resuming a paused transfer, CopyTej compares the current source file size with the initial metadata record to safeguard against corrupted data.
* **Asynchronous Double-Buffering:** Decouples reading and writing using Tokio channels to saturate local SSDs, HDDs, and network locations without locking file systems.
* **On-the-Fly Hashing & Verification:** Computes Blake3, XXHash3, MD5, or SHA-256 checksums concurrently during the write loop to avoid redundant read cycles.
* **Detailed Transfer Log Exporter:** Export transfer manifests directly from the History tab into formatted CSV or JSON reports containing source, destination, sizes, checksums, and error descriptions.
* **Synthetic Sound chimes:** Generates clean digital chimes dynamically via the HTML5 Web Audio API (ascending major arpeggio for successful completions, triangle minor-third drop for warnings/errors) with a settings toggle.
* **Single-Instance CLI Named Pipe Server:** Runs a named pipe server at `\\.\pipe\CopyTej`. Second launches (e.g. from Explorer right-click) serialize path arguments and forward them directly to the active transfer queue, exiting instantly.
* **Native Windows Explorer Context Menu (HKCU):** Integrates "Copy with CopyTej" and "Move with CopyTej" directly into the right-click menu for both files and directories without requiring administrator rights.
* **Tauri Drag & Drop Integration:** Drop files and folders directly onto the dashboard window to instantly parse paths and configure transfer jobs.
* **Smart Overwrite Rules:** Interactive side-by-side collision cards let you resolve naming conflicts with options like Overwrite Older, Skip Same Size/Date, and Auto-Rename.
* **Windows Fluent UI Dashboard:** Responsive grid layouts, real-time speed charts, moving average ETA calculations, active sidebar selectors, and detailed file history.

---

## 🛠️ Architecture Overview

```
                          ┌──────────────────────────┐
                          │  Windows File Explorer   │
                          └─────────────┬────────────┘
                                        │ (Right-Click Context Menu)
                                        ▼
                          ┌──────────────────────────┐
                          │   CopyTej CLI Launcher   │
                          └─────────────┬────────────┘
                                        │
                 ┌──────────────────────┴──────────────────────┐
                 │ (Forward Job Specs)                         │ (Spawn New Process)
                 ▼                                             ▼
     ┌────────────────────────┐                    ┌────────────────────────┐
     │  Running Named Pipe    │                    │  New Tauri App Window  │
     │  Server (\pipe\CopyTej)│                    │  (Initial Instance)    │
     └────────────────────────┘                    └───────────┬────────────┘
                                                                │
                                                                ▼
                                                    ┌────────────────────────┐
                                                    │  Tauri Rust Backend    │
                                                    │  (TransferEngine, DB)  │
                                                    └───────────┬────────────┘
                                                                │
                                                                ▼
                                                     ┌────────────────────────┐
                                                     │  Fluent React          │
                                                     │  (Web Audio, Frontend) │
                                                     └────────────────────────┘
```

---

## 💻 Getting Started

### 📋 Prerequisites

* **Rust Toolchain:** Installed via [rustup](https://rustup.rs/) (edition 2021).
* **Node.js:** v18 or later.
* **Package Manager:** npm.

### 🔧 Installation

1. Clone the repository and navigate to the project directory:
   ```bash
   git clone https://github.com/SV-stark/CopyTej.git
   cd CopyTej
   ```

2. Install Node dependencies:
   ```bash
   npm install
   ```

### 🚀 Running in Development Mode

Launch the Tauri developer compiler:
```bash
npm run tauri dev
```

### 📦 Building for Production

Compile a production binary:
```bash
npm run tauri build
```
The compiled executable will be located in: `src-tauri/target/release/copytej-app.exe`.

---

## ⚙️ Settings & System Integration

* **Explorer Context Menu:** Toggle context menu shortcuts directly from the **Settings** tab. It registers paths dynamically in the registry under `HKCU\Software\Classes` (safe and requires no UAC administrator elevation).
* **Rate Limiter:** Set global throttle limits (e.g. 50000 for 50MB/s) in the settings panel. Enter 0 for unlimited transfer speed.

---

## 🧪 Running Tests

A comprehensive unit test suite is provided to validate database safety, hashing speeds, buffer adjustments, and oneshot conflict channels.

Run the tests using Cargo:
```bash
cd src-tauri
cargo test
```

# CopyTej — High-Performance TeraCopy & UltraCopier Replacement

CopyTej is a high-performance, single-instance file transfer utility built with **Tauri v2**, **React**, **TypeScript**, and **Rust**. It provides a clean, native Windows Fluent interface and replicates the core features of TeraCopy and UltraCopier, including double-buffered file I/O, on-the-fly verification, NTFS block cloning, metadata preservation, and smart overwrite rules.

---

## 🚀 Key Features

* **NTFS/ReFS Block Cloning (Reflinks):** Implemented native Windows `DeviceIoControl` hook (`FSCTL_DUPLICATE_EXTENTS_TO_FILE`). Same-drive transfers on supported file systems copy instantly (0ms) and use 0 extra bytes.
* **Asynchronous Double-Buffering:** Read chunks from source files in a background thread while concurrently writing to the destination, maximizing speed on local SSDs, HDDs, and network locations.
* **On-the-Fly Hashing & Verification:** Computes Blake3, XXHash3, MD5, or SHA-256 checksums on-the-fly during write loops to prevent redundant read cycles.
* **Smart Overwrite Rules:** Logical conflict resolution options including **Overwrite Older Only** (comparing file modification times) and **Skip Same Size & Date** (automated duplicate skipping).
* **Metadata & Attribute Preservation:** Replicates exact Creation, Modification, and Access timestamps via the `std::fs::FileTimes` API and applies Windows File Attributes (Hidden, System, Archive, Read-Only) via inline FFI.
* **Same-Drive Move Optimizations:** Instantly renames files (`std::fs::rename`) when moving items within the same physical volume root instead of slow chunk copying.
* **Single-Instance CLI Forwarding:** Launches a background named pipe server at `\\.\pipe\CopyTej`. Launching a second instance (e.g. from Explorer) serializes CLI paths, forwards them to the active instance's transfer queue, and exits immediately.
* **Windows Context Menu Integration:** Add right-click shortcuts to copy/move files directly via CopyTej.
* **Windows Fluent UI Dashboard:** Real-time speed charts, ETA calculators, active queue selectors, detailed file history, and interactive side-by-side conflict card grids designed like a native Windows desktop app.
* **Interactive I/O Pipeline Flow:** Real-time visual representation of file read stages, dynamic buffer utilization, hash-verification engine, and destination verification checks.

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
                                                    │  Frontend Dashboard    │
                                                    └────────────────────────┘
```

* **Backend (Rust):** Handled by a structured transfer manager utilizing Tokio-based channels, SQLite (rusqlite) task logging, and inline Windows FFI bindings.
* **Frontend (React / TypeScript):** Clean, flat Windows design system built using CSS grid structures, native border profiles, and Tauri event listener hooks (`transfer://file-progress`, `transfer://conflict`, etc.).

---

## 💻 Getting Started

### 📋 Prerequisites

* **Rust Toolchain:** Installed via [rustup](https://rustup.rs/) (edition 2021).
* **Node.js:** v18 or later.
* **Package Manager:** npm.

### 🔧 Installation

1. Clone the repository and navigate to the project directory:
   ```bash
   git clone <repo-url>
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

## 🎛️ Explorer Context Menu Setup

To add CopyTej to your Windows right-click options:

1. Compile the app in release mode (`npm run tauri build`).
2. Open PowerShell as an **Administrator**.
3. Run the registration script:
   ```powershell
   Set-ExecutionPolicy Bypass -Scope Process
   ./register_context_menu.ps1
   ```
4. This adds **"Copy with CopyTej"** and **"Move with CopyTej"** entries to your context menu for files and folders.
5. To remove the context menu entries, run:
   ```powershell
   ./register_context_menu.ps1 -Unregister
   ```

---

## 🧪 Running Tests

A comprehensive unit test suite is provided to validate database safety, hashing speeds, buffer adjustments, and oneshot conflict channels:

Run the tests using Cargo or Cargo Nextest:
```bash
cd src-tauri
cargo nextest run
```

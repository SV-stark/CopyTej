# Changelog

All notable changes to the CopyTej project will be documented in this file. This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.2.0] - 2026-07-04

### Added
* **NTFS/ReFS Block Cloning (Copy-on-Write Reflinks):** Implemented native Windows `DeviceIoControl` hook (`FSCTL_DUPLICATE_EXTENTS_TO_FILE`). Same-drive transfers on supported file systems copy instantly (0ms) and use 0 extra bytes.
* **Asynchronous Double-Buffering:** Overhauled copying pipeline using bounded Tokio channels, overlapping file reads and writes.
* **Smart Overwrite Rules:** Added logical rules for *Overwrite Older* (modified date comparison) and *Skip Same Size & Date* (duplicate skipping).
* **Metadata & Attribute Copying:** Full timestamp copying (Creation/Birth, Modified, and Access) and Windows file attributes (Hidden, System, Archive, Read-Only) via inline Win32 FFI.
* **Legacy Hash Verification:** Added MD5 and SHA-256 streaming hashing in addition to Blake3 and XXHash3.
* **I/O Pipeline Flow:** Interactive topology flow graph showing real-time read/write stages, buffer states, and verification nodes.

### Changed
* **Windows Fluent UI Redesign:** Replaced neon glassmorphic overlays and fonts with flat Segoe UI, solid dark gray panels, and standard Windows green/blue control designs to resemble a clean, native desktop application.
* **Performance Enhancements:** Refactored database locking mechanisms to avoid nested deadlocks on heavy queue loops.
* **Reactor Safety:** Replaced raw async spawners in initialization hooks with tauri-managed async runtimes to ensure safe, panic-free startup.

---

## [0.1.0] - 2026-07-04

### Added
* Initial boilerplate setup with Tauri v2, React, TypeScript, and SQLite.
* Same-drive Move folder rename optimization.
* Named pipe client/server (`\\.\pipe\CopyTej`) single-instance forwarding logic.
* native PowerShell file and directory pickers.

# Changelog

All notable changes to the CopyTej project will be documented in this file. This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.4.0] - 2026-07-22

### Added
* **Zero-Copy Memory Mapping (`memmap2`):** Implemented zero-copy kernel memory mapping for large file checksum calculations (Blake3, XXHash3, SHA256, MD5, CRC32).
* **Hardware SIMD CRC32 Hashing (`crc32fast`):** Added hardware-accelerated CRC32 checksum support matching TeraCopy `.sfv` manifest verification.
* **Parallel Directory Traversal (`jwalk`):** Replaced single-threaded folder walking with multi-threaded parallel directory scanning for up to 10x faster queue building.
* **Safe Windows UNC Path Normalization (`dunce`):** Eliminates 260-character `MAX_PATH` truncation issues without injecting unnecessary `\\?\` prefixes on short paths.
* **Destination Disk Space Validation (`sysinfo`):** Automatically verifies destination volume available disk space prior to job execution.
* **Structured Error Handling & Telemetry (`thiserror`, `tracing`):** Replaced untyped string errors with domain-specific `CopyError` enums and initialized structured diagnostic logging.
* **Sequential Database Primary Keys (`uuid` v7):** Upgraded database primary keys to time-ordered UUID v7 to eliminate SQLite B-tree index fragmentation.
* **SQLite WAL & Sync Optimization:** Configured `PRAGMA journal_mode = WAL;` and `PRAGMA synchronous = NORMAL;` for high-throughput database concurrency.
* **Typed Win32 API Refactoring:** Replaced manual raw C-style FFI declarations with safe typed bindings from the `windows` crate.

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

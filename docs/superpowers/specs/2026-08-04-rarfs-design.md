# rarfs — Design Spec

Date: 2026-08-04
Status: Approved (design sections 1–2 confirmed by user)

## Purpose

A read-only FUSE filesystem in Rust that mounts a directory tree of RAR-archived
video files and exposes their contents as regular files, so media servers
(Plex/Jellyfin) can play the videos without extracting the archives.

Target workload: a large collection of mostly **store-mode** (uncompressed, `-m0`)
RAR archives, one video per archive set, files of ~2–17 GB, with a few concurrent
playback streams plus background library scans.

## Non-goals

- Writing / modifying archives or any other files (the FS is read-only).
- Reimplementing RAR decompression in pure Rust (compressed files are handled via
  rarlab's unrar code through FFI).
- Archive formats other than RAR (RAR4 and RAR5).
- Performance heroics for compressed archives beyond a small seek window.

## Mount semantics

`rarfs <source-dir> <mountpoint>`

- The mount mirrors the source directory tree, read-only.
- Every real file passes through untouched, including the `.rar` / `.r00` /
  `.partNN.rar` volume files themselves.
- Each archive *set* additionally surfaces its archived contents as regular files
  at the appropriate path (e.g. `Show.S01E01.rar` + `Show.S01E01.r00..r37` also
  yields `Show.S01E01.mkv` with the correct size).
- Media servers only scan video extensions, so pass-through volumes alongside
  extracted entries do not confuse Plex/Jellyfin.
- Volume sets are detected by naming convention:
  - RAR4: `name.rar` followed by `name.r00`, `name.r01`, …
  - RAR5: `name.part01.rar` / `name.part1.rar` followed by sequential parts.
- Directory contents are cached with mtime-based invalidation, so new files
  appear without remounting.

## Architecture

One binary, four internal modules. The chosen approach (A) exploits the fact that
stored files need no decompression: reads become direct byte-range reads from the
volume files, and the kernel page cache does the caching.

### 1. `rarhdr` — RAR header parser

Our own parser for RAR4 and RAR5 headers. It never decompresses anything; it
walks the headers of each volume in a set and produces, per archived file:

- unpacked size, compression method, CRC;
- a **segment map**: an ordered list of `(volume path, byte offset, packed
  length)` chunks which, concatenated, form the file's packed data.

This module is the heart of the design. Wrong output shows up as obvious data
corruption in tests, not subtle bugs, and it is covered heavily by unit tests
(see Testing).

### 2. `catalog` — in-memory index

- Scans the source tree lazily per directory.
- Groups volume files into sets, runs `rarhdr`, and maps each virtual path to
  either `Passthrough(real path)` or `ArchiveMember { segment map, method, size, crc }`.
- Cached per directory; invalidated by directory mtime check on access.

### 3. `readers` — two implementations behind one trait

Common interface: `read(offset, &mut buf) -> io::Result<usize>`.

- **`StoreReader`** (fast path, the common case): pure offset arithmetic over the
  segment map, then `pread()` directly on the volume files. Stateless — no locks,
  unlimited concurrent readers, kernel page cache provides all caching.
- **`UnrarReader`** (fallback for compressed members): rarlab unrar C++ via FFI,
  streaming decode with a small tail buffer (~8 MB) so short backward seeks
  (typical player behavior) do not force a full re-decode. Long backward seeks
  reopen the archive and skip forward — slow but correct, and rare in the target
  collection.

### 4. `fuse` layer

Thin layer over the `fuser` crate. `lookup`/`getattr`/`readdir` consult the
catalog; `open` selects the appropriate reader; `read` forwards to it; plus
`statfs` and the usual read-only stubs. No logic beyond plumbing.

### Data flow example (playback seek)

1. Plex opens `Show.S01E01/Show.S01E01.mkv`.
2. Catalog hit: stored member with a segment map.
3. Player seeks to 40:00 → FUSE `read` at some offset.
4. Segment map resolves the range to volumes 12–13 at known offsets.
5. Two `pread()` calls; the kernel may serve them from page cache.
6. Total overhead over a plain file: one small in-memory lookup.

## Caching

Deliberately minimal:

- Stored fast path: **kernel page cache only**. No userspace block cache —
  double-buffering gigabyte-scale sequential video would just waste RAM.
- Catalog: parsed directory listings + header parses, mtime-invalidated.
- `UnrarReader`: ~8 MB tail window per open compressed file.

Total daemon memory: tens of MB regardless of library size.

## Error handling

Read-only FS — all failures are about reporting, not recovery.

- Missing/corrupt volume → `EIO` only for reads spanning that segment; the rest
  of the file still plays.
- Unparseable headers → that set's contents are hidden from listings; a warning
  with the path is logged; everything else keeps working.
- Truncated last volume (partial download) → exposed size reflects the headers;
  reads past available data return `EIO` (same behavior as rar2fs).
- `--log <file>` flag for diagnosing why a given video did not appear.

## Testing

- **Unit tests for `rarhdr`** against synthetic archives generated in-test with
  the system `rar` binary: RAR4 vs RAR5, store vs compressed, single vs
  multi-volume (including volumes split mid-file), unicode names. Assert that the
  segment map reconstructs the original bytes exactly.
- **Property-style test**: random offset/length reads through `StoreReader`,
  compared byte-for-byte against the original file.
- **Integration test**: mount a tempdir via `fuser` in-process, walk it, compare
  every extracted file against `unrar`-extracted references.
- **Manual soak**: mount a sample of the real collection, play and scrub several
  files in the actual media player before declaring done.

## Project setup

- New Cargo project at `~/rarfs`.
- Dependencies: `fuser` (FUSE), a thin vendored FFI wrapper over rarlab's unrar
  source (same approach as rar2fs), `clap` (CLI), `tracing` (logging).
- Environment prerequisites: Rust toolchain via `rustup`; `libfuse3-dev` via apt
  (sudo — user runs this). fuse3 runtime is already installed.

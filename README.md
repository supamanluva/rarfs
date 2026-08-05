# rarfs

Mount a directory tree of RAR-archived videos as a read-only filesystem — play
them directly in **Plex** or **Jellyfin** without extracting anything first.

Inspired by [rar2fs](https://github.com/hasse69/rar2fs), rewritten in Rust with a
fast path for the common case: most scene/P2P video releases are RAR'd in
*store* mode (no compression), and for those, rarfs never decompresses anything.

```text
/media/movies/
├── Some.Movie.2024.1080p/
│   ├── some.movie.2024.1080p.part01.rar   ─┐
│   ├── some.movie.2024.1080p.part02.rar    │  47 volumes, 15 GB packed
│   └── ...                                ─┘
└── ...

/mnt/plex/  (rarfs mount of /media/movies)
└── Some.Movie.2024.1080p/
    ├── some.movie.2024.1080p.part01.rar   (volumes pass through untouched)
    ├── ...
    └── some.movie.2024.1080p.mkv          (15 GB, playable, seekable)
```

## How it works

rarfs parses RAR4/RAR5 headers itself (no decompression) and builds a **segment
map** per archived file: which volume file, at which byte offset, how many bytes.

- **Stored members** (method `-m0`, the usual case for video): reads are served
  by `pread()` directly on the volume files. Stateless, lock-free, and the
  **kernel page cache** does all caching. Seeking/scrubbing costs nothing extra,
  memory usage stays in the tens of MB, and Plex library scans (which read the
  head and tail of every file) are fast.
- **Compressed members**: decoded via rarlab's unrar (vendored at build time,
  linked through a small C++ shim) with an 8 MiB sliding window. Sequential
  playback streams fine; short backward seeks are served from the window;
  larger jumps re-decode — slow but correct, and rare in store-mode collections.

Supports RAR4 and RAR5, old-style (`name.rar` + `name.r00…`/`s00…`) and
new-style (`name.partNN.rar`) volume naming, multi-file archives, unicode
names, and files larger than 4 GB.

## Requirements

Everything below is for Debian/Ubuntu; on other distros install the
equivalents.

| Need | Package / where to get it |
| --- | --- |
| FUSE 3 runtime + headers | `sudo apt-get install -y fuse3 libfuse3-dev` |
| Rust toolchain (stable) | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh -s -- -y` (see [rustup.rs](https://rustup.rs)) |
| C++ compiler (builds unrar) | `sudo apt-get install -y g++` |
| rarlab unrar source | fetched by `scripts/fetch-unrarsrc.sh` (not redistributed here for license reasons) |
| `rar` binary | only for the test suite's RAR4 fixtures; not needed to build or run |

## Build

```bash
git clone https://github.com/supamanluva/rarfs.git
cd rarfs
scripts/fetch-unrarsrc.sh      # downloads unrar source into vendor/unrarsrc
source "$HOME/.cargo/env"      # fresh rustup installs only
cargo build --release
# binary: target/release/rarfs
```

## Usage

```bash
mkdir -p /mnt/rarfs            # mount point must exist and be empty
target/release/rarfs <SOURCE_DIR> /mnt/rarfs [--log FILE] [--allow-other]
# unmount: fusermount3 -u /mnt/rarfs
```

- The mount mirrors the source tree read-only. Real files (including the
  `.rar` volumes themselves) pass through untouched; each archive set
  additionally exposes its contents as regular files in the same directory
  (member paths are flattened to base names).
- New archives appear without remounting (directory cache is
  mtime-invalidated).

### Plex / Jellyfin

The media server usually runs as a different user, so mount with
`--allow-other` and enable it system-wide once:

```bash
grep -q '^user_allow_other' /etc/fuse.conf || echo 'user_allow_other' | sudo tee -a /etc/fuse.conf
sudo mkdir -p /mnt/plex
rarfs /media/movies /mnt/plex --allow-other --log /var/log/rarfs.log
```

Then point your library at `/mnt/plex`. A systemd unit or `@reboot` cron entry
is the easiest way to make the mount permanent (the binary runs in the
foreground).

## Behavior and limitations

- **Read-only** by design.
- **Missing or corrupt volume** → `EIO` only for reads spanning that segment;
  the rest of the file still plays.
- **Incomplete sets** (missing last volume, or store-mode members whose packed
  data is short) are hidden from listings and logged, never half-exposed.
- **Unparseable archives** are hidden and logged (`hiding unparseable set …`);
  everything else keeps working. Run with `--log` to diagnose.
- Compressed members: seeking backwards further than the 8 MiB window
  re-decodes from the start — fine for occasional use, not for heavy scrubbing
  of compressed files.
- Old-style naming supports followers up to `.z99` (`r00…r99`, `s00…`, …).

## Testing

```bash
cargo test
```

41 tests: byte-level parser round-trips (hand-built RAR5 fixtures, real `rar`
RAR4 fixtures incl. multi-volume and unicode names), randomized seek
verification, a 10 MiB backpressure stream, and a real in-process FUSE mount
test. For real-world validation against an actual media collection, see the
10-step checklist in [docs/soak-test.md](docs/soak-test.md).

## License

rarfs itself is [GPLv3](LICENSE). The build downloads rarlab's unrar source,
which is covered by its own license (see `vendor/unrarsrc/license.txt` after
fetching); it may not be used to develop a RAR-compatible archiver.

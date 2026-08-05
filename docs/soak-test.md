# rarfs soak test runbook

Manual end-to-end validation against the real media collection before trusting
rarfs with the Plex/Jellyfin library. Run top to bottom; each step lists what
to check and what a failure looks like. Estimated time: 30–60 minutes, mostly
playback.

Prerequisites: release build environment (`source "$HOME/.cargo/env"`),
`unrar` on PATH, `mpv` or VLC, and `fusermount3` (fuse3 package).

## 1. Pick a sample subtree

Choose a directory of the real collection — or assemble a scratch copy with
symlinks — that contains all of:

- [ ] At least one **RAR4** set in `name.rar` + `name.r00`, `name.r01`, … form.
- [ ] At least one **RAR4** set in `name.partNN.rar` form, if your collection
      has any.
- [ ] At least one **RAR5** set (`name.part01.rar` / `name.part1.rar` …).
- [ ] At least one file **2 GB or larger** (multi-volume, so seeks cross
      volume boundaries).
- [ ] Any **compressed (non-store) sets** you know of — note them explicitly;
      they exercise the `UnrarReader` decompression fallback instead of the
      `StoreReader` fast path.

Write down which sets are which so you can check them off in step 4.

## 2. Build

```bash
cd ~/rarfs
source "$HOME/.cargo/env"
cargo build --release
```

- [ ] Build completes with no errors (warnings are acceptable; note any new
      ones).
- Failure looks like: compile errors — stop and fix before proceeding; nothing
  else in this runbook is meaningful without the binary.

## 3. Mount

```bash
mkdir -p /tmp/rarfs-mnt
./target/release/rarfs /path/to/sample /tmp/rarfs-mnt --log /tmp/rarfs.log
```

The process stays in the foreground; run the remaining steps from another
terminal.

- [ ] Command returns to a running (blocked) process with no error output.
- [ ] `mount | grep rarfs` shows the mount.
- Failure looks like: `mount failed` (mountpoint not empty, fuse3 missing) or
  `source dir not accessible` (bad path/permissions).

## 4. Compare listings

```bash
ls -la /tmp/rarfs-mnt/
ls -la /tmp/rarfs-mnt/<subdir-with-a-set>/
unrar l /path/to/sample/<subdir>/SomeTitle.rar        # or .part01.rar
```

- [ ] Every archive set from step 1 shows its archived video file (base name
      only — paths inside the archive are flattened) **in the same directory
      as the set's first volume**.
- [ ] The size shown by `ls -la` matches the size `unrar l` reports **exactly,
      byte for byte**.
- [ ] The raw volume files (`.rar`, `.r00`, …) also pass through untouched —
      this is expected; media servers only scan video extensions.
- Failure looks like: a set's video is missing from the listing → check the
  log for a `hiding unparseable set` warning naming that set (see step 8).
  A wrong size means a header-parse or segment-map bug — stop and report.

## 5. Playback and scrubbing

Play a 2 GB+ file directly from the mount:

```bash
mpv /tmp/rarfs-mnt/<path>/BigMovie.mkv
```

- [ ] Playback starts within a second or two and plays smoothly.
- [ ] Scrub to several positions: the middle, ~75 %, and **inside the last 10
      minutes**. Each seek resumes playback within a second or two. Late seeks
      exercise cross-volume seeks in `StoreReader` — the segment at the end of
      the file lives in the last volumes.
- [ ] No visual corruption (macroblocks, smeared frames) after a seek — that
      would indicate the segment map is serving bytes from the wrong offset.
- Failure looks like: seeks hang or the player reports an I/O error. If the
  set is intact, that is a `StoreReader` bug; check the log and note the
  position where it failed.

## 6. Concurrent reads + integrity spot-check

While one file is playing in mpv, start a second player on a different file,
then in a third terminal:

```bash
find /tmp/rarfs-mnt -name '*.mkv' -exec md5sum {} \;
```

Pick one set and compare against a manual extraction pipe:

```bash
unrar p -inul /path/to/sample/<set>/Title.rar | md5sum
md5sum /tmp/rarfs-mnt/<set>/Title.mkv      # from the find output above
```

(Use `unrar p -inul <first volume>` for RAR5/part sets too — unrar follows the
volumes automatically.)

- [ ] Both streams play simultaneously without stuttering.
- [ ] The two md5 hashes match.
- Failure looks like: hash mismatch — data corruption in the segment map or
  reader; stop and report the set name. Stuttering under concurrency alone is
  worth noting but may be disk-bound on the underlying volumes.

## 7. Plex/Jellyfin access (`--allow-other`)

Skip this step if the media server runs as your own user. Otherwise:

```bash
fusermount3 -u /tmp/rarfs-mnt
grep -q '^user_allow_other' /etc/fuse.conf || echo 'user_allow_other' | sudo tee -a /etc/fuse.conf
./target/release/rarfs /path/to/sample /tmp/rarfs-mnt --log /tmp/rarfs.log --allow-other
```

- [ ] As the plex/jellyfin user, the mount is readable, e.g.
      `sudo -u plex ls /tmp/rarfs-mnt/`.
- [ ] Point a **test** library at the mount and let it scan; extracted videos
      appear and one of them plays through the server's own player.
- Failure looks like: permission denied for the service user →
  `user_allow_other` missing from `/etc/fuse.conf`, or the `--allow-other`
  flag was omitted.

## 8. Log review

```bash
grep -i warn /tmp/rarfs.log
```

- [ ] Investigate every `hiding unparseable set <path>: <error>` warning.
      Each one names a set whose contents were hidden from listings. Common
      benign causes: incomplete downloads (missing volumes), corrupt archives.
      Confirm each warned-about set is genuinely broken with
      `unrar t <first volume>`; if unrar reads it fine but rarfs hid it,
      that's a header-parser bug — keep the set aside and report it.
- [ ] No `ERROR`-level lines other than ones you deliberately triggered.

## 9. Memory check

During playback of a large file:

```bash
ps -o rss= -p $(pgrep rarfs)
```

- [ ] RSS stays in the **tens of MB** (roughly < 100 MB) and does not climb
      steadily during sustained playback or scrubbing. rarfs keeps no
      userspace block cache — reads go through the kernel page cache — so
      growth proportional to playback time or file size would indicate a
      leak.
- [ ] Re-check after step 6's concurrent load; RSS should settle back to the
      same range.

## 10. Unmount

Stop any players reading from the mount, then:

```bash
fusermount3 -u /tmp/rarfs-mnt
```

- [ ] The rarfs process exits cleanly (exit code 0, no panic in its terminal
      or at the end of `/tmp/rarfs.log`).
- [ ] `mount | grep rarfs` shows nothing.
- Failure looks like: `target is busy` → a player or shell still has a file or
  cwd under the mount; close it and retry. A panic on shutdown is a bug even
  if everything else passed — save the log.

## Sign-off

All boxes checked, hashes match, RSS in range, clean unmount → rarfs is good
to point at the full library. Keep `/tmp/rarfs.log` until the first real
library scan completes.

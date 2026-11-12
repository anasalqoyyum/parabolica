# Album tag and artwork editing

Issue [#12](https://github.com/anasalqoyyum/parabolica/issues/12) adds reviewed album metadata and artwork writes on top of master `0d28ed5532524d5c2ef9d2f593a2e6a0b055cf79`. This document separates the Linux automated evidence from the Windows checks that have not run yet.

## Entry point

Open an album and choose Files & tags. The page reads every member file, hashes it, and shows:

- **Album fields.** Album, album artist, date, genre, style, label, catalog number, country and disc total, each with the current value (or "Mixed: n values"), a new value and Keep / Replace / Fill empty. Typing a value selects Replace. Multi-value fields split on `;`.
- **Tracks.** Each file with its current title, artist and track number, an Include checkbox and per-file overrides for those three fields. A track override replaces the album choice for that file. Files that cannot be written show why: missing or offline, a format this build cannot write, or a read error.
- **Artwork.** Front cover, back cover and booklet pages. Each lists how many files embed one and which sidecar images exist beside the files. Choose image validates the file before anything else happens. The destination is Embed in files, Save beside files or Both. Existing embedded pictures and existing sidecars (`cover.*`, `folder.*`, `front.*`, `back.*`, `booklet*.*`) are kept unless their Replace checkbox is ticked.
- **Review.** Review changes lists every field and picture change per file and every sidecar to write, then Write N files runs the job. Changing any choice discards the review.
- **Write history.** Each write shows per-file and per-sidecar outcomes, with Retry unfinished, Restore original per written file, and Delete backups.

Nothing touches a music file or folder before Write. After a write the page asks the library to rescan, so the collection shows the new tags.

## Contracts

`parabolica-tags` has no storage, UI or audio dependency.

- `write::prepare` refuses a read-only file, then compares size and modification time with the reviewed signature. It reads the original once, hashing all of it, and writes a temporary sibling `.<name>.<job id>.parabolica-tmp`: the edited metadata blocks followed by the original audio frames copied byte for byte. It syncs the file, re-reads it, and checks that the blocks equal the intended blocks and the audio bytes hash to the original audio. A digest that differs from review fails with Changed. It then copies the original to the backup folder. An edit that changes nothing returns Unchanged and writes nothing.
- `Prepared::commit` checks size and modification time again and renames the temporary file over the original. Any failure before or during the rename removes the temporary file and the backup and leaves the original as it was.
- Only the Vorbis comment block and the replaced PICTURE blocks are re-encoded. STREAMINFO, SEEKTABLE, APPLICATION, CUESHEET, padding, the comment vendor string, untouched comments (including invalid UTF-8) and their order stay byte-identical. A replaced field takes the position of its first existing entry and removes its aliases (`TRACKTOTAL`/`TOTALTRACKS`, `DISCTOTAL`/`TOTALDISCS`, `ALBUMARTIST`/`ALBUM ARTIST`). A picture replaces only pictures of its own type code: 3 front, 4 back, 5 booklet. Icons, artist photos and other types are never removed. Writing the same edit again is Unchanged, so retries cannot duplicate pictures.
- Images must be JPEG or PNG, at most 16 MiB minus FLAC picture header room, at most 10,000 px per side and 40 megapixels. They must decode completely within a 256 MiB allocation limit. The bytes are embedded unchanged.
- `job::run` records each file as Replacing (backup path and expected content) before the rename and with its outcome after it, through a `Ledger`. Outcomes are Written, Unchanged, Changed (needs a new review), Deferred, Failed and, after the job, Restored. The caller's in-use check defers the file the player has open. A sharing or lock violation on Windows is Deferred as well. Cancellation stops between files.
- `job::recover` resolves an interrupted target from the file's content. If it matches the expected digest the target is Written. If it matches the reviewed digest it is Failed and the backup is removed. Anything else is Changed. The temporary file is removed either way.
- Sidecars are written once per folder and picture type through a temporary file and rename. Reviewed existing sidecars are backed up and removed just before the new file is renamed into place, so `Cover.PNG` becomes `cover.png` on case-sensitive and case-insensitive folders alike. If the rename fails, their backups are copied back. A sidecar that appeared after review fails as Changed. Sidecar outcomes are reported separately from file outcomes.

The ReplayGain writer (#11) and the Discogs import (#21) build `TagPatch`es with their own fields and use the same `job::run`/`write::prepare` path.

Migration `0012_01_tag_writes.sql` stores each write as a `jobs` row of kind `tag-write`. It also stores the reviewed patch per file (fields, values, artwork), the staged image paths, sidecar plans, per-file and per-sidecar states, backup paths and the written signature. `storage::tag_writes` owns the SQL. A job finishes Completed when every file and sidecar succeeded, Partial (`Incomplete`) when some did, Failed when none did, or Cancelled. Retry creates a new job linked by `retry_of` with the retryable files (Pending, Deferred, Failed) and sidecars, reusing the reviewed patch and signature. A file changed by another program is not retried; it needs a new review. Album and file IDs in this history are not foreign keys, so history outlives a file's removal.

The desktop worker (`ui::tags::service`) runs reads, hashing, staging, writes, restores and startup recovery on its own thread. Backups go to `<app data>/tag-backups/<job id>/`, staged images to `<app data>/tag-staging/`. Restore copies the backup back only while the file still has the written content. Delete backups removes the job's backup folder and any staged image no remaining write needs.

Writing the album title can regroup files into a new library album, because the album title is part of automatic grouping. Reload files follows the reviewed files to their current album, and write history includes writes to any of the album's current files.

## Scope limits

- FLAC is the only writable format. MP3, MP4/M4A and WAV files are listed with "files can't be written in this build" and are skipped.
- The final step is `std::fs::rename`, which is `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` on Windows, not `ReplaceFileW`. The replaced file gets the temporary file's creation time and the folder's inherited ACL; alternate data streams on the original are not carried over.
- Any write changes the file's full-content fingerprint, so ReplayGain results for the file become stale. Retargeting analysis after a decoded-PCM equivalence check belongs to the ReplayGain owner.
- One CUE image is one physical file and is written once, as for any file. No CUE-specific UI exists yet (#13).

## Automated Linux evidence

Developed on Linux x86_64 with Rust 1.99.0 and GPUI Kit 0.9.0. These ran from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p parabolica-desktop --locked
```

Fixtures are the synthetic `crates/audio/tests/fixtures/flac/s16_44100.flac`, with comments, an APPLICATION block, icon and front-cover pictures and padding added by the tests, and generated or hard-coded synthetic PNGs.

`crates/tags` tests:

- An edit of title, multi-value genre and front cover leaves every other block byte-identical. The vendor string, an invalid-UTF-8 comment and ReplayGain comments are unchanged. The audio frame bytes are identical, and Symphonia decodes the same integer PCM before and after. The backup equals the original. Repeating the edit is Unchanged.
- Injected disk-full errors after the temporary file, after the backup and before the rename leave the original byte-identical with no temporary file or backup left behind.
- A concurrent edit just before the rename fails as Changed and keeps the other program's content. A same-size edit with a restored modification time is caught by the content digest.
- Read-only and non-FLAC files fail without changes. A FLAC file without a comment block gains one after STREAMINFO.
- A job records Replacing then Written, defers the playing file without touching it, writes `cover.png` and backs up and removes the reviewed `folder.jpg`. Replacing `Cover.PNG` leaves only the new `cover.png`, with the old file backed up. It refuses a sidecar that appeared after review. Restore copies the backup back and refuses once the file differs.
- Recovery resolves an interrupted job as Written after the rename, as Failed with the backup removed before it, or as Changed. Cancellation stops before the first file.

`crates/storage/tests/tag_writes.rs` round-trips patches with multi-value, fill-only, remove and custom fields, artwork and sidecars across a reopen. It checks that outcomes are refused unless the job runs, that final states follow the outcomes, and the restore, backup-discard, retry link and invalid-write rules.

`apps/desktop/src/ui/tags/tests.rs` indexes tagged copies of the fixture as a two-disc album through the real library indexer:

- Album replace, fill-empty, a per-track title override, an excluded file and a front cover written to files and beside them reach the files. Unselected fields, ReplayGain and custom tags, the excluded file and every file's audio bytes are unchanged. The next library pass shows the new album tag.
- A playing file is deferred, and a retry completes it with exactly one front cover in every file.
- A read-only file fails and a file edited after review is Changed, in the same job as a written file. The retry covers only the read-only file.
- Startup recovery resolves a Replacing target without its rename and a Pending target, removes the leftover temporary file and backup, and the retry then writes.
- Reload follows files regrouped by a title change and keeps their history.
- A headless GPUI window drives the page itself: open the album, type an album title, Review changes, Write with one read-only file, then Retry unfinished from the history.

Headless GPUI tests do not activate the native AccessKit tree, and nothing here ran on Windows.

## Windows checks still needed

Run on the Windows machine with `cargo run -p parabolica-desktop --locked`, using copies of disposable FLAC files in their own folder:

1. Real app flow: add the folder, open the album, Files & tags, edit album fields, a track override, front/back/booklet artwork with each destination, Review, Write, then confirm the library shows the new tags and artwork after its rescan.
2. File writes on Windows paths: Unicode, long (>260 character) paths, a network or removable drive if available, and a file with the read-only attribute. Confirm decoded audio is unchanged (for example `ffmpeg -i file -f md5 -` before and after) and that unselected tags and pictures survive in Mp3tag or foobar2000.
3. Replacement while another program has the file open: play it in foobar2000 or keep it open in Mp3tag, write, and confirm the file is Deferred or fails recoverably with the original intact. Check playback in Parabolica itself is Deferred.
4. Concurrent edit: save the file in Mp3tag after review and before Write; confirm Changed and that Mp3tag's edit survives.
5. Interrupted write: end `parabolica.exe` from Task Manager during a large write, restart, and confirm the history shows Written or "Interrupted before replacement" and no `.parabolica-tmp` files remain.
6. Restore original and Delete backups, and check `%LOCALAPPDATA%\Parabolica\tag-backups` afterwards.
7. Disk full: write to a small nearly full volume (for example a VHD) and confirm "Disk is full" with the original intact.
8. Native file dialog for Choose image, including cancel.
9. UI Automation / Narrator: every field input, mode radio, Include checkbox, artwork control, review line and history action is reachable by keyboard and announced with its label.
10. Attributes after replacement: creation time, ACL and alternate data streams of a replaced file, given the rename-based replacement.

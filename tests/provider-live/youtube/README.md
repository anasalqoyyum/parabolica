# YouTube browser access spike

This is the Linux route test for issue #15. It uses a loopback page and the official YouTube IFrame Player API in visible Chrome. It does not implement the production provider, extract audio or inspect the user's existing browser profile.

## Run

Use a desktop session with Chrome installed. The default executable is `/usr/local/bin/google-chrome`; set `CHROME_PATH` to override it. The public test item is YouTube's API-demo video. `YOUTUBE_VIDEO_ID` accepts another 11-character ID, not a URL.

```sh
pnpm install --dir tests/provider-live/youtube --frozen-lockfile
pnpm --dir tests/provider-live/youtube test
pnpm --dir tests/provider-live/youtube probe
```

The probe writes redacted state evidence to `tests/provider-live/youtube/evidence/local-public-iframe.json`. It never logs responses, cookies, credentials or signed media URLs. The temporary Chrome profile is deleted in `finally`. To save a screenshot, set `PARABOLICA_SCREENSHOT_DIR` to the user's Pictures `Screenshots` directory.

The test opens a visible player at 640×360 pixels. It calls `playVideo`, verifies that position advances, calls `pauseVideo` and verifies `seekTo(10)`. A player event or moving position is not audible-output evidence. Account acceptance, entitlement, expiry/reconnect, background behavior and Windows WebView2 integration remain unverified.

The package is a disposable provider-live test, not part of the Rust workspace. It requires network access to YouTube and is not run by the repository's ordinary unit-test command.

This public spike does not choose a final account or WebView2 route. See [the route decision and remaining gates](../../../docs/research/YOUTUBE_MUSIC.md) and [the redacted Linux observation](../../../docs/implementation/evidence/youtube/public-iframe-linux.json).

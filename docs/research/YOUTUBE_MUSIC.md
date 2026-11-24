# YouTube Music playback access and issue #15 plan

Checked 2026-11-07 against current first-party documentation. This is a route decision and an implementation plan for [issue #15](https://github.com/anasalqoyyum/parabolica/issues/15), not proof of authenticated playback. No account, cookie or token was used. The public Linux player result is recorded below.

## Recommendation

Run #15 as a browser playback spike before building the catalog adapter. Test the documented YouTube IFrame player in visible Linux Chrome first, then repeat the selected route in Windows WebView2. Test a full `music.youtube.com` page as a comparison if the IFrame route cannot represent the required Music account or audio behavior. Choose only a route that actually plays the selected Music item, reports its state and supports pause and seek.

The official Data API supplies video metadata and an embed player. It does not expose a YouTube Music audio-download endpoint. Do not port `ytmusicapi` or implement native stream resolution before the browser test. That work would put the unsupported Innertube protocol at the center of the app without answering the access question.

This route can run alongside #7. Keep the spike under `crates/providers/youtube/src/{auth,transport}` and `tests/provider-live/youtube`. Do not edit #7's engine, queue controller or playback command files. Add a native WebView2 child window as a coordinated platform change after the playback test justifies it; do not solve that with unrelated GPUI rewrites.

## What the routes can establish

| Route | Established by documentation | Decision for #15 |
| --- | --- | --- |
| YouTube Data API | Video, playlist and channel operations. `videos.list` returns an embedded-player HTML value, not a general Music audio stream. [D][V] | Optional metadata support after playback works. It is not the playback implementation. |
| Visible IFrame player in WebView2 | The IFrame API supports video loading, play, pause, seek, queueing and player-state events. Google explicitly documents identification for embedded players in WebViews. [P][R] | Test first. It is the narrowest documented embedded-playback route. Account-backed YouTube Music catalog parity and output details still need live evidence. |
| Full YouTube Music page in WebView2 | WebView2 hosts web content and persists its browser profile. No official source reviewed here guarantees YouTube Music sign-in or playback in WebView2. [W] | Comparison test only. Record actual sign-in, selected-item playback and controls before selecting it. |
| `ytmusicapi` or a Rust Innertube client | `ytmusicapi` emulates browser requests and says Google does not support or endorse it. Its `get_song` documentation exposes `playabilityStatus` and expiring `streamingData`. [Y][S] | Do not make this the foundation yet. It needs a separate explicit owner decision after the documented-player test, plus a current policy review and live proof. Personal use is not a permission exception. |
| System browser opens YouTube Music | The browser itself may play music. | Does not satisfy #15's in-app playback proof or the unified-player requirement. |

There is no reviewed official YouTube Music desktop playback SDK or documented Music-specific OAuth scope. Google's normal desktop OAuth flow can authorize documented YouTube API scopes through the system browser and loopback redirect. That does not turn a YouTube Music website cookie into an OAuth access token or grant an audio stream. [O][G]

Do not send Google OAuth through WebView2. Google's OAuth policy prohibits embedded user agents under the developer's control. A website sign-in inside WebView2 and an OAuth authorization request are different flows. If a full-page session is selected, the user interacts only with Google's page; the native app must not read, inject or retain the user's password. [G]

## Constraints that shape the UI

The player cannot be a hidden audio extractor. YouTube's developer policies prohibit separating audio, modifying player functionality and background playback from a player not displayed on the user's current page or screen. The minimum embedded-player viewport is 200×200 pixels. Automatic scripted playback requires the player to be visible and more than half visible. The app must retain YouTube branding and avoid overlays on the player. [T][R]

The required-minimum-functionality document now explicitly covers desktop WebViews. It says an embedded player needs an `HTTP Referer`; where a WebView leaves it empty, the host must supply its identity using the documented approach. A bare `file://` page or an unspecified synthetic origin is not enough. [R]

Premium documentation describes background play in YouTube's mobile apps. It does not grant background playback to a hidden desktop WebView. It also lists an upper bound of 256 kbps AAC or Opus for Music streaming quality, not a promise that this integration receives that format or bitrate. Report only values the selected player actually exposes. [B][Q]

## Spike and implementation sequence

### 1. Prove rendering and control without an account

The Linux probe under `tests/provider-live/youtube` serves a local page to visible Chrome, loads the official IFrame API and checks real player events and position. It uses a temporary browser profile and stores no cookies, responses or account data. Run it with `pnpm --dir tests/provider-live/youtube probe`; its deterministic checks use `pnpm --dir tests/provider-live/youtube test`.

After this route passes, build a minimal Windows host using Microsoft's `windows-webview` crate. It supports hosting WebView2 in a caller-supplied parent `HWND`, which is the seam a later GPUI integration needs. Keep the spike outside the production audio engine. [M]

The host must:

- Create a dedicated WebView2 profile for Parabolica. Do not reuse the user's Edge profile or ask for copied cookie headers.
- Display the official IFrame player at a compliant size with its controls and attribution intact.
- Supply the documented WebView identity and `Referer` behavior. Test this in the actual WebView, rather than assuming a local HTML origin works.
- Send narrow commands for cue/load, play, pause and seek. Observe ready, playing, paused, ended and error states.
- Check the sender origin and validate every browser-to-host message. Use typed messages, not a generic script or native-object bridge. Microsoft recommends this for untrusted WebView content. [W]
- Handle navigation failure, an unavailable WebView2 Runtime and process exit. Recreate the control after browser-process failure; do not delete a live profile directory. [F]

Gate A: a real public, embeddable item advances its playback position and native commands can pause and seek it. A loaded thumbnail, a successful metadata response or an external browser window does not pass. The Linux run establishes this behavior in Chrome only. The Windows run must establish it again in WebView2 and add audible-output evidence. Record the platform, browser or WebView2 Runtime version, test item, elapsed playback and observed state transitions.

### Public Linux result

The structured probe passed on Linux x64 with Google Chrome 156.0.8078.79. Against the base source at `754984d2f8dad70dc664e6a8a4c85ce85a91faa2` plus this spike, the official public API-demo item `M7lc1UVf-VE` advanced to 4.096 seconds, emitted `PLAYING` then `PAUSED`, and seeked to 10 seconds with no reported player errors. Eleven deterministic tests passed, covering input validation, bounded waiting, request filtering, loopback serving and command/error behavior. The redacted result is [public-iframe-linux.json](../implementation/evidence/youtube/public-iframe-linux.json).

This is real playback through the visible browser, not a fixture or catalog query. Player state does not independently establish audible output. No signed-in account, Premium entitlement, account persistence, background behavior, native PCM, output-device control or Windows WebView2 behavior was tested. #15 and its downstream issues remain open.

### 2. Prove the required account behavior

With the owner present for interactive authorization, test a known entitled Music item. Start the item through the provider's own play control if autoplay is restricted. Do not request credentials in chat and do not save protocol bodies containing cookies, authorization headers, signed URLs or account identifiers.

Use a disposable test playlist where account mutations are needed, with separate owner approval for each write. Playback tests themselves are read-only.

Gate B must establish:

- The selected Music item plays, not merely a catalog page or a public substitute.
- The page has a stable account identifier and a safe way to distinguish signed-out, signed-in and expired states.
- Pause, resume, seek and next-item selection act on the displayed player.
- Playback remains correct while the visible app stays open. If the requirement includes audio after closing or hiding that player's page, record it as unsupported rather than hiding the WebView to force it.
- The app can report delivery details that the player exposes. Everything else is `Unavailable`, including native PCM, sample format, ReplayGain applicability and ASIO/exclusive control.

If the IFrame player cannot establish the required Music account behavior, repeat this gate with a visible full YouTube Music page. Compare both results. Select the smaller route that passes. If neither passes, stop with #15 blocked and leave #16/#17 unstarted. Do not quietly substitute public YouTube playback, a browser launch or a fake transport.

### 3. Fit the proven session into the account lifecycle

The current #14 `AuthFlow` returns a token-like `Secret` that `Account` saves through `CredentialStore`. A pure WebView2 session instead stores cookies in its browser profile. Credential Manager must not contain a second copy of those cookies.

Make one coordinated change to the provider contracts and account registration:

- Add a session kind for OAuth credentials versus a browser-owned profile. Store only a non-secret account/profile reference in Credential Manager for the latter, if one is needed at all.
- Keep cancellation, wrong-account rejection, generation-tagged requests and `SignedOut` events. These parts of #14 still apply.
- On restore, inspect the visible WebView session and publish the observed account state. Do not mark the account connected merely because a profile directory exists.
- On disconnect, stop playback and clear the provider profile's cookies and browsing data using WebView2's documented API. Wait for browser shutdown before deleting any profile files. Verify that a fresh session is signed out. [C][F]
- Keep account lookup off the UI thread and route all browser calls through the UI-owned WebView controller.

This is preferable to exporting WebView cookies into the existing token store. Cookie export would enlarge the credential boundary and bypass the browser's own storage protections.

### 4. Expose a narrow contract to #16 and #17

After gates A and B pass, publish the provider support matrix from observed behavior:

- `Playback` and `Seek` only after their live checks pass.
- `AppOwnedPcm` never for a browser-owned route. `Gapless`, native ReplayGain and native output selection remain unsupported there.
- `QualityInspection` only for fields the selected player exposes. Browser output is `Managed by YouTube Music/browser` in the existing signal-path contract.
- Catalog, playlists, likes and lyrics remain undeclared until their own issues implement and verify them.

Give #17 a `ProviderPlaybackSession` interface with cue/play/pause/seek/stop, the provider item identity, generation-tagged events and a terminal error. Do not give it cookies, signed media URLs, a JavaScript evaluation primitive or arbitrary navigation. The provider controller owns WebView lifetime and stops the old backend before local playback resumes. That integration belongs to #17 after #15 and #7 merge, not to the #15 spike.

Give #16 a separate catalog transport interface only after choosing whether it uses the same visible browser session or a separately approved API. Do not infer that a working player authorizes arbitrary background Innertube calls.

## Evidence still required from the owner

One interactive Windows session with the owner's entitled YouTube Music account and a known playable item. No password, cookie, token or exported browser profile should enter the conversation or repository. Save redacted state-transition logs and screenshots without account details or signed URLs.

No account step is needed to implement the public-player spike and its deterministic command, origin-validation and bounded-failure tests. Account-free work cannot complete #15. Browser-process recovery belongs to the Windows WebView2 host; this Linux harness does not claim to test it.

## Sources

- [D] [YouTube Data API reference](https://developers.google.com/youtube/data/reference/rest): documented resources cover videos, playlists, channels and related YouTube operations.
- [V] [`videos.list`](https://developers.google.com/youtube/data/reference/rest/v3/videos/list): the `player` part supplies embed HTML; the listed parts are metadata, not an audio-download grant.
- [P] [YouTube IFrame Player API](https://developers.google.com/youtube/iframe_api_reference): video queueing, play, pause, seek and state events.
- [R] [YouTube API Services Required Minimum Functionality](https://developers.google.com/youtube/terms/required-minimum-functionality): WebView identity/`Referer`, minimum player size, visibility and overlay requirements.
- [T] [YouTube API Services Developer Policies](https://developers.google.com/youtube/terms/developer-policies): branding, player modification, audio separation and background-player restrictions.
- [O] [OAuth 2.0 for desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app): system browser and loopback flow for documented API scopes.
- [G] [Google OAuth 2.0 policies](https://developers.google.com/identity/protocols/oauth2/policies): prohibition on developer-controlled embedded OAuth user agents.
- [W] [Develop secure WebView2 apps](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security) and [WebView2 best practices](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/developer-guide): origin validation, typed messages, user-data-folder lifetime and runtime handling.
- [F] [WebView2 process-related events](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events): process-failure recovery and safe profile cleanup ordering.
- [C] [Clear WebView2 browsing data](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/clear-browsing-data): clearing selected or all profile data, including for sign-out.
- [M] [`windows-webview` crate documentation](https://github.com/microsoft/windows-rs/blob/master/crates/libs/webview/readme.md): WebView2 hosting and the caller-owned parent-window integration.
- [Y] [`ytmusicapi` project documentation](https://github.com/sigma67/ytmusicapi/blob/488dbf31d8f9173331c80375fdc2fb6de6d45f6d/docs/source/index.rst): unofficial browser-request emulation, not supported or endorsed by Google.
- [S] [`ytmusicapi.get_song`](https://github.com/sigma67/ytmusicapi/blob/488dbf31d8f9173331c80375fdc2fb6de6d45f6d/ytmusicapi/mixins/browsing.py): `playabilityStatus`, `streamingData`, format metadata and expiry fields. This is the third-party protocol behavior, not an official access guarantee.
- [B] [YouTube Premium benefits](https://support.google.com/youtube/answer/6308116?hl=en): background-play documentation for YouTube's mobile apps.
- [Q] [YouTube Music audio quality](https://support.google.com/youtubemusic/answer/9076559?hl=en): stated 256 kbps AAC/Opus upper bound.

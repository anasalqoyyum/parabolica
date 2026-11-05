# Parabolica design proposal

Status: proposed, awaiting user feedback. These are editable mockups, not an approved or implemented application.

## Direction

The supplied desktop image is the visual authority. Keep its square album art, library sidebar and persistent bottom player. Preserve its light appearance and provide a matching dark appearance, as the user requested. Adapt the macOS window decoration to Windows and give local collection work the same prominence as streaming discovery. Follow system, Light, and Dark are required settings.

Impeccable's Operate guidance informs the hierarchy: familiar controls, compact useful information, restrained expression and clear state. The chosen mood is a quiet listening room in daylight. A cooler porcelain palette preserves the supplied reference without turning the app into a paper or hardware imitation. Album covers provide most of the color.

UI copy names controls, reports values, explains non-obvious consequences, or offers recovery. Remove generic page subtitles, self-evident helper text, repeated status prose, and commentary about themes applying across the player or preserving playback. Keep write destinations, driver constraints, errors and signal-path uncertainty. Do not fill empty space with explanatory text.

The selected implementation stack is Rust with GPUI Kit. Use `gpui-component` for standard controls and lists/tables, and compose the custom player views with Kit's GPUI and `gpui-base` APIs. Map these tokens to Kit's theme system while preserving the proposed layouts.

Other directions considered were a dark listening view, an expressive record-store layout, a dense collector table, a streaming discovery feed, a sleeve-notes layout, and a minimal player. The sleeve-notes idea informs the relationship between album art and its track list; it does not replace familiar app navigation. Full-screen media and handwritten cassette styling obscure collection work. A specimen-like discipline in numeric columns improves scan readability without making typography the subject. These are rationale for this proposal, not user-approved choices. Impeccable seed reference: `seed:8e884e61a615ab37`. The user's reference constrains the result.

## Tokens

| Role | Light | Dark |
| --- | --- | --- |
| Window and main content | `#FAFAF9` | `#17191C` |
| Sidebar and secondary surfaces | `#F0F1F2` | `#202328` |
| Raised panel | `#FFFFFF` | `#282C32` |
| Primary text | `#24272B` | `#ECEEF1` |
| Secondary text | `#62686F` | `#ADB4BE` |
| Rule and input border | `#DDDFE2` | `#3B424D` |
| Primary action | `#24272B`, white text | `#ECEEF1`, `#1B2028` text |
| Focus and selected detail | `#315897` | `#ADCAFF` |
| Selection surface | `#E8EEF7` | `#283B56` |
| Success | `#34694D`, on `#EDF5EF` | `#A7DABA`, on `#253B2D` |
| Warning | `#80591F`, on `#FBF3E5` | `#F2CE8C`, on `#433723` |

Use semantic roles throughout the shell, player, forms, selection, inspector and status messages. Dark mode raises panels above the window through surface tones rather than inverting the artwork. The saved mode and resolved theme are separate values. Follow system is the proposed default and observes Windows app-theme changes; explicit Light/Dark remains fixed. Resolve before the first draw and update without resetting audio, focus, queue or scroll position. High contrast is a separate preference.

Use Segoe UI Variable / Segoe UI for the Windows implementation, with a sans-serif mockup fallback. Typography is functional UI typography: 30 px page titles, 22 px section titles, 15 px body, 13 px labels and 12 px captions. Numeric measurements use tabular figures. Japanese metadata uses the system's appropriate CJK fallback. No decorative display face is needed for this native operating interface.

Base spacing is 4 px. Main content padding is 32 px; section gaps are 24–32 px. Desktop sidebar is 224 px. Bottom player is 104 px. The library shows five columns of 184 px artwork at the proposed size. Buttons are 36–40 px tall; icon hit areas are at least 32 px in dense desktop controls. Artwork radii are 8 px. Panels use 12 px corners. Use thin rules for organization, with one soft shadow only on floating layers.

## Screens and behavior

- Albums shows an artwork grid and source filters. The sidebar keeps Albums, Songs, Artists, playlists, scanner and sources within reach. Metadata counts are secondary.
- Album detail pairs a large sleeve with ordered tracks. Play album is primary. A compact ReplayGain state links to scanning. Local format is a source property.
- Files & tags must offer an album-level Update from Discogs flow: choose a release, map all album/disc tracks, review current/proposed fields and artwork, then apply the selected changes. Show edition details and unresolved mappings where the user chooses them. Keep write destinations and image replacement explicit. This flow is specified in `docs/TECH_SPEC.md`; its detailed Doop screens have not yet been drawn. The research note records unresolved source-data/artwork use constraints.
- ReplayGain is a work page, not a blocking dialog. The left side shows album grouping and per-track results; the right side shows analysis settings and save destination. Saving to the app library is the default. Tag writes are a separate deliberate action.
- Audio signal path expands above the player and remains anchored to its Signal path button. It reads in processing order. Source, app output and endpoint information occupy separate rows. Resampling, active gain and uncertainty are explicit. The streaming state uses Unavailable where native output cannot be inspected.
- Sources and output settings show folder health, the experimental account connection, selected device and mode. Account status is separate from audio-device status.
- Listening device separates WASAPI Shared, WASAPI Exclusive, and ASIO from the selected endpoint or driver. The ASIO example uses the user's FiiO K3, output channels 1–2, a 96 kHz source-following stream, and a driver-managed 256-frame buffer. Buffer duration and reported latency are distinct. A driver-panel action sits beside the buffer information.
- Appearance offers Follow system, Light, and Dark with small previews and a selected-state indicator. Its Light artboard starts with Light selected and its Dark artboard starts with Dark selected, regardless of the reviewer's OS theme. Selecting Follow system explicitly enables browser color-scheme observation in the mockup. The proposed app still defaults to Follow system. A shortcut in the top bar and the settings navigation makes the setting discoverable; native Windows observation remains implementation work.

The ASIO inspector marks the Windows mixer bypassed and Windows session volume outside the path. ReplayGain, app volume, sample conversion, channel routing and driver/device uncertainty remain visible. It makes no bit-perfect claim. The FiiO driver label, negotiated sample type, buffer size and capabilities are illustrative. Enumerate the user's installed driver during implementation.

The signature interaction is the player’s compact path summary expanding into a stage-by-stage explanation of what is happening to the music. It is an inspector, not a quality score.

## Layout and accessibility

The proposal uses 1440 × 960 Windows desktop artboards. The canvas has Light and Dark pages with matching seven-screen flows. At 1280 px, reduce album columns and main padding; at 1024 px, collapse the sidebar to a labelled rail with a reveal control. Long track titles truncate visually with full accessible text. Keep numeric columns aligned. A narrow window may scroll its content while the player remains visible. Mobile needs a separate design.

Use a visible focus ring, 4.5:1 normal text contrast, text plus icon for health states, and no automatic motion. Optional panel motion lasts about 140 ms and respects reduced motion. Escape closes the signal inspector and returns focus. Space toggles playback outside text inputs, Ctrl+F focuses search, and media keys work when the window is unfocused. GPUI accessibility remains a feasibility gate.

## Content and delivery

The album selection follows the user's reference. The rendered screens use verified public Apple catalog artwork URLs because Doop's screenshot renderer could not load its private upload host. Source URLs and local review copies are recorded in `docs/design/assets/catalog.json`. The reference artwork atlas is retained separately. Counts, track names, file paths, account states, scan results, devices and playback measurements are illustrative. They demonstrate UI hierarchy; they are not a live library or measured audio path. The mockups carry a visible Illustrative data label. A Google Fonts Noto Sans JP fallback supplies Japanese glyphs in the browser mockups; the native implementation uses Windows font fallback.

Doop canvas and frame links are recorded in `docs/design/README.md`. Self-contained HTML snapshots are design artifacts for review. The selected application stack is Rust with GPUI Kit; application implementation is pending.

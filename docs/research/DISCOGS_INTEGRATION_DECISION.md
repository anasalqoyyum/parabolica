# Discogs access and durable import decision

Verified 2026-11-06 in Asia/Jakarta. The recorded run was 2026-11-05 18:39:24 through 18:40:02 UTC. Source commit `815207e200c8e7d633458c2682ebfcb239043fc2`. Investigation for [issue #19](https://github.com/anasalqoyyum/parabolica/issues/19), using gpt-6.1-sol with high reasoning effort.

Status: investigation complete for the revised personal-use scope. Proceed with permanent tags from covered CC0 catalog metadata and the observed public read endpoints. The owner has explicitly chosen to defer authenticated access and withdrawn Discogs artwork embedding. Local artwork with optional cropping is specified, and Cover Art Archive is evaluated as an optional source. No provider clarification or account run is required to finish this scoped decision. This completes the research decision, not an implemented importer.

## Metadata and access decision approved by the owner

On 2026-11-06 the owner confirmed that CC0 catalog metadata can be used for permanent tagging and directed the project to use the working public requests without authenticated access for now. Adopt that implementation decision. Release titles, tracklists, artist names, credits, barcodes and the other covered CC0 catalog fields may be written permanently into the user's local tags, including for offline playback and later manual edits. Keep release identity and source provenance for reimport. The CC0 grant is not a temporary metadata license. [T][Z]

Keep API access conditions distinct from the copyright/database-rights grant. Apply source freshness, transient storage and linked attribution to the Discogs lookup/review data; treat accepted CC0 file tags as the user's durable local metadata. This is the owner-approved interpretation for the personal app, not a new written exception from Discogs. Do not require a provider reply, a periodic retag or a deletion job for the user's saved tags. The broader API wording remains in the evidence so this decision does not misstate what Discogs has said.

Use unauthenticated search/release/master/version GETs, as observed. Authenticated access, quota and token lifecycle are untested and deferred by the owner, not failed acceptance checks. If public access later stops working, preserve staged changes and report the unavailable lookup; evaluate authentication then. Do not add a token setup requirement now or assume the public behavior is guaranteed indefinitely.

This decision applies to the covered catalog metadata. Images, user/community information and marketplace prices are not made CC0 by being present in the same JSON response. They must not enter the durable metadata patch. Cover Art Archive's image copyright status is likewise unchanged by the metadata decision.

## Artwork direction after the owner decision

On 2026-11-06 the owner said to stop pursuing artwork embedding from Discogs and asked about Cover Art Archive, user-supplied artwork and optional cropping. The owner explicitly requires advisory size recommendations and support for vertical covers. That instruction supersedes the earlier mandatory Discogs embedding requirement. The earlier unsent provider request has not been approved for submission.

Recommend a local **Choose image** action and optional **Find artwork** using Cover Art Archive. Choosing an image reads a local file; it does not upload the user's artwork to Discogs, MusicBrainz or Internet Archive. Artwork selection remains separate from the Discogs metadata proposal. Do not fall back to Discogs images for a permanent cover file when another source has no match. The owner withdrew embedding specifically; Discogs sidecar permission has not been established or newly approved.

Cover Art Archive is technically suitable for release-art lookup, but it is not a copyright-free alternative. Its landing page says "All images are copyrighted by their respective copyright owners." MusicBrainz's archive documentation also says the images are collected for archival purposes and used at the consumer's own risk. This establishes public access, not a blanket license to embed, redistribute or modify every returned cover. The lack of an explicit license field in the tested response must not be interpreted as CC0. `approved` means community edit approval, not copyright clearance. [CAA][CAAINFO][CAAAPI]

The [follow-up public evidence][CAAE] records four first-party source fetches and a real unauthenticated listing request for release MBID `76df3287-6cda-33eb-8e9a-044b5e15ffdd`. It returned 307 to Internet Archive, then 302 to an Archive download host, then 200 JSON with six images. The first was marked Front. No image bytes were downloaded or decoded, and no embedding/cropping or legal clearance was tested. The JSON has image IDs, types, approval flags, image URLs and thumbnails, with no standard image-license field in this sample.

The integration plan is:

- Resolve a MusicBrainz **release MBID**, using an existing trusted ID or a user-confirmed release search. A Discogs release ID cannot be passed directly to Cover Art Archive. Artist/title/barcode suggestions must still preserve edition choice.
- Fetch `/release/{mbid}/` to let the user choose a Front, Back, Booklet or other image. Use thumbnails to browse. A release-group result can represent another edition; offer it explicitly rather than silently substituting its art.
- Keep the image's source release MBID and archive image ID separate from the Discogs metadata release ID. Show a linked Cover Art Archive/MusicBrainz source beside the candidate. Do not label CAA art as provided by Discogs.
- Follow bounded redirects only to validated service/archive hosts over HTTPS, with no provider credentials or user file paths. Handle missing artwork, redirects to missing thumbnails, network failure and 503 without changing local artwork. The CAA documentation currently states no rate-limiting rules, while still documenting 503 responses; use bounded concurrency/backoff. MusicBrainz release search separately requires a meaningful User-Agent and no more than one request per second. Its public metadata reads need no API key. [CAAAPI][MBAPI]

This is a useful optional acquisition route for the personal app, with the source's copyright status accurately recorded. It does not establish that every archived image is licensed for every use, and it is not a substitute for the local file picker. There is no new requirement to contact a provider before building the independent local-image workflow.

### Local image selection and cropping

Keep this behavior independent of online metadata and artwork access:

1. Choose a local image or drop it into the album's artwork editor. Show the full image, dimensions, format and estimated output size. Keep the selected source file untouched.
2. Default to **Keep original** with the full aspect ratio. Vertical, landscape, panoramic and small images remain valid. The player and editor should fit the whole artwork with padding when their container has a different ratio; do not automatically crop or stretch it into a square.
3. Offer **Crop** as an optional action with a movable, resizable freeform selection, an optional square preset, reset and cancel. Preview the output before applying. Keep the original source plus crop coordinates until the user accepts the edit, so resetting does not require recovering overwritten pixels.
4. A suggested helper is "Recommended: about 1000 px on the longest edge. Any aspect ratio is supported." This is a proposed quality recommendation, not a provider rule, minimum, exact target or approval gate. Show the actual resulting dimensions. Never require a square, reject a small image for quality alone, enlarge it automatically or force resizing to the recommendation.
5. The user deliberately chooses Embed in selected files, Save beside files or Both for the chosen local or otherwise permitted image. Preserve existing covers until replacement is accepted; apply the cropped derivative only to the chosen destinations. Keep artwork failures separate from successful metadata writes.

Actual format decoding, malformed-file rejection, decoder memory limits and destination-format constraints remain necessary technical validation. They must not become arbitrary square/aspect-ratio or aesthetic size rules. For example, a corrupt image can fail; a valid 600×1200 portrait must not fail because it is vertical or below a suggested width. A decoded-image memory limit should offer a recoverable resize path where feasible, rather than silently shrinking the original.

Implementation verification must exercise an uncropped portrait, optional portrait and square crops, a valid small image, reset/cancel, unchanged source bytes, full-image display, existing-cover replacement and independent embedded/sidecar outcomes. These are specified behaviors, not passed app tests: this checkout still has no application implementation. The shared PRODUCT, TECH_SPEC and DESIGN documents retain their earlier artwork wording; their owners need this explicit product update when integrating it. This thread keeps changes within issue #19's assigned files.

## Decision

Use a read-only Discogs REST adapter for on-demand album search, release selection and edition review. Fetch one chosen **release** for the whole album. A master is an edition browser, never the source of a silently selected tracklist. Use the demonstrated public access for the initial personal app. Authentication is a deferred option if access requirements change. [E][C][A]

Separate three uses that have different evidence:

| Use | What the evidence supports | Decision for this delivery |
| --- | --- | --- |
| Persistent local metadata tags | Listed release/artist/label metadata has a CC0 copyright and database-rights grant permitting durable copies. API access conditions are recorded separately. [T][Z] | Proceed with permanent user-confirmed CC0 tags and retained source identity. Apply the owner-approved distinction between fresh API source previews and durable local file metadata. No provider reply is a prerequisite for this scoped decision. |
| Embedded Discogs artwork | The release API returns image descriptors; a returned URL answered HEAD. Images are Restricted Data, with a personal, revocable application-use license and no blanket third-party copyright clearance. [E][T][I] | Withdrawn by the owner's explicit decision. Do not implement or keep asking for permission for this destination. Local/permitted artwork embedding remains part of the independent editor. |
| Discogs sidecar artwork | The same bytes and Restricted Data conditions apply. A separate `cover.jpg` is easier to replace or delete, but is still a durable copy outside an app preview cache. [T] | No supported permission has been established. Do not use it as an automatic fallback for CAA/local artwork. If the owner later wants this source/destination, it needs its own decision. |

There is a concrete supported metadata alternative. Discogs publishes monthly XML dumps under CC0, and its current general terms expressly assign dump content to the license on the dump landing page, separately from API Service Data. A local index built from independently acquired dump records provides a supported basis for durable CC0 metadata, without assuming an exception to API freshness rules. Preserve dump provenance and keep API-derived values out of that write path. This does not license image bytes linked from a dump. [D][S][Z]

The dump/index route has not been selected or implemented and is unnecessary for the chosen public REST workflow. Keep it as an alternative with different freshness, storage and indexing costs. Do not relabel API results as dump data, or propose scraping the website to avoid API conditions. The general terms restrict scraping and contain broad software-development language; the explicit dump and API provisions are the relevant separate grants. [S]

The selected Discogs path is public read access, a release-based album proposal, short-lived attributed source previews and permanent CC0 metadata tags after user review. Artwork uses the separate local/permitted-source editor. The owner's decisions resolve the earlier metadata and account input requests for this investigation.

## Dated primary evidence

Every source below was fetched during the recorded run. [The capture][E] retains URLs, UTC timestamps, HTTP status, selected headers, original-body SHA-256 and relevant text excerpts or redacted JSON. Source names below identify entries in `sources`; API observation names identify entries in `observations`.

| Source | Provider date or version | Relevant evidence |
| --- | --- | --- |
| [API Terms of Use][T], `api_terms` | Text says last updated 2025-05-27; Help Center `updated_at` is 2025-07-23 | Content definition; CC0 versus Restricted Data; API use and restrictions §3 and §5; intellectual-property notices. |
| [General Terms of Service][S], `terms_of_service` | Text and Help Center update 2026-02-23 | User content distinguishes Data Dump licensing from API Service Data. License and site access restricts downloading, scraping and dataset transfer. |
| [Image guidelines][I], `image_guidelines` | Help Center update 2026-04-10 | Uploads may be public domain, CC0 or fair use for specified purposes. These are alternative submission bases, not a license label on each API image. |
| [Application name policy][N], `application_name` | Effective 2019-12-11; Help Center update 2024-03-20 | Accurate integration descriptions are allowed; implied sponsorship is not. |
| [Account settings][A], `account_settings` | Help Center update 2025-07-23 | Developers settings offers application registration and API-token creation. |
| [Master guidelines][M], `master_guidelines` | Help Center update 2026-03-31 | Releases in a master can have differing tracklists. |
| [Tracklisting guidelines][L], `tracklist_guidelines` | Help Center update 2026-04-10 | Disc positions, heading/index groups and duration handling. |
| [Data dump landing page][D], `data_dump` | No version date stated on landing page | Monthly Release, Artist, Label and Master XML data; explicit CC0 license. No bulk dump was downloaded or indexed. |
| [CC0 1.0 legal code][Z], `cc0_legalcode` | CC0 version 1.0 | §2 permanent, irrevocable waiver; §3 fallback license; §4 no trademark grant or clearance of others' rights. |
| [Official client README][C] and [authentication guide][CA], `client_readme`, `client_auth` | Commit `c159195a43ceb3def5a9be2acce43bbad71a2e32` | First-party token and OAuth examples. The README explicitly says deprecated and unmaintained. It describes search as requiring authentication. This is historical guidance fetched live, not current production validation. |
| [Developer portal][P], `developers` | HTTP 403 during this run | The current schema, precise published quota window and authenticated allowance could not be independently inspected. No challenge bypass was attempted. |

An article's Help Center update time is not necessarily its terms' effective date. The capture preserves both where available. This investigation found no written Discogs exception for Parabolica and obtained no provider reply.

## Actual public access

These are real requests from Linux using an identifying `ParabolicaDiscogsAccessResearch/0.1` User-Agent and the Discogs v2 JSON Accept header. No Authorization header, cookie jar, user session or private credentials were used. API bodies were processed in memory; retained JSON omits user/community data, marketplace fields, unknown fields and image URLs. Image bytes were not downloaded. [E]

| Observation | Request and result | Consequence |
| --- | --- | --- |
| `search` | `GET /database/search?artist=Rick%20Astley&release_title=Never%20Gonna%20Give%20You%20Up&type=release&per_page=2`, 200; 71 matches; empty thumbnail fields in both returned candidates | Search is technically available anonymously in this session. It does not establish an account-free production guarantee. |
| `barcode` | `GET /database/search?barcode=5012394144777&type=release&per_page=1`, 200; 8 matches | A barcode narrows editions, not a unique match. |
| `release` | `GET /releases/249504`, 200; release title, artists, edition fields, A/B tracks and four image descriptors | Fetch the release after selection; do not write the combined search display title as `ALBUM`. |
| `master` | `GET /masters/96559`, 200; `main_release=221824`; A/B1/B2 tracklist | The fetched master has three track rows while release 249504 has two. Master identity is not release identity. |
| `versions`, `versions_next` | `GET /masters/96559/versions?per_page=2`, then its returned next URL; both 200; pages 1 and 2 of 34, 67 editions | Paginate on demand. Validate the origin/path before following provider URLs. |
| `two_disc` | `GET /releases/1890689`, 200; two CDs, 15 tracks from `1-1` through `2-9`; 300×299 primary and 600×600 secondary art | One selection covers both discs; primary does not mean highest resolution. |
| `index` | `GET /releases/1156598`, 200; an `index` row has seven nested `sub_tracks`, some with empty durations | Preserve grouping and raw order. The older note's claim of a flat response is superseded. |
| `heading` | `GET /releases/6061`, 200; headings have empty positions | Heading rows are not physical audio files. |
| `missing_release` | `GET /releases/0`, 404; `{"message":"That release does not exist or may have been deleted."}` | A missing release must return to selection without changing local files. |
| `image_head` | HEAD to the first image URL returned for release 249504, 200; `image/jpeg`, content length 68,934, `Cache-Control: public, max-age=31536000` | One URL was reachable. This proves neither byte decoding, a storage license, an authenticated image entitlement nor a universal CDN rule. |

All ten API responses advertised `X-Discogs-Ratelimit: 25` with used/remaining headers. Counters did not advance uniformly, so they cannot prove an exact accounting algorithm. The number is an observed allowance, not evidence by itself of the time window. Authenticated quota, any separate image allowance, 401/403 recovery, token revocation and actual 429 behavior remain untested. No load test or deliberate quota exhaustion was performed. A synthetic 429 test only verifies that this capture tool stops making requests.

## Access and credential contract

Use only public search/release/master/version GETs for the initial app. Send an identifying User-Agent, validate the API origin and expected route for pagination, and do not add account setup or credential storage to this path. The historical first-party client calls search authenticated, while the recorded public calls returned 200. Preserve that distinction and handle 401/403 as unavailable lookup without changing files or discarding the user's proposal. [C][E]

A personal access token is a possible future option if public access becomes insufficient. Discogs' account settings and historical guide describe token creation. The intended header would be `Authorization: Discogs token=<personal token>` to exactly `api.discogs.com`, stored through the OS credential boundary from #14. That header, authenticated quota and token revocation were not validated here; they must be checked if authentication is introduced. Never put a future token in URLs, exports, logs or a public binary, or forward it to redirects/CDNs. This is deferred guidance, not a current implementation dependency. [A][CA]

OAuth 1.0a is another future option for per-user browser authorization, with application registration, user approval and token/secret exchange. It was not tested or selected for the current scope. [C][CA]

Make requests serially and conservatively, debit locally rather than relying solely on potentially cached counters, and obey returned limit/remaining headers. The probe uses at least three seconds between API calls. Do not configure the commonly quoted 60/minute authenticated allowance as a verified entitlement. On 429, honor valid `Retry-After`, otherwise use bounded backoff; permit cancellation and avoid spinning. Do not evade limits with extra credentials. The downstream importer must handle offline state, 401, 403, 404, 429, malformed responses and transient server failures without committing a tag patch. Those application tests belong to its implementation, not a required account run for this research issue. [T]

## Source previews, durable output and attribution

The terms say Content cannot be displayed when more than six hours older than information on Discogs. They separately forbid caching or storing it longer than necessary to provide the service. They do not prescribe a universal six-hour storage entitlement, and the wording does not explicitly exempt CC0 fields. [T, API use and restrictions §5][T]

For the source-review portion, use an in-memory proposal and fetch timestamp, discard unused responses when the import ends or is cancelled, and refetch before displaying a proposal older than six hours. An expired proposal cannot be applied without a fresh diff and renewed user review. Do not create an offline Discogs catalog or retain full API JSON in durable job records under this preview policy. A CDN's one-year cache header does not override these conditions.

Permanent tags are a different output. CC0 permits durable reuse of the covered metadata. The owner has chosen to treat accepted tag values as local file metadata while the source preview remains subject to API access conditions. Save the selected CC0 values and release/source provenance; local playback can use them offline and the user can edit them later. Do not periodically overwrite, expire or delete those tags. Reimport is user-triggered and fetches a fresh proposal for review. This records the chosen implementation interpretation without claiming a provider-issued export exception. [T][Z]

The importer attribution design is concrete:

- Every search/version candidate has **Data provided by Discogs.** linked to that candidate's release page. An adjacent shared source label may cover a clearly bounded master overview, linked to its master page.
- In release review, put the same linked notice beside the Discogs column heading that covers the source field table and tracklist. Keep it visible during mapping and the final apply review. Link to the selected release page, not the API endpoint or Discogs homepage.
- If a Discogs artwork preview is retained, put the linked notice beside it too. Do not burn notices into the image or append them to music titles. CAA and local images must keep their own source identity.
- Put the exact application notice prominently in the Discogs import view or provider information panel and in usage documentation: "This application uses Discogs’ API but is not affiliated with, sponsored or endorsed by Discogs. ‘Discogs’ is a trademark of Zink Media, LLC." A connection/sign-in screen is unnecessary for the selected public path.
- Use ordinary direct links without `nofollow` or ranking-blocking redirects. `Parabolica` remains the application name; `Update from Discogs` accurately describes an action. [T][N]

These are source labels, not recurring legal warnings. Under the chosen model, local playback reads the user's saved tags and does not need a Discogs warning on every track. Preserve source provenance and show the linked notice in Files & tags wherever the Discogs source/provenance is presented. Any screen directly displaying API lookup data needs adjacent attribution; a hidden About notice alone does not satisfy that requirement. CC0 itself has no attribution condition; these source labels implement the API-use requirement for the lookup/import context. This is the owner-approved placement for the personal app, not a claim that Discogs has issued a separate written interpretation. [T][Z]

## Implementation handoff

Keep source identity, chosen release ID, optional master ID, source URL, fetch time and per-file mapping distinct from the user's durable tag patch. Retain that provenance for reimport under the chosen model. Never persist the whole mixed response as a shortcut: release JSON also contains Restricted Data such as community/user and marketplace information. The probe's redaction is evidence minimization, not a production licensing classifier for every possible JSON field. [T][E]

The full mapping and write requirements remain in [DISCOGS.md](DISCOGS.md) and [TECH_SPEC.md](../TECH_SPEC.md#album-metadata-import). Preserve unknown tags, ReplayGain and unselected pictures; require resolved mappings for included files; stage and validate before writes; detect external changes; report partial results per physical file. A single-file CUE image has one embedded tag/artwork container. Image selection, Embed, Save beside files and Both are separate choices. Rights clearance does not replace format decoding or file-safety tests.

No Rust app files, manifests, module registrations or migrations were changed. #2 and #14 can consume this decision without assuming a finalized shared API type. The decision does not claim app integration, FLAC writes, Windows credential storage or image decoding have been tested.

## Remaining input

None for the revised investigation scope. The owner has chosen permanent CC0 catalog tags, accepted the demonstrated public access without an authenticated run for now, and withdrawn Discogs embedding. The decisions above replace the earlier requests for provider clarification and account validation. The owner subsequently authorized committing and pushing this evidence directly to master and closing issue #19. No provider correspondence has been authorized or conducted.

Known evidence limits remain visible: the developer portal returned 403; the exact published quota window was not inspected; authenticated behavior is untested; public API availability is not guaranteed; and CAA access does not establish an image copyright license. These limits do not create another required account/provider task for the chosen personal scope. If future access or product requirements change, revisit the affected decision then.

Reintroducing permanent Discogs sidecar downloads would require a separate source/destination decision. It is not part of the selected CAA/local-image path and is not an outstanding research blocker. Implementing the importer, cropping and safe file writes, and synchronizing shared specifications with the recorded owner decisions, are downstream handoff work. This research completion does not certify those features as implemented.

## Acceptance and checks

| Issue criterion | Evidence and status |
| --- | --- |
| Cover durable metadata, embedded images and sidecars separately with current primary sources | Met. Permanent CC0 tags are selected; Discogs embedding is withdrawn; Discogs sidecars are excluded from the selected art path. Original evidence is retained. |
| Attribution in import context without repeated generic warnings | Met for the recorded implementation decision. Linked source labels accompany import data and source provenance; local playback uses saved tags without recurring generic warnings. |
| Distinguish public observations from authenticated/production requirements | Met. Public GETs were tested; authenticated behavior is explicitly untested and deferred by owner instruction. Runtime error handling remains a downstream implementation requirement. |
| Resolve provider/owner questions rather than restate uncertainty | Met for the revised scope. The owner-selected CC0/public-access model is concrete; no external input remains required for this investigation. |
| Keep Discogs tagging and art in scope absent an explicit owner change | The owner explicitly withdrew Discogs embedding. Local artwork and optional CAA lookup are documented above; CAA has not been misrepresented as copyright-free. |

The capture command and offline checks are documented in the [probe README][R]. The recorded run contains ten API GETs, one image HEAD and twelve primary-source fetches. Sixteen offline tests verify capture safety and the recorded edition, pagination, grouping, artwork and error observations. They do not simulate a successful account validation or test an application importer. `git diff --check` and scoped Python lint/format checks are recorded there after execution.

[E]: ../../tests/provider-live/discogs_access/evidence/2026-11-06-final/capture.json
[R]: ../../tests/provider-live/discogs_access/README.md
[T]: https://support.discogs.com/hc/en-us/articles/360009334593-API-Terms-of-Use
[S]: https://support.discogs.com/hc/en-us/articles/360009334333-Terms-of-Service
[I]: https://support.discogs.com/hc/en-us/articles/360005006874-Database-Guidelines-13-Images
[N]: https://support.discogs.com/hc/en-us/articles/360009207054-Application-Name-and-Description-Policy
[A]: https://support.discogs.com/hc/en-us/articles/360007423833-How-Do-I-Change-My-Account-Settings
[M]: https://support.discogs.com/hc/en-us/articles/360005055493-Database-Guidelines-16-Master-Release
[L]: https://support.discogs.com/hc/en-us/articles/360005055373-Database-Guidelines-12-Tracklisting
[D]: https://data.discogs.com/
[Z]: https://creativecommons.org/publicdomain/zero/1.0/legalcode.en
[C]: https://github.com/discogs/discogs_client/blob/c159195a43ceb3def5a9be2acce43bbad71a2e32/README.mkd
[CA]: https://github.com/discogs/discogs_client/blob/c159195a43ceb3def5a9be2acce43bbad71a2e32/docs/authentication.md
[P]: https://www.discogs.com/developers/
[CAA]: https://coverartarchive.org/
[CAAINFO]: https://musicbrainz.org/doc/Cover_Art_Archive
[CAAAPI]: https://musicbrainz.org/doc/Cover_Art_Archive/API
[MBAPI]: https://musicbrainz.org/doc/MusicBrainz_API
[CAAE]: ../../tests/provider-live/discogs_access/evidence/artwork-source-review-2026-11-06.json

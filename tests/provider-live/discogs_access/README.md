# Discogs public access evidence

This directory supports issue #19's [integration decision](../../../docs/research/DISCOGS_INTEGRATION_DECISION.md). It contains a public HTTP probe, redacted observations, primary-source excerpts and offline checks. The owner has selected permanent covered CC0 metadata tags with public access and deferred authentication. These files do not implement an app provider or package artwork assets.

## Recorded run

[capture.json](evidence/2026-11-06-final/capture.json) was captured on 2026-11-06 in Asia/Jakarta, from 2026-11-05 18:39:24 to 18:40:02 UTC, on Linux with CPython 3.11.17 and uv 0.13.5. The source commit was `815207e200c8e7d633458c2682ebfcb239043fc2`; the probe and decision are local changes on that commit.

From the repository root:

```sh
uv run --no-project python tests/provider-live/discogs_access/capture.py --output tests/provider-live/discogs_access/evidence/2026-11-06-final
```

Results:

- Ten API GETs: nine returned 200, and `/releases/0` returned 404. This includes search, barcode search, release, master, two version pages, a two-disc album, nested index rows and headings.
- One image HEAD returned 200 and `image/jpeg`. No image GET or byte decoding occurred.
- Twelve primary-source fetches: eleven returned 200. The official developer portal returned 403. Current API/general terms, image/master/tracklisting guidance, account settings, naming policy, the CC0 dump landing page, CC0 legal code and two pinned files from the deprecated official client were retrieved.
- API quota headers advertised 25. No authenticated allowance or precise quota window was proved. No live 401, API 403, 429, token-expiry or revocation test was performed.

The capture is actual network evidence. The test responses constructed in `CaptureSafetyTests` are synthetic and test the probe's handling only. No Windows, app, tag-write, account, OAuth or audio-device validation is claimed.

## Reproduce public observations

Use the command above with a **new** output directory. The recorded directory already exists, and the tool refuses to overwrite it. Each run uses fixed public examples and may return different current data. Review any status, fields, counters or source wording that changes before updating the decision. The offline tests intentionally check the recorded snapshot, not a fresh provider state.

The script uses Python's standard library, no project dependencies. It reads no tokens, browser sessions, cookie files, `.netrc` or private account settings. There is no authenticated mode. Requests use an identifying User-Agent; API requests also use the Discogs v2 JSON Accept header. Redirects are not followed, pagination must stay on the expected API origin/path, and the image HEAD must use `i.discogs.com`. The probe spaces API requests by at least three seconds and stops on a 429. It does not retry to obtain a preferred outcome or exhaust the quota.

Each observation retains request method/URL/public headers, status, UTC time, selected response headers, original-body byte count and original-body SHA-256. Those hashes identify the original responses; they are not hashes of the formatted/redacted JSON. Raw responses exist only in memory.

JSON redaction retains selected catalog and tracklist fields, pagination and image dimensions/types. It drops whole community/user and marketplace branches, prices, unknown fields and thumbnail URLs. Image URL values become markers, and the image HEAD target retains only a hash plus its source release and image index. `omitted_json_paths` names dropped fields. Response headers use an allowlist that excludes cookies and request identifiers. Non-JSON error/challenge bodies are omitted. No artwork is packaged in these fixtures.

Source excerpts normalize HTML whitespace, omit scripts/styles and select relevant paragraphs. Their original HTTP body hashes refer to the fetched Help Center JSON, HTML or pinned Markdown file. An unmatched excerpt query or failed source retrieval is visible in the capture and needs review. A partial run retains accumulated evidence with `capture_complete: false`; a completed capture is not by itself proof that every status is acceptable.

These bounded files are research evidence, not an application cache, an offline API catalog or a production asset bundle. The chosen distinction between fresh API previews and durable CC0 file tags is recorded in the decision; the fixture tests do not make legal assertions. The owner authorized publishing this evidence to the repository as part of issue #19's completion.

## Offline checks

```sh
uv run --no-project python -B -m unittest discover -s tests/provider-live/discogs_access -p 'test_*.py' -v
uvx ruff check --no-cache tests/provider-live/discogs_access
uvx ruff format --check --no-cache tests/provider-live/discogs_access
git diff --check
```

The tests check that mixed responses lose user/marketplace/unknown fields, image URLs are removed, nested track groups survive, unsafe URLs and redirects are rejected, HTML challenge bodies are omitted, and a simulated 429 stops the probe. Recorded-evidence assertions verify provenance, headers, selected-release versus master identity, barcode ambiguity, returned pagination, two-disc positions, nested movements, missing durations, headings and the observed 404/image HEAD. No test asserts that an HTTP response grants reuse rights.

Executed locally on 2026-11-06: all 16 tests passed; Ruff 0.17.3 lint and format checks passed; `git diff --check` passed. A separate local check passed for document file targets, Python syntax, evidence counts and trailing whitespace in all changed files, including untracked files. The live developer-portal 403 is an external documentation gap, not a passed documentation check.

## Authenticated access deferred

The owner has directed the app to use the observed public requests without an authenticated run for now. The present probe supplies that public evidence. Authentication, authenticated quota and token lifecycle remain untested and are not required to complete the revised investigation scope. If public access later becomes insufficient and authentication is introduced, validate it separately using secure local credential input and redacted results; do not infer that this public run tested it.

The owner has also confirmed permanent tagging from covered CC0 metadata and withdrawn Discogs artwork embedding. The [decision](../../../docs/research/DISCOGS_INTEGRATION_DECISION.md#remaining-input) records no remaining provider/account input for this scoped investigation. Repository publication and issue closure are authorized; no provider message has been authorized or sent. The independent artwork editor and safe tag-writing flow remain implementation work.

## Artwork source follow-up

[artwork-source-review-2026-11-06.json](evidence/artwork-source-review-2026-11-06.json) records a separate public HTTP review after that owner decision. Four first-party pages returned 200. An unauthenticated Cover Art Archive listing request returned 307, followed by 302 on `archive.org` and 200 JSON on an Archive download host, with six images. The record retains the exact listing URLs, response hashes, selected headers, image IDs/types and copyright/API excerpts. It omits user comments and image URLs/bytes.

The listing request was `GET https://coverartarchive.org/release/76df3287-6cda-33eb-8e9a-044b5e15ffdd/` with `Accept: application/json` and User-Agent `ParabolicaArtworkResearch/0.1 (+https://github.com/anasalqoyyum/parabolica)`. Only HTTPS redirects to `archive.org` and its subdomains were followed, with a four-request bound. The source pages and request chain in the evidence can be fetched again with these public headers; this was an ad hoc source review, not an extension of the Discogs probe or its fixture assertions.

CAA's copyright statement applies independently of public access and metadata approval flags. The [updated decision](../../../docs/research/DISCOGS_INTEGRATION_DECISION.md#artwork-direction-after-the-owner-decision) distinguishes that candidate source from the local file picker and specifies optional cropping, no square/size quality gate and full portrait display. No live image download, crop, embed or application flow was tested. The existing 16 tests still describe only the original Discogs probe and recorded snapshot.

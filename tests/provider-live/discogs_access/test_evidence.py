import json
import unittest
from pathlib import Path
from unittest.mock import patch

import capture

EVIDENCE = Path(__file__).parent / "evidence/2026-11-06-final/capture.json"


class CaptureSafetyTests(unittest.TestCase):
    def test_mixed_response_drops_user_marketplace_and_unknown_fields(self):
        removed = []
        result = capture.redact(
            {
                "id": 123,
                "community": {"submitter": {"username": "private-example"}},
                "lowest_price": 12,
                "future_private_field": "not-for-evidence",
                "artists": [{"id": 7, "name": "Example", "private_email": "secret"}],
            },
            removed,
        )
        self.assertEqual(result, {"id": 123, "artists": [{"id": 7, "name": "Example"}]})
        self.assertEqual(
            set(removed),
            {
                "/community",
                "/lowest_price",
                "/future_private_field",
                "/artists/0/private_email",
            },
        )

    def test_nested_track_groups_survive_redaction(self):
        tracks = {
            "tracklist": [
                {
                    "type_": "index",
                    "sub_tracks": [
                        {"type_": "track", "title": "Movement", "duration": ""},
                    ],
                }
            ]
        }
        self.assertEqual(capture.redact(tracks, []), tracks)

    def test_images_keep_dimensions_but_no_download_urls(self):
        result = capture.redact(
            {
                "images": [
                    {
                        "type": "primary",
                        "width": 300,
                        "height": 299,
                        "uri": "https://i.discogs.com/example",
                        "resource_url": "https://i.discogs.com/example",
                        "uri150": "https://i.discogs.com/thumb",
                    }
                ],
                "thumb": "",
                "cover_image": "https://i.discogs.com/cover",
            },
            [],
        )
        self.assertEqual(result["images"][0]["width"], 300)
        self.assertNotIn("https://", json.dumps(result))
        self.assertEqual(result["thumb"], "")

    def test_url_policy_rejects_credentials_and_unexpected_origins(self):
        for url in [
            "http://api.discogs.com/releases/1",
            "https://api.discogs.com.attacker.invalid/releases/1",
            "https://api.discogs.com:444/releases/1",
            "https://user:secret@api.discogs.com/releases/1",
            "https://api.discogs.com/releases/1?TOKEN=secret",
            "https://api.discogs.com/releases/1?oauth_token=secret",
        ]:
            with self.subTest(url=url), self.assertRaises(ValueError):
                capture.public_url(url)

    def test_redirect_is_not_followed(self):
        self.assertIsNone(
            capture.NoRedirect().redirect_request(
                None, None, 302, "Redirect", {}, "https://example.invalid"
            )
        )

    def test_challenge_body_is_omitted(self):
        evidence = {"headers": {"content-type": "text/html"}}
        self.assertIsNone(
            capture.json_response(evidence, b"<html>cookie challenge</html>")
        )
        self.assertNotIn("response", evidence)
        self.assertNotIn("cookie challenge", json.dumps(evidence))

    def test_source_excerpts_join_wrapped_lines_and_omit_scripts(self):
        parser = capture.ArticleText()
        parser.feed(
            "<p>permanently,\n irrevocably <a>and</a> unconditionally</p>"
            "<script>secret</script><p>Next paragraph</p>"
        )
        self.assertEqual(
            parser.lines(),
            [
                "permanently, irrevocably and unconditionally",
                "Next paragraph",
            ],
        )

    def test_429_stops_public_probe_without_retrying(self):
        observations = []
        response = {"status": 429, "headers": {"content-type": "application/json"}}
        with (
            patch.object(
                capture, "request", return_value=(response, b'{"message":"limited"}')
            ) as req,
            patch.object(capture.time, "sleep") as sleep,
        ):
            self.assertFalse(capture.capture_public(observations))
        self.assertEqual(req.call_count, 1)
        sleep.assert_not_called()
        self.assertEqual(observations[0]["status"], 429)


class RecordedPublicEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        cls.observations = {o["name"]: o for o in cls.evidence["observations"]}
        cls.sources = {s["name"]: s for s in cls.evidence["sources"]}

    def response(self, name):
        return self.observations[name]["response"]

    def test_provenance_and_redaction(self):
        self.assertTrue(self.evidence["capture_complete"])
        self.assertEqual(
            self.evidence["source_commit"], "815207e200c8e7d633458c2682ebfcb239043fc2"
        )
        for record in self.evidence["observations"] + self.evidence["sources"]:
            with self.subTest(name=record["name"]):
                self.assertEqual(len(record["body_sha256"]), 64)
                self.assertTrue(set(record["headers"]).issubset(capture.HEADERS))
                self.assertEqual(
                    set(record["request"]["headers"]) - {"Accept"}, {"User-Agent"}
                )
        for name in ["release", "two_disc", "index", "heading"]:
            record = self.observations[name]
            self.assertIn("/community", record["omitted_json_paths"])
            self.assertNotIn("community", record["response"])
            self.assertNotIn("https://i.discogs.com/", json.dumps(record["response"]))

    def test_lookup_statuses_and_quota_are_observed_not_assumed(self):
        expected = {name: 200 for name, _ in capture.PUBLIC_REQUESTS}
        expected.update(image_head=200, versions_next=200, missing_release=404)
        self.assertEqual(
            {name: o["status"] for name, o in self.observations.items()}, expected
        )
        for name in expected.keys() - {"image_head"}:
            self.assertEqual(
                self.observations[name]["headers"]["x-discogs-ratelimit"], "25"
            )
        self.assertEqual(
            self.response("missing_release"),
            {
                "message": "That release does not exist or may have been deleted.",
            },
        )

    def test_barcode_and_search_require_release_selection(self):
        self.assertGreater(self.response("search")["pagination"]["items"], 1)
        self.assertGreater(self.response("barcode")["pagination"]["items"], 1)
        self.assertTrue(
            all(r["type"] == "release" for r in self.response("search")["results"])
        )
        self.assertTrue(
            all(r["thumb"] == "" for r in self.response("search")["results"])
        )

    def test_versions_followed_the_returned_next_page(self):
        first = self.response("versions")["pagination"]
        second = self.response("versions_next")["pagination"]
        self.assertEqual(
            first["urls"]["next"], self.observations["versions_next"]["request"]["url"]
        )
        self.assertEqual((first["page"], second["page"]), (1, 2))
        self.assertGreater(first["pages"], 2)

    def test_master_tracklist_is_not_the_selected_edition(self):
        release = self.response("release")
        master = self.response("master")
        self.assertEqual(release["master_id"], master["id"])
        self.assertNotEqual(release["id"], master["main_release"])
        self.assertNotEqual(release["tracklist"], master["tracklist"])
        self.assertEqual(release["released"], "1987-07-00")

    def test_two_discs_index_groups_and_headings(self):
        tracks = self.response("two_disc")["tracklist"]
        self.assertEqual(
            (tracks[0]["position"], tracks[-1]["position"]), ("1-1", "2-9")
        )
        index = next(
            t for t in self.response("index")["tracklist"] if t["type_"] == "index"
        )
        self.assertTrue(index["sub_tracks"])
        self.assertTrue(any(t["duration"] == "" for t in index["sub_tracks"]))
        heading = next(
            t for t in self.response("heading")["tracklist"] if t["type_"] == "heading"
        )
        self.assertEqual(heading["position"], "")

    def test_image_head_does_not_claim_a_download_or_license(self):
        head = self.observations["image_head"]
        self.assertEqual(head["request"]["method"], "HEAD")
        self.assertEqual(head["body_bytes"], 0)
        self.assertEqual(head["headers"]["content-type"], "image/jpeg")
        image = self.response("two_disc")["images"][0]
        self.assertEqual((image["width"], image["height"]), (300, 299))
        self.assertTrue(
            any(i["type"] == "secondary" for i in self.response("two_disc")["images"])
        )

    def test_primary_sources_preserve_terms_and_known_documentation_gap(self):
        for name, source in self.sources.items():
            with self.subTest(name=name):
                self.assertEqual(source["status"], 403 if name == "developers" else 200)
                self.assertFalse(source.get("unmatched_excerpt_queries"))
        terms = "\n".join(self.sources["api_terms"]["excerpts"])
        self.assertIn("May 27th, 2025", terms)
        self.assertIn("six (6) hours", terms)
        self.assertIn("Data provided by Discogs.", terms)
        self.assertIn(
            "Content made available via our Data Dump",
            "\n".join(self.sources["terms_of_service"]["excerpts"]),
        )


if __name__ == "__main__":
    unittest.main()

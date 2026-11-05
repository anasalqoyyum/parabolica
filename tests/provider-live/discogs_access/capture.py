"""Capture public Discogs access evidence without account credentials or image files."""

import argparse
import hashlib
import json
import platform
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import UTC, datetime
from html.parser import HTMLParser
from pathlib import Path

USER_AGENT = (
    "ParabolicaDiscogsAccessResearch/0.1 (+https://github.com/anasalqoyyum/parabolica)"
)
CLIENT_COMMIT = "c159195a43ceb3def5a9be2acce43bbad71a2e32"
HEADERS = {
    "date",
    "content-type",
    "content-length",
    "cache-control",
    "age",
    "etag",
    "retry-after",
    "x-discogs-ratelimit",
    "x-discogs-ratelimit-used",
    "x-discogs-ratelimit-remaining",
}
FIELDS = {
    "id",
    "title",
    "status",
    "year",
    "country",
    "released",
    "released_formatted",
    "uri",
    "resource_url",
    "master_id",
    "master_url",
    "main_release",
    "most_recent_release",
    "versions_url",
    "artists",
    "artists_sort",
    "labels",
    "formats",
    "format",
    "format_quantity",
    "genres",
    "genre",
    "styles",
    "style",
    "tracklist",
    "extraartists",
    "identifiers",
    "notes",
    "data_quality",
    "name",
    "anv",
    "join",
    "role",
    "tracks",
    "catno",
    "entity_type",
    "entity_type_name",
    "qty",
    "descriptions",
    "text",
    "type",
    "type_",
    "value",
    "description",
    "position",
    "duration",
    "sub_tracks",
    "pagination",
    "page",
    "pages",
    "per_page",
    "items",
    "urls",
    "next",
    "last",
    "prev",
    "first",
    "results",
    "versions",
    "label",
    "barcode",
    "major_formats",
    "message",
    "images",
    "width",
    "height",
    "thumb",
    "cover_image",
    "uri150",
}
IMAGE_KEYS = {"thumb", "cover_image", "uri150"}
PUBLIC_REQUESTS = [
    (
        "search",
        "/database/search?artist=Rick%20Astley&release_title=Never%20Gonna%20Give%20You%20Up&type=release&per_page=2",
    ),
    ("barcode", "/database/search?barcode=5012394144777&type=release&per_page=1"),
    ("release", "/releases/249504"),
    ("master", "/masters/96559"),
    ("versions", "/masters/96559/versions?per_page=2"),
    ("two_disc", "/releases/1890689"),
    ("index", "/releases/1156598"),
    ("heading", "/releases/6061"),
    ("missing_release", "/releases/0"),
]
ARTICLES = [
    (
        "api_terms",
        "360009334593",
        [
            "Last Updated:",
            "These Terms of Use",
            "CC0 Data:",
            "Restricted Data:",
            "Release titles, notes",
            "Artist names, notes",
            "Label, producer",
            "“Images”",
            "Image is Public Domain",
            "The user owns the rights",
            "Image is already made available",
            "Fair Use",
            "limited, personal",
            "Transfer Restricted Data",
            "Use Restricted Data for any commercial",
            "may also be subject",
            "six (6) hours",
            "We require You to display",
            "In addition, You must display",
            "Circumvent any security",
            "circumvent Our rate limits",
            "offered with no warranty of non-infringement",
            "Commercial use of Our API",
            "Selling or giving to any third party",
        ],
    ),
    (
        "terms_of_service",
        "360009334333",
        [
            "Last updated:",
            "We grant You a limited license",
            "We strictly prohibit",
            "Content made available via our Data Dump",
            "Some User Content may be",
        ],
    ),
    (
        "image_guidelines",
        "360005006874",
        [
            "By uploading images",
            "Image is Public Domain",
            "You own the rights",
            "Image is already made available",
            "Fair Use",
            "They must be of the exact",
            "Do not download an image from Discogs",
            "The primary image, i.e.",
        ],
    ),
    ("account_settings", "360007423833", ["Developers", "Applications"]),
    (
        "application_name",
        "360009207054",
        [
            "Effective Date:",
            "You may accurately indicate",
            "implies partnership",
        ],
    ),
    (
        "master_guidelines",
        "360005055493",
        [
            "Master Release",
            "tracklist",
            "radically",
        ],
    ),
    (
        "tracklist_guidelines",
        "360005055373",
        [
            "Heading",
            "Index",
            "1-1",
            "duration",
        ],
    ),
]


def now():
    return datetime.now(UTC).isoformat(timespec="seconds")


def digest(body):
    return hashlib.sha256(body).hexdigest()


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class ArticleText(HTMLParser):
    def __init__(self):
        super().__init__()
        self.parts = []
        self.hidden = 0

    def handle_starttag(self, tag, attrs):
        if tag in {"script", "style"}:
            self.hidden += 1
        if tag in {"p", "li", "h1", "h2", "h3", "br", "pre"}:
            self.parts.append("\n")

    def handle_endtag(self, tag):
        if tag in {"script", "style"}:
            self.hidden -= 1
        if tag in {"p", "li", "h1", "h2", "h3", "pre"}:
            self.parts.append("\n")

    def handle_data(self, data):
        if not self.hidden:
            self.parts.append(data.replace("\n", " "))

    def lines(self):
        return [
            " ".join(line.split())
            for line in "".join(self.parts).splitlines()
            if line.strip()
        ]


def public_url(url):
    parsed = urllib.parse.urlsplit(url)
    if (
        parsed.scheme != "https"
        or parsed.hostname
        not in {
            "api.discogs.com",
            "i.discogs.com",
            "www.discogs.com",
            "support.discogs.com",
            "data.discogs.com",
            "raw.githubusercontent.com",
            "creativecommons.org",
        }
        or parsed.username
        or parsed.password
        or parsed.port not in {None, 443}
    ):
        raise ValueError("Unexpected public source origin")
    query = urllib.parse.parse_qs(parsed.query)
    if any(key.lower() in {"token", "key", "secret", "oauth_token"} for key in query):
        raise ValueError("Credential query parameters are forbidden")
    return parsed


def request(url, method="GET"):
    parsed = public_url(url)
    headers = {"User-Agent": USER_AGENT}
    if parsed.hostname == "api.discogs.com":
        headers["Accept"] = "application/vnd.discogs.v2.discogs+json"
    req = urllib.request.Request(url, headers=headers, method=method)
    opener = urllib.request.build_opener(NoRedirect())
    try:
        response = opener.open(req, timeout=30)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        body = response.read(2_000_001)
        if len(body) > 2_000_000:
            raise ValueError("Response exceeds research capture size limit")
        evidence = {
            "observed_at_utc": now(),
            "request": {"method": method, "url": url, "headers": headers},
            "status": response.code,
            "headers": {
                k.lower(): v
                for k, v in response.headers.items()
                if k.lower() in HEADERS
            },
            "body_bytes": len(body),
            "body_sha256": digest(body),
        }
    return evidence, body


def redact(value, removed, path=""):
    if isinstance(value, list):
        return [redact(item, removed, f"{path}/{i}") for i, item in enumerate(value)]
    if isinstance(value, dict):
        result = {}
        for key, item in value.items():
            child = f"{path}/{key}"
            if key not in FIELDS:
                removed.append(child)
            elif key in IMAGE_KEYS or (
                key in {"uri", "resource_url"} and "/images/" in child
            ):
                result[key] = "[REDACTED image URL]" if item else item
            else:
                result[key] = redact(item, removed, child)
        return result
    if isinstance(value, str) and value.startswith("https://i.discogs.com/"):
        return "[REDACTED image URL]"
    return value


def json_response(evidence, body):
    if "json" not in evidence["headers"].get("content-type", ""):
        evidence["body_omitted"] = (
            "Non-JSON response. No HTML, cookies or challenge data retained."
        )
        return None
    data = json.loads(body)
    removed = []
    evidence["response"] = redact(data, removed)
    evidence["omitted_json_paths"] = removed
    return data


def capture_sources(sources):
    for name, ident, needles in ARTICLES:
        evidence, body = request(
            f"https://support.discogs.com/api/v2/help_center/en-us/articles/{ident}.json"
        )
        evidence["name"] = name
        if evidence["status"] == 200:
            article = json.loads(body)["article"]
            evidence.update(
                {
                    k: article[k]
                    for k in ["title", "html_url", "updated_at", "edited_at"]
                }
            )
            parser = ArticleText()
            parser.feed(article["body"])
            lines = parser.lines()
            evidence["excerpts"] = [
                line
                for line in lines
                if any(needle.casefold() in line.casefold() for needle in needles)
            ]
            evidence["unmatched_excerpt_queries"] = [
                needle
                for needle in needles
                if not any(needle.casefold() in line.casefold() for line in lines)
            ]
        sources.append(evidence)
    for name, url, needles, html in [
        ("developers", "https://www.discogs.com/developers/", [], True),
        ("data_dump", "https://data.discogs.com/", ["monthly dumps", "CC0"], True),
        (
            "client_readme",
            f"https://raw.githubusercontent.com/discogs/discogs_client/{CLIENT_COMMIT}/README.mkd",
            [
                "DEPRECATED",
                "no longer maintained",
                "official Discogs",
                "such as search",
                "user-token",
                "OAuth 1.0a",
            ],
            False,
        ),
        (
            "client_auth",
            f"https://raw.githubusercontent.com/discogs/discogs_client/{CLIENT_COMMIT}/docs/authentication.md",
            [
                "single user",
                "Discogs User Token",
                "consumer key",
                "verifier",
                "authorization URL",
            ],
            False,
        ),
        (
            "cc0_legalcode",
            "https://creativecommons.org/publicdomain/zero/1.0/legalcode.en",
            [
                "permanently, irrevocably",
                "non transferable",
                "No trademark",
                "other rights",
            ],
            True,
        ),
    ]:
        evidence, body = request(url)
        evidence["name"] = name
        if evidence["status"] == 200:
            if html:
                parser = ArticleText()
                parser.feed(body.decode("utf-8"))
                lines = parser.lines()
            else:
                lines = body.decode("utf-8").splitlines()
            evidence["excerpts"] = [
                line
                for line in lines
                if any(needle.casefold() in line.casefold() for needle in needles)
            ]
            evidence["unmatched_excerpt_queries"] = [
                needle
                for needle in needles
                if not any(needle.casefold() in line.casefold() for line in lines)
            ]
        sources.append(evidence)


def capture_public(observations):
    for name, path in PUBLIC_REQUESTS:
        evidence, body = request("https://api.discogs.com" + path)
        evidence["name"] = name
        data = json_response(evidence, body)
        observations.append(evidence)
        print(f"{name}: HTTP {evidence['status']}", flush=True)
        if evidence["status"] == 429:
            return False
        if name == "release" and evidence["status"] == 200 and data.get("images"):
            image_url = data["images"][0]["uri"]
            if public_url(image_url).hostname != "i.discogs.com":
                raise ValueError("Unexpected release image host")
            head, _ = request(image_url, "HEAD")
            head["name"] = "image_head"
            head["image_source"] = {"release_id": 249504, "image_index": 0}
            head["request"]["url_sha256"] = digest(image_url.encode())
            head["request"]["url"] = "https://i.discogs.com/[REDACTED image path]"
            observations.append(head)
            print(f"image_head: HTTP {head['status']}", flush=True)
            if head["status"] == 429:
                return False
        if name == "versions" and evidence["status"] == 200:
            next_url = data.get("pagination", {}).get("urls", {}).get("next")
            if next_url:
                parsed = public_url(next_url)
                if (
                    parsed.hostname != "api.discogs.com"
                    or parsed.path != "/masters/96559/versions"
                ):
                    raise ValueError("Unexpected pagination target")
                time.sleep(3)
                next_evidence, next_body = request(next_url)
                next_evidence["name"] = "versions_next"
                json_response(next_evidence, next_body)
                observations.append(next_evidence)
                print(f"versions_next: HTTP {next_evidence['status']}", flush=True)
                if next_evidence["status"] == 429:
                    return False
        time.sleep(3)
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="A new directory; existing evidence is never overwritten",
    )
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    result = {
        "schema_version": 1,
        "started_at_utc": now(),
        "source_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], text=True
        ).strip(),
        "environment": {"os": platform.system(), "python": platform.python_version()},
        "access": "Public HTTP only. No credentials, cookies, account sessions or image GETs.",
        "observations": [],
        "sources": [],
        "capture_complete": False,
    }
    try:
        capture_sources(result["sources"])
        result["capture_complete"] = capture_public(result["observations"])
    finally:
        result["finished_at_utc"] = now()
        (args.output / "capture.json").write_text(
            json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
    print(args.output / "capture.json")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Adds files to the GitHub release of a tag, creating the release first.

    python3 scripts/ci/github_release.py v0.1.0 dist/*

Every CI service that builds part of a release (Linux and Android, Windows,
the Mac DMG) runs this for its own files, so whichever comes first creates
the release. A file already attached under the same name is replaced, and
each file gets a <name>.sha256 next to it.

Environment:
    GITHUB_RELEASE_TOKEN  token with Contents: read and write on the repository
    GITHUB_REPOSITORY     owner/name (default routeforge-dev/paperloom)

The notes come from docs/release-notes/<tag>.md when it exists. Versions
starting with 0. or carrying a suffix (1.0.0-beta.1) are pre-releases.
"""

import hashlib
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
API = "https://api.github.com"


def request(method, url, token, body=None, content_type="application/json"):
    data = body if isinstance(body, (bytes, type(None))) else json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Authorization", f"Bearer {token}")
    req.add_header("Accept", "application/vnd.github+json")
    req.add_header("X-GitHub-Api-Version", "2022-11-28")
    if data is not None:
        req.add_header("Content-Type", content_type)
    for attempt in range(3):
        try:
            with urllib.request.urlopen(req, timeout=600) as resp:
                raw = resp.read()
                return json.loads(raw) if raw else None
        except urllib.error.HTTPError as err:
            if err.code < 500 or attempt == 2:
                raise
        except urllib.error.URLError:
            if attempt == 2:
                raise
        time.sleep(10 * (attempt + 1))
    return None


def release_for(tag, repo, token):
    url = f"{API}/repos/{repo}/releases/tags/{urllib.parse.quote(tag)}"
    try:
        return request("GET", url, token)
    except urllib.error.HTTPError as err:
        if err.code != 404:
            raise
    version = tag.removeprefix("v")
    notes = ROOT / "docs" / "release-notes" / f"{tag}.md"
    body = notes.read_text(encoding="utf-8") if notes.exists() else f"Paperloom {tag}"
    try:
        return request("POST", f"{API}/repos/{repo}/releases", token, {
            "tag_name": tag,
            "name": f"Paperloom {tag}",
            "body": body,
            "prerelease": version.startswith("0.") or "-" in version,
        })
    except urllib.error.HTTPError as err:
        # Another CI service created it a moment ago.
        if err.code != 422:
            raise
        return request("GET", url, token)


def upload(release, path, data, repo, token):
    for asset in request("GET", f"{API}/repos/{repo}/releases/{release['id']}/assets?per_page=100", token):
        if asset["name"] == path.name:
            request("DELETE", f"{API}/repos/{repo}/releases/assets/{asset['id']}", token)
    base = release["upload_url"].split("{")[0]
    url = f"{base}?name={urllib.parse.quote(path.name)}"
    request("POST", url, token, data, "application/octet-stream")
    print(f"uploaded {path.name} ({len(data) / 1e6:.1f} MB)")


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    tag, files = sys.argv[1], [Path(f) for f in sys.argv[2:]]
    token = os.environ.get("GITHUB_RELEASE_TOKEN", "")
    repo = os.environ.get("GITHUB_REPOSITORY") or "routeforge-dev/paperloom"
    if not token:
        sys.exit("GITHUB_RELEASE_TOKEN is not set; cannot publish the release")
    release = release_for(tag, repo, token)
    for path in files:
        if not path.is_file():
            continue
        data = path.read_bytes()
        upload(release, path, data, repo, token)
        digest = f"{hashlib.sha256(data).hexdigest()}  {path.name}\n".encode()
        upload(release, Path(f"{path.name}.sha256"), digest, repo, token)
    print(release["html_url"])


if __name__ == "__main__":
    main()

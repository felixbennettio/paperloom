#!/usr/bin/env python3
"""Revokes the development certificates CI signing left behind.

Xcode signs archives with an Apple Development certificate. When the Mac
that builds has no such certificate in its keychain (a fresh GitHub-hosted
runner, every time), `xcodebuild -allowProvisioningUpdates` creates one
through the App Store Connect API ("Created via API" in the developer
portal), and its private key is lost with the machine. A self-hosted Mac
keeps the key and reuses its certificate.

`prune` revokes the development certificates created through the API whose
private key is not in this Mac's keychain, so they cannot be used again
anyway. Certificates made in Xcode or the portal, and all distribution
certificates, are left alone. App Store builds are not affected: they are
re-signed with the cloud-managed distribution certificate on export.

    python3 scripts/appstore_certs.py prune [--dry-run] [--ephemeral]

--ephemeral (a GitHub-hosted runner, at the end of the job) also revokes
the certificate this run created: its key goes away with the machine.

Needs APP_STORE_CONNECT_ISSUER_ID, APP_STORE_CONNECT_KEY_ID and
APP_STORE_CONNECT_PRIVATE_KEY (the .p8 file's text, or that text
base64-encoded), and PyJWT with the "crypto" extra.
"""

import base64
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

import jwt
from cryptography import x509

API = "https://api.appstoreconnect.apple.com/v1"
DEV_TYPES = ("DEVELOPMENT", "IOS_DEVELOPMENT", "MAC_APP_DEVELOPMENT")


def private_key():
    key = os.environ["APP_STORE_CONNECT_PRIVATE_KEY"].strip()
    return key if key.startswith("-----BEGIN") else base64.b64decode(key).decode()


def token():
    now = int(time.time())
    return jwt.encode(
        {"iss": os.environ["APP_STORE_CONNECT_ISSUER_ID"].strip(), "iat": now, "exp": now + 15 * 60,
         "aud": "appstoreconnect-v1"},
        private_key(),
        algorithm="ES256",
        headers={"kid": os.environ["APP_STORE_CONNECT_KEY_ID"].strip(), "typ": "JWT"},
    )


def call(bearer, method, path, **params):
    url = API + path + ("?" + urllib.parse.urlencode(params) if params else "")
    req = urllib.request.Request(url, method=method, headers={"Authorization": f"Bearer {bearer}"})
    try:
        with urllib.request.urlopen(req, timeout=30) as res:
            body = res.read()
            return res.status, json.loads(body) if body else {}
    except urllib.error.HTTPError as e:
        try:
            return e.code, json.load(e)
        except Exception:
            return e.code, {}


def serial(value):
    """Upper-case hex without leading zeros, as App Store Connect shows it."""
    return value.upper().lstrip("0") or "0"


def keychain_serials():
    """Serial numbers of the Apple Development certificates in this Mac's
    keychains that have their private key (`security find-identity`)."""
    if sys.platform != "darwin":
        return set()
    out = subprocess.run(["security", "find-identity", "-v", "-p", "codesigning"],
                         capture_output=True, text=True).stdout
    names = {line.split('"')[1] for line in out.splitlines() if line.count('"') >= 2}
    serials = set()
    for name in names:
        if not name.startswith("Apple Development"):
            continue
        pems = subprocess.run(["security", "find-certificate", "-a", "-c", name, "-p"],
                              capture_output=True, text=True).stdout
        for block in pems.split("-----END CERTIFICATE-----"):
            if "-----BEGIN CERTIFICATE-----" in block:
                pem = (block.strip() + "\n-----END CERTIFICATE-----\n").encode()
                serials.add(serial(format(x509.load_pem_x509_certificate(pem).serial_number, "X")))
    return serials


def prune(dry_run, ephemeral):
    bearer = token()
    status, body = call(bearer, "GET", "/certificates", **{
        "filter[certificateType]": ",".join(DEV_TYPES),
        "fields[certificates]": "name,displayName,serialNumber,certificateType,expirationDate",
        "limit": "200",
    })
    if status != 200:
        print(f"::warning::Could not list certificates (HTTP {status}): {body.get('errors')}")
        return
    local = set() if ephemeral else keychain_serials()
    for cert in body.get("data", []):
        a = cert["attributes"]
        label = f"{a.get('certificateType')} {a.get('serialNumber')} (expires {str(a.get('expirationDate'))[:10]})"
        made_by_api = "Created via API" in f"{a.get('displayName', '')} {a.get('name', '')}"
        if not made_by_api:
            continue
        if serial(a.get("serialNumber", "")) in local:
            print(f"keep    {label}: its key is in this Mac's keychain")
            continue
        if dry_run:
            print(f"would revoke {label}")
            continue
        status, err = call(bearer, "DELETE", f"/certificates/{cert['id']}")
        print(f"revoked {label}" if status in (200, 204) else f"::warning::Could not revoke {label}: HTTP {status} {err}")


if __name__ == "__main__":
    if len(sys.argv) < 2 or sys.argv[1] != "prune":
        sys.exit(__doc__)
    prune("--dry-run" in sys.argv[2:], "--ephemeral" in sys.argv[2:])

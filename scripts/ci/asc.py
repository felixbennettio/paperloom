#!/usr/bin/env python3
"""Xcode Cloud from the command line through the App Store Connect API,
signed with a team API key (~/.appstoreconnect/private_keys/AuthKey_<id>.p8,
read by openssl and never printed; the key id in ASC_KEY_ID, the issuer in
ASC_ISSUER_ID or ~/.appstoreconnect/issuer_id). See docs/ci.md.

  scripts/ci/asc.py workflows                        products and workflows
  scripts/ci/asc.py start <workflow name> <branch>   start a build of a branch
  scripts/ci/asc.py run <build run id>               its state, and errors of failed actions
  scripts/ci/asc.py screens <build run id> <folder>  test results, and the UI
                                                     tests' screenshots from each
                                                     destination's result bundle
"""
import base64, json, os, subprocess, sys, tempfile, time, urllib.request, urllib.error
from pathlib import Path

KEY_ID = os.environ.get("ASC_KEY_ID", "D4XGJM28DR")
ISSUER = os.environ.get("ASC_ISSUER_ID") or open(os.path.expanduser("~/.appstoreconnect/issuer_id")).read().strip()
KEY = os.path.expanduser(f"~/.appstoreconnect/private_keys/AuthKey_{KEY_ID}.p8")
BASE = "https://api.appstoreconnect.apple.com"


def b64(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def der_to_raw(der: bytes) -> bytes:
    assert der[0] == 0x30
    i = 2 if der[1] < 0x80 else 2 + (der[1] & 0x7F)
    out = b""
    for _ in range(2):
        assert der[i] == 0x02
        n = der[i + 1]
        out += der[i + 2 : i + 2 + n].lstrip(b"\x00").rjust(32, b"\x00")
        i += 2 + n
    return out


def token() -> str:
    now = int(time.time())
    head = b64(json.dumps({"alg": "ES256", "kid": KEY_ID, "typ": "JWT"}).encode())
    body = b64(json.dumps({"iss": ISSUER, "iat": now, "exp": now + 1200, "aud": "appstoreconnect-v1"}).encode())
    msg = f"{head}.{body}".encode()
    with tempfile.NamedTemporaryFile() as f:
        f.write(msg)
        f.flush()
        der = subprocess.run(["openssl", "dgst", "-sha256", "-sign", KEY, f.name], check=True, capture_output=True).stdout
    return f"{head}.{body}.{b64(der_to_raw(der))}"


def call(method: str, path: str, body=None, tries=3):
    for attempt in range(tries):
        req = urllib.request.Request(BASE + path, method=method, data=json.dumps(body).encode() if body else None)
        req.add_header("Authorization", "Bearer " + token())
        req.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            sys.exit(f"HTTP {e.code}: {e.read().decode()[:2000]}")
        except (urllib.error.URLError, ConnectionError, TimeoutError):
            if attempt == tries - 1:
                raise
            time.sleep(5)


def paged(path):
    while path:
        r = call("GET", path)
        yield from r["data"]
        nxt = r.get("links", {}).get("next")
        path = nxt.replace(BASE, "") if nxt else None


def workflows():
    found = []
    for p in call("GET", "/v1/ciProducts")["data"]:
        for w in call("GET", f"/v1/ciProducts/{p['id']}/workflows")["data"]:
            found.append((p["attributes"]["name"], w["attributes"]["name"], w["id"], p["id"]))
    return found


def git_ref(workflow_id: str, branch: str) -> str:
    repo = call("GET", f"/v1/ciWorkflows/{workflow_id}/repository")["data"]["id"]
    for r in paged(f"/v1/scmRepositories/{repo}/gitReferences?limit=200"):
        if r["attributes"].get("kind") == "BRANCH" and r["attributes"].get("name") == branch:
            return r["id"]
    sys.exit(f"no branch {branch}")


def main():
    cmd = sys.argv[1]
    if cmd == "workflows":
        for product, name, wid, pid in workflows():
            print(product, "|", name, "|", wid)
    elif cmd == "start":
        name, branch = sys.argv[2], sys.argv[3]
        wid = next(w for _, n, w, _ in workflows() if n == name)
        run = call("POST", "/v1/ciBuildRuns", {"data": {"type": "ciBuildRuns", "relationships": {
            "workflow": {"data": {"type": "ciWorkflows", "id": wid}},
            "sourceBranchOrTag": {"data": {"type": "scmGitReferences", "id": git_ref(wid, branch)}}}}})
        a = run["data"]["attributes"]
        print(run["data"]["id"], a.get("number"), a.get("executionProgress"))
    elif cmd == "screens":
        screens(sys.argv[2], Path(sys.argv[3]))
    elif cmd == "run":
        rid = sys.argv[2]
        a = call("GET", f"/v1/ciBuildRuns/{rid}")["data"]["attributes"]
        print("build", a.get("number"), a.get("executionProgress"), a.get("completionStatus"))
        for act in call("GET", f"/v1/ciBuildRuns/{rid}/actions")["data"]:
            aa = act["attributes"]
            print("action", aa.get("name"), aa.get("executionProgress"), aa.get("completionStatus"))
            if aa.get("completionStatus") not in (None, "SUCCEEDED"):
                for i in call("GET", f"/v1/ciBuildActions/{act['id']}/issues?limit=50")["data"]:
                    ia = i["attributes"]
                    if ia.get("issueType") != "WARNING":
                        print("  ", ia.get("issueType"), (ia.get("fileSource") or {}).get("path"), (ia.get("fileSource") or {}).get("lineNumber"), "-", (ia.get("message") or "")[:400])


def screens(rid: str, out: Path):
    """Test results per destination, then each result bundle's attachments
    (scripts/ci/xcresult-attachments.py) in <out>."""
    out.mkdir(parents=True, exist_ok=True)
    extract = Path(__file__).with_name("xcresult-attachments.py")
    for act in call("GET", f"/v1/ciBuildRuns/{rid}/actions")["data"]:
        for t in call("GET", f"/v1/ciBuildActions/{act['id']}/testResults?limit=200")["data"]:
            ta = t["attributes"]
            for d in ta.get("destinationTestResults") or []:
                print(ta.get("className"), ta.get("name"), d.get("deviceName"), d.get("status"))
            if ta.get("status") not in ("SUCCESS", None) and ta.get("message"):
                print("  ", ta["message"][:800])
        for art in paged(f"/v1/ciBuildActions/{act['id']}/artifacts?limit=50"):
            a = art["attributes"]
            if a.get("fileType") != "RESULT_BUNDLE" or "test-without-building" not in a.get("fileName", ""):
                continue
            with tempfile.TemporaryDirectory() as work:
                bundle = Path(work) / "bundle.zip"
                urllib.request.urlretrieve(a["downloadUrl"], bundle)
                subprocess.run([sys.executable, "-I", str(extract), str(bundle), str(out)], check=True)


if __name__ == "__main__":
    main()

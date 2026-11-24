#!/bin/bash
# CircleCI from a terminal, with a personal API token kept in the macOS
# Keychain as "circleci-token" (never printed; curl reads it from stdin, so
# it is not in any process's arguments). See docs/ci.md.
#   scripts/ci/circleci.sh trigger <param>=true [branch]   start a pipeline (android_test, desktop_test…)
#   scripts/ci/circleci.sh wait <pipeline id>              wait for it; print each job's name, number, state
#   scripts/ci/circleci.sh latest                          the newest pipeline on free-tier-ci
#   scripts/ci/circleci.sh artifacts <job> <folder> [part] download a job's artifacts whose path contains part
set -euo pipefail
SLUG="circleci/GdG1CWb2FwAxcUqdaTVjzK/Qvvp5juwhsDVi2Xh8nqMgZ"
DEFINITION="16be4452-3c13-4408-a937-9b0043f54220"
TOKEN=$(security find-generic-password -s circleci-token -w | tr -d '\n')
api() { printf 'header = "Circle-Token: %s"\n' "$TOKEN" | curl -sf --retry 3 -K - -H "Content-Type: application/json" "$@"; }
case "${1:?command}" in
  trigger)
    param=${2%%=*} value=${2#*=} branch=${3:-free-tier-ci}
    api -X POST "https://circleci.com/api/v2/project/$SLUG/pipeline/run" \
      -d "{\"definition_id\":\"$DEFINITION\",\"config\":{\"branch\":\"$branch\"},\"checkout\":{\"branch\":\"$branch\"},\"parameters\":{\"$param\":$value}}" |
      python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("number"), d.get("id"), d.get("state"))'
    ;;
  latest)
    api "https://circleci.com/api/v2/project/$SLUG/pipeline?branch=free-tier-ci" |
      python3 -c 'import json,sys; d=json.load(sys.stdin)["items"][0]; print(d["id"], d["number"])'
    ;;
  artifacts)
    job=$2 dir=$3 part=${4:-}
    api "https://circleci.com/api/v2/project/$SLUG/$job/artifacts" |
      python3 -c 'import json,sys; [print(a["path"], a["url"]) for a in json.load(sys.stdin)["items"]]' |
      while read -r path url; do
        case "$path" in *"$part"*) ;; *) continue ;; esac
        mkdir -p "$dir/$(dirname "$path")"
        api -L -o "$dir/$path" "$url"
      done
    ;;
  wait)
    id=$2
    while :; do
      out=$(api "https://circleci.com/api/v2/pipeline/$id/workflow" | python3 -c '
import json,sys
d=json.load(sys.stdin)["items"]
print("running" if not d or any(w["status"] in ("running", "on_hold") for w in d) else "done")
' || echo running)
      [ "$out" = done ] && break
      sleep 30
    done
    for w in $(api "https://circleci.com/api/v2/pipeline/$id/workflow" | python3 -c 'import json,sys; print(" ".join(w["id"] for w in json.load(sys.stdin)["items"]))'); do
      api "https://circleci.com/api/v2/workflow/$w/job" | python3 -c 'import json,sys; [print(j["name"], j.get("job_number"), j["status"]) for j in json.load(sys.stdin)["items"]]'
    done
    ;;
esac

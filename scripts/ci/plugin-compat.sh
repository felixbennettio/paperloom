#!/usr/bin/env bash
# Starts pinned releases of common Zotero and Obsidian plugins in the plugin runtimes
# (app/src/lib/plugin-compat.test.ts). Run from app/ after npm install.
# A download that fails is reported and skipped, so a network hiccup does
# not fail the Nightly; a plugin that no longer starts does.
set -eu

dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
mkdir -p "$dir/plugins"

# name|url|sha256
plugins="
pdf-translate|https://github.com/windingwind/zotero-pdf-translate/releases/download/v2.4.8/translate-for-zotero.xpi|8145e9972c507f6baee93b93d2c2e8e7fe24388c58f2cebe0a37e29db5de26e6
reading-list|https://github.com/Dominic-DallOsto/zotero-reading-list/releases/download/v1.7.1/zotero-reading-list.xpi|e49424154b808bd75aa1a978dcc65bc3b0bcaad36a4ef3cfe9cfda0400ac1995
zotero-actions-tags|https://github.com/windingwind/zotero-actions-tags/releases/download/v2.6.2/actions-and-tags-for-zotero.xpi|05e0bacc60ec5f937f3a99db491eb723b98bb15d2dabe887d1eca9158b1fa495
zotero-format-metadata|https://github.com/northword/zotero-format-metadata/releases/download/v4.0.1/linter-for-zotero.xpi|105c8a04f5b7c720bbaf83c9d2c27bd47eaea0f22d27bb9d11d10351da258c47
"

summary=${GITHUB_STEP_SUMMARY:-/dev/null}
for line in $plugins; do
  IFS='|' read -r name url sum <<<"$line"
  if ! curl -fsSL --retry 3 --retry-delay 10 -m 300 -o "$dir/$name.xpi" "$url"; then
    echo "Could not download $name; skipping it." | tee -a "$summary"
    continue
  fi
  echo "$sum  $dir/$name.xpi" | sha256sum -c - >/dev/null || { echo "$name does not match its pinned checksum." >&2; exit 1; }
  mkdir -p "$dir/plugins/$name"
  python3 -c 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])' "$dir/$name.xpi" "$dir/plugins/$name"
done

# Obsidian plugins ship loose files: name|release base URL|file:sha256,...
# file:optional explicitly marks an absent optional asset (not downloaded).
obsidian="
highlightr|https://github.com/chetachiezikeuzor/Highlightr-Plugin/releases/download/1.2.2|main.js:fd4327f3d69d72fc10936aa12af51ba195248db3a62eab0ce33d6ec9bdb0bb8d,manifest.json:2c1b21c61afd8ebeacd365774553e42d317f372a8c45f07b930909e451f787a3,styles.css:6fdd3e52f7df3ff4a74297d346fb4832309406d2aa061f7c9dc12591cb89a72f
nldates|https://github.com/argenos/nldates-obsidian/releases/download/0.6.2|main.js:387d36a43412f761c0c69320655a7ec09aa9189ae2267550224cacc861e63fd6,manifest.json:dfdcbdd8272d839ec0620af3a1fa7ab1f785ad3cdc6feed1f18ccb7b09621f29,styles.css:optional
various-complements|https://github.com/tadashi-aikawa/obsidian-various-complements-plugin/releases/download/11.4.2|main.js:8de946308b375919f74006a40717f1f598c614205b93d0887362170477a22391,manifest.json:dcee24cdee134542ced33a9bfc493fdd5996ef2552a0c681e3f5e9994b39327c,styles.css:b79acfb2babedd0329b569febb7d509e5b131be06c583526326275a2fe9cfc54
dataview|https://github.com/blacksmithgu/obsidian-dataview/releases/download/0.5.68|main.js:794e9eaede73920bb8d54b0eda4f5de2182d698cc638774500f24f14bcd4da0b,manifest.json:9235db47112da81b85591c79ecb9ae2574e5e72207056e976472f90616286185,styles.css:3306dd9032e00f989ba7233a37fd255bc4d3f4340cee661762e952f3f6aa1de9
"
for line in $obsidian; do
  IFS='|' read -r name base files <<<"$line"
  mkdir -p "$dir/obsidian/$name"
  ok=true
  for entry in ${files//,/ }; do
    file=${entry%%:*} sum=${entry#*:}
    if [ "$sum" = optional ]; then continue; fi
    if ! curl -fsSL --retry 3 --retry-delay 10 -m 300 -o "$dir/obsidian/$name/$file" "$base/$file"; then ok=false; break; fi
    echo "$sum  $dir/obsidian/$name/$file" | sha256sum -c - >/dev/null || { echo "$name/$file does not match its pinned checksum." >&2; exit 1; }
  done
  if $ok; then mv "$dir/obsidian/$name" "$dir/plugins/$name"; else echo "Could not download $name; skipping it." | tee -a "$summary"; fi
done

if [ -z "$(ls -A "$dir/plugins" 2>/dev/null)" ]; then
  echo "No plugin could be downloaded; compatibility not checked." | tee -a "$summary"
  exit 0
fi
PAPERLOOM_PLUGIN_COMPAT="$dir/plugins" npx vitest run src/lib/plugin-compat.test.ts

#!/usr/bin/env bash
# Assemble the GitHub Pages site in site/dist: the static files here plus
# the npm package's web build (which this runs npm/build.sh to produce).
set -euo pipefail
cd "$(dirname "$0")/.."

# The page's Content-Security-Policy allows its one inline script by hash:
# refuse to build if the script changed without the hash.
hash=$(python3 -c 'import re,hashlib,base64; s=open("site/index.html", encoding="utf-8").read(); m=re.search(r"<script>(.*?)</script>", s, re.S); print(base64.b64encode(hashlib.sha256(m.group(1).encode()).digest()).decode())')
grep -q "'sha256-$hash'" site/index.html || { echo "site/index.html: update the CSP hash of the inline script to sha256-$hash" >&2; exit 1; }

./npm/build.sh
rm -rf site/dist
mkdir -p site/dist/pkg/dist site/dist/pkg/web
cp site/index.html site/style.css site/app.js site/worker.js site/favicon.svg site/dist/
cp -R site/samples site/dist/
# The package's own browser entry, laid out as in the package so its
# relative imports resolve.
cp npm/dist/web.mjs npm/dist/core.mjs site/dist/pkg/dist/
cp npm/web/smallpeg_wasm.js npm/web/smallpeg_wasm_bg.wasm site/dist/pkg/web/
du -sh site/dist
echo "site ready in site/dist (serve it over HTTP: python3 -m http.server -d site/dist)"

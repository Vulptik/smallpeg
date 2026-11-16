#!/usr/bin/env bash
# Assemble the GitHub Pages site in site/dist: the static files here plus
# the npm package's web build (which this runs npm/build.sh to produce).
set -euo pipefail
cd "$(dirname "$0")/.."

./npm/build.sh
rm -rf site/dist
mkdir -p site/dist/pkg
cp site/index.html site/style.css site/app.js site/worker.js site/favicon.svg site/dist/
cp -R site/samples site/dist/
cp npm/web/smallpeg_wasm.js npm/web/smallpeg_wasm_bg.wasm site/dist/pkg/
du -sh site/dist
echo "site ready in site/dist (serve it over HTTP: python3 -m http.server -d site/dist)"

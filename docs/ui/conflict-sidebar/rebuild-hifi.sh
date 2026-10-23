#!/bin/sh
# Rebuild the high-fidelity OpenPencil document from build-hifi.js and export every board at 2x.
set -eu
cd "$(dirname "$0")"
OP=./node_modules/.bin/openpencil
$OP import seed.html -o hifi.fig --pageName "Conflict hi-fi" --json >/dev/null
$OP eval hifi.fig --stdin -w < build-hifi.js >/dev/null 2>&1
$OP tree hifi.fig --depth 0 --json | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{for(const n of JSON.parse(s))console.log(n.id+"\t"+n.name)})' > hifi.tsv
mkdir -p exports/hifi
rm -f exports/hifi/*.png
while IFS="$(printf '\t')" read -r id name; do
  slug=$(printf '%s' "$name" | cut -c1-2)
  $OP export hifi.fig --node "$id" -s "${SCALE:-2}" -o "exports/hifi/$slug.png" >/dev/null 2>&1
done < hifi.tsv
cat hifi.tsv

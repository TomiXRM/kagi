#!/bin/sh
# Rebuild the editable OpenPencil document from build-boards.js and export every board.
set -eu
cd "$(dirname "$0")"
OP=./node_modules/.bin/openpencil
$OP import seed.html -o conflict-sidebar.fig --pageName "Conflict sidebar" --json >/dev/null
$OP eval conflict-sidebar.fig --stdin -w < build-boards.js >/dev/null
$OP tree conflict-sidebar.fig --depth 0 --json | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{for(const n of JSON.parse(s))console.log(n.id+"\t"+n.name)})' > boards.tsv
rm -f exports/board-*.png
while IFS="$(printf '\t')" read -r id name; do
  slug=$(printf '%s' "$name" | cut -c1-3 | tr -d ' ')
  $OP export conflict-sidebar.fig --node "$id" -s "${SCALE:-1}" -o "exports/board-$slug.png" >/dev/null
done < boards.tsv
cat boards.tsv

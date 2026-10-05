#!/usr/bin/env bash
# Pair captured inventory PNGs by name and concatenate before | after.
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <before-dir> <after-dir>" >&2
    exit 2
fi
before_dir=$1
after_dir=$2
for dir in "$before_dir" "$after_dir"; do
    if [[ ! -d "$dir" ]]; then
        echo "inventory-diff: directory not found: $dir" >&2
        exit 1
    fi
done

shopt -s nullglob
pairs=()
missing=0
for file in "$before_dir"/*.png; do
    name=${file##*/}
    [[ $name == *-before-after.png ]] && continue
    if [[ -f "$after_dir/$name" ]]; then
        pairs+=("$name")
    else
        echo "inventory-diff: missing after image: $name" >&2
        missing=1
    fi
done
for file in "$after_dir"/*.png; do
    name=${file##*/}
    [[ $name == *-before-after.png ]] && continue
    if [[ ! -f "$before_dir/$name" ]]; then
        echo "inventory-diff: missing before image: $name" >&2
        missing=1
    fi
done
(( missing == 0 )) || exit 1
if (( ${#pairs[@]} == 0 )); then
    echo "inventory-diff: no matching PNG pairs" >&2
    exit 1
fi

if command -v magick >/dev/null 2>&1; then
    method=magick
elif command -v sips >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1; then
    method=bmp
else
    echo "inventory-diff: 合成なし (requires magick or sips + python3)" >&2
    exit 1
fi

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
work_dir=$(mktemp -d "$after_dir/.inventory-diff.XXXXXXXX")
trap 'rm -rf -- "$work_dir"' EXIT
for name in "${pairs[@]}"; do
    output="$after_dir/${name%.png}-before-after.png"
    # Never leave a truncated generated PNG on a failed conversion.
    temp_output="$work_dir/combined.png"
    if [[ $method == magick ]]; then
        magick "$before_dir/$name" "$after_dir/$name" -background none +append "$temp_output"
    else
        sips -s format bmp "$before_dir/$name" --out "$work_dir/before.bmp" >/dev/null
        sips -s format bmp "$after_dir/$name" --out "$work_dir/after.bmp" >/dev/null
        python3 "$script_dir/inventory-diff.py" "$work_dir/before.bmp" "$work_dir/after.bmp" "$temp_output"
    fi
    mv -f -- "$temp_output" "$output"
    echo "$output"
done

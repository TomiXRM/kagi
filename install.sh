#!/bin/sh
# Install release archives without modifying shell configuration (ADR-0047).
set -eu

fail() {
    printf 'kagi install: %s\n' "$1" >&2
    exit 1
}

# Only explicit fixture URL overrides may use plain HTTP.
download() {
    policy=$1
    shift
    if [ -z "$policy" ]; then
        curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL "$@"
    else
        curl -fsSL "$@"
    fi
}

main() {
    version=
    prefix=
    dry_run=false
    path_hint=true
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --version|--prefix)
                option=$1
                [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$option requires a value"
                case "$option" in
                    --version) version=$2 ;;
                    --prefix) prefix=$2 ;;
                esac
                shift 2 ;;
            --dry-run) dry_run=true; shift ;;
            --no-modify-path) path_hint=false; shift ;;
            --help|-h)
                printf '%s\n' 'Usage: install.sh [--version vX.Y.Z] [--prefix DIR] [--dry-run] [--no-modify-path]' \
                    'No shell files are modified; --no-modify-path also suppresses the PATH hint.'
                return ;;
            *) fail "unknown option: $1" ;;
        esac
    done

    case "$(uname -s)" in
        Darwin) os=macos ;;
        Linux) os=linux ;;
        *) fail 'supported systems are macOS arm64 and Linux arm64/x86_64' ;;
    esac
    case "$(uname -m)" in
        arm64|aarch64) arch=aarch64; checksum_arch=arm64 ;;
        x86_64|amd64) arch=x86_64; checksum_arch=x86_64 ;;
        *) fail 'unsupported CPU architecture' ;;
    esac
    [ "$os-$arch" != macos-x86_64 ] || fail 'macOS releases support arm64 only'
    if [ "$os" = macos ]; then arch=arm64; fi
    command -v curl >/dev/null 2>&1 || fail 'curl is required'
    if [ -z "$version" ]; then
        api=${KAGI_RELEASE_API_URL:-https://api.github.com/repos/TomiXRM/kagi/releases/latest}
        response=$(download "${KAGI_RELEASE_API_URL:+override}" "$api" 2>&1) ||
            fail "release API unavailable; specify a tag with --version vX.Y.Z: $(printf '%s' "$response" | tr '\r\n' '  ')"
        version=$(printf '%s\n' "$response" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
        [ -n "$version" ] || fail 'release API returned no tag; specify a tag with --version vX.Y.Z'
    fi
    case "$version" in
        *[!v0-9.]*|'') fail 'invalid release tag; specify --version vX.Y.Z' ;;
    esac
    valid=$(printf '%s\n' "$version" | sed -n '/^v[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*$/p')
    [ "$valid" = "$version" ] || fail 'invalid release tag; specify --version vX.Y.Z'
    asset="kagi-${version#v}-$arch-$os.tar.gz"
    sums="SHA256SUMS-$os-$checksum_arch.txt"
    release_base=${KAGI_RELEASE_BASE_URL:-https://github.com/TomiXRM/kagi/releases/download}
    url="${release_base%/}/$version"

    if [ -n "$prefix" ]; then
        case "$prefix" in /*) ;; *) prefix="$PWD/$prefix" ;; esac
        bin_dir="$prefix/bin"
        app_dir=$prefix
    else
        [ -n "${HOME:-}" ] || fail 'HOME must be set, or provide --prefix'
        prefix="$HOME/.local"
        bin_dir="$prefix/bin"
        app_dir="$HOME/Applications"
        if [ -d /Applications ] && [ -w /Applications ]; then
            app_dir=/Applications
        fi
    fi
    if [ "$dry_run" = true ]; then
        printf 'Download %s/%s; verify %s\n' "$url" "$asset" "$sums"
        if [ "$os" = macos ]; then
            printf 'Install %s/Kagi.app and %s/kagi; remove app quarantine\n' "$app_dir" "$bin_dir"
        else
            printf 'Install bin/kagi and desktop resources under %s\n' "$prefix"
        fi
        return
    fi

    # Download and validate everything before touching the installation.
    temp=$(mktemp -d "${TMPDIR:-/tmp}/kagi-install.XXXXXXXX" 2>/dev/null) || fail 'cannot create temporary directory'
    trap 'rm -rf "$temp"' 0
    trap 'exit 1' HUP INT TERM
    download "${KAGI_RELEASE_BASE_URL:+override}" "$url/$asset" -o "$temp/$asset" || fail "cannot download $asset"
    download "${KAGI_RELEASE_BASE_URL:+override}" "$url/$sums" -o "$temp/$sums" || fail "cannot download $sums"
    expected=
    while read -r digest name extra; do
        if [ "$name" = "$asset" ] || [ "$name" = "*$asset" ]; then
            [ -z "$expected" ] && [ -z "$extra" ] || fail 'ambiguous checksum entry'
            expected=$digest
        fi
    done < "$temp/$sums"
    case "$expected" in ''|*[!0-9a-fA-F]*) fail "missing or invalid checksum for $asset" ;; esac
    [ "${#expected}" -eq 64 ] || fail "invalid checksum for $asset"
    if [ "$os" = macos ]; then
        actual=$(shasum -a 256 "$temp/$asset" 2>/dev/null) || fail 'SHA256 verification requires shasum'
    else
        actual=$(sha256sum "$temp/$asset" 2>/dev/null) || fail 'SHA256 verification requires sha256sum'
    fi
    actual=${actual%% *}
    expected=$(printf '%s' "$expected" | tr 'A-F' 'a-f')
    [ "$actual" = "$expected" ] || fail "SHA256 mismatch for $asset"
    mkdir "$temp/unpacked" 2>/dev/null || fail 'cannot create extraction directory'
    tar -xzf "$temp/$asset" -C "$temp/unpacked" 2>/dev/null || fail 'cannot extract release archive'

    if [ "$os" = macos ]; then
        app="$temp/unpacked/Kagi.app"
        [ -x "$app/Contents/MacOS/kagi" ] && [ -L "$temp/unpacked/bin/kagi" ] || fail 'invalid macOS archive layout'
        codesign --verify --deep --strict "$app" >/dev/null 2>&1 || fail 'app signature verification failed'
        xattr -dr com.apple.quarantine "$app" 2>/dev/null || fail 'cannot remove app quarantine'
        mkdir -p "$app_dir" "$bin_dir" 2>/dev/null || fail 'installation directories are not writable'
        # Stage on the destination filesystem so app replacement uses rename.
        staged=$(mktemp -d "$app_dir/.kagi-install.XXXXXXXX" 2>/dev/null) || fail 'cannot stage application'
        trap 'rm -rf "$temp" "$staged"' 0
        cp -R "$app" "$staged/Kagi.app" 2>/dev/null || fail 'cannot stage application'
        codesign --verify --deep --strict "$staged/Kagi.app" >/dev/null 2>&1 || fail 'staged app signature verification failed'
        [ ! -e "$bin_dir/kagi" ] || [ ! -d "$bin_dir/kagi" ] || fail 'CLI destination is a directory'
        link_stage=$(mktemp -d "$bin_dir/.kagi-link.XXXXXXXX" 2>/dev/null) || fail 'cannot stage CLI symlink'
        trap 'rm -rf "$temp" "$staged" "$link_stage"' 0
        ln -s "$app_dir/Kagi.app/Contents/MacOS/kagi" "$link_stage/kagi" 2>/dev/null || fail 'cannot stage CLI symlink'
        # Do not let catchable signals delete the saved app between renames.
        trap '' HUP INT TERM
        if [ -e "$app_dir/Kagi.app" ] || [ -L "$app_dir/Kagi.app" ]; then
            mv "$app_dir/Kagi.app" "$staged/previous.app" 2>/dev/null || fail 'cannot move existing application'
        fi
        if ! mv "$staged/Kagi.app" "$app_dir/Kagi.app" 2>/dev/null; then
            if [ -e "$staged/previous.app" ] || [ -L "$staged/previous.app" ]; then
                if ! mv "$staged/previous.app" "$app_dir/Kagi.app" 2>/dev/null; then
                    trap 'rm -rf "$temp" "$link_stage"' 0
                    fail "cannot restore application; previous app retained at $staged/previous.app"
                fi
            fi
            fail 'cannot install application'
        fi
        if ! mv -f "$link_stage/kagi" "$bin_dir/kagi" 2>/dev/null; then
            if ! mv "$app_dir/Kagi.app" "$staged/failed.app" 2>/dev/null; then
                trap 'rm -rf "$temp" "$link_stage"' 0
                fail "cannot restore application; previous app retained at $staged/previous.app"
            fi
            if [ -e "$staged/previous.app" ] || [ -L "$staged/previous.app" ]; then
                if ! mv "$staged/previous.app" "$app_dir/Kagi.app" 2>/dev/null; then
                    trap 'rm -rf "$temp" "$link_stage"' 0
                    fail "cannot restore application; previous app retained at $staged/previous.app"
                fi
            fi
            fail 'cannot install CLI symlink'
        fi
        trap 'exit 1' HUP INT TERM
    else
        # Linux tarballs retain their single top-level package directory.
        set -- "$temp/unpacked"/*
        [ "$#" -eq 1 ] && [ -d "$1" ] || fail 'invalid Linux archive layout'
        package=$1
        [ -x "$package/bin/kagi" ] && [ -f "$package/share/applications/com.tomixrm.kagi.desktop" ] \
            && [ -f "$package/share/icons/hicolor/512x512/apps/kagi.png" ] || fail 'incomplete Linux archive'
        for relative in bin/kagi share/applications/com.tomixrm.kagi.desktop share/icons/hicolor/512x512/apps/kagi.png; do
            destination="$prefix/$relative"
            mkdir -p "${destination%/*}" 2>/dev/null || fail 'installation directories are not writable'
            [ ! -d "$destination" ] || fail 'installation file destination is a directory'
        done
        staged=$(mktemp -d "$prefix/.kagi-install.XXXXXXXX" 2>/dev/null) || fail 'cannot stage Linux installation'
        trap 'rm -rf "$temp" "$staged"' 0
        cp -R "$package/bin" "$package/share" "$staged/" 2>/dev/null || fail 'cannot stage Linux installation'
        for relative in share/applications/com.tomixrm.kagi.desktop share/icons/hicolor/512x512/apps/kagi.png bin/kagi; do
            mv -f "$staged/$relative" "$prefix/$relative" 2>/dev/null ||
                fail 'partial Linux install: resources may have changed; executable is replaced last'
        done
    fi
    printf 'Installed Kagi %s (%s/kagi)\n' "$version" "$bin_dir"
    if [ "$path_hint" = true ]; then
        case ":${PATH:-}:" in
            *":$bin_dir:"*) ;;
            *) printf 'Add %s to PATH to run kagi.\n' "$bin_dir" ;;
        esac
    fi
}

main "$@"

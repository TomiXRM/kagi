#!/usr/bin/env bash
# Arduino "Blink" conflict fixture — used to capture the Conflict Mode README
# asset (docs/images/conflict-blink.*). Two branches tune the same LED config
# differently, so merging them conflicts on BOTH the LED pin and the blink
# interval — exactly the "pick the pin from one branch, the interval from the
# other" story the screenshot tells.
#
# Usage:
#   scripts/make_blink_fixture.sh [DEST]
# DEST defaults to a mktemp dir under /tmp. The repo path is printed on the last
# line. Pass --merge to leave the repo mid-merge (in conflict) so Kagi opens
# straight into Conflict Mode.
set -euo pipefail

MERGE=0
ARGS=()
for a in "$@"; do
  case "$a" in
    --merge) MERGE=1 ;;
    *) ARGS+=("$a") ;;
  esac
done

DEST="${ARGS[0]:-$(mktemp -d /tmp/kagi-blink.XXXXXX)}"
case "$DEST" in
  /tmp/*|/private/tmp/*|/var/folders/*) ;;
  *) echo "error: DEST must be under /tmp (got: $DEST)" >&2; exit 1 ;;
esac

mkdir -p "$DEST"
cd "$DEST"
git init -q -b main
git config user.name "Maker"
git config user.email "maker@example.com"
git config commit.gpgsign false

write_blink() {
  # $1 = LED pin expression, $2 = blink interval (ms)
  cat > Blink.ino <<EOF
// Blink — turn an LED on and off once a second.

// The pin the LED is wired to.
#define LED_PIN $1

// How long the LED stays on / off, in milliseconds.
#define BLINK_MS $2

void setup() {
  pinMode(LED_PIN, OUTPUT);
}

void loop() {
  digitalWrite(LED_PIN, HIGH);
  delay(BLINK_MS);
  digitalWrite(LED_PIN, LOW);
  delay(BLINK_MS);
}
EOF
}

# ── main: the original sketch ──────────────────────────────────
write_blink "13" "1000"
git add Blink.ino
git commit -qm "Blink: turn the LED on and off once a second"

# ── feature/onboard-led: use the board's built-in LED, blink a bit faster ──
git checkout -q -b feature/onboard-led
write_blink "LED_BUILTIN" "500"
git commit -qam "Use the built-in LED and blink twice a second"

# ── feature/fast-blink: external LED on pin 2, blink fast ──
git checkout -q main
git checkout -q -b feature/fast-blink
write_blink "2" "200"
git commit -qam "Wire the LED to pin 2 and blink fast (200ms)"

git checkout -q feature/onboard-led

if [ "$MERGE" = "1" ]; then
  # Leave the repo mid-merge so Kagi opens into Conflict Mode. The pin line
  # (LED_BUILTIN vs 2) and the interval line (500 vs 200) both conflict.
  git merge feature/fast-blink >/dev/null 2>&1 || true
fi

echo "$DEST"

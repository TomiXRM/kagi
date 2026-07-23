# ADR-0130: Bundle the Japanese UI font fallback

- Status: Accepted (amended 2026-07-23 — ship static instances, not the variable font)
- Date: 2026-07-21
- Related: ADR-0036, ADR-0047, cosmic-text PR #522, Zed #60155

## Context

Kagi embeds Inter for UI text, but Inter has no CJK glyphs. On Linux, GPUI's
cosmic-text backend therefore selected a host-installed fallback. The result
varied by Ubuntu installation and package format: Japanese text could mix
fonts, use larger-looking metrics, or render Han characters with Simplified
Chinese glyph forms. AppImage and tar.gz cannot declare a host font dependency;
the Debian package only recommended `fonts-noto-cjk`.

Kagi already carries a temporary cosmic-text locale fix for `ja-JP` Han
unification, but that fix can only choose `Noto Sans CJK JP` when an appropriate
host font exists. It does not make rendering reproducible on a minimal system.

## Decision

1. Embed the unmodified Google Fonts `Noto Sans JP` variable font under its SIL
   OFL 1.1 license alongside Inter and JetBrains Mono.
2. Register all three families synchronously through the existing startup
   `TextSystem::add_fonts` call, before opening a window.
3. Attach `FontFallbacks(["Noto Sans JP"])` to gpui-component's window `Root`.
   Descendant Kagi elements and overlays inherit the same Inter -> Noto Sans JP
   stack, including entities rendered after asynchronous repository loads.
4. Keep the cosmic-text locale patch until upstream PR #522 ships in GPUI's
   selected cosmic-text release. The explicit fallback is the deterministic
   primary path; the patch still protects other implicit Han fallback paths.
5. Remove the Debian `fonts-noto-cjk` recommendation because every native
   package now contains the required glyphs.
6. On Linux, test the real `CosmicTextSystem` with system fonts disabled and
   assert that both regular and bold text split into Inter and Noto font runs.
   This guards the non-400-weight fallback regression tracked in Zed #60155.

The source is Google Fonts commit
`2f6daa88e1e71320a6fe71cc91ecbfc018928737`, file
`ofl/notosansjp/NotoSansJP[wght].ttf`, SHA-256
`c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f`.

## Amendment (2026-07-23): ship static instances, not the variable font

The variable font shipped by the original decision rendered Japanese text **thin**
on Ubuntu. Root cause: `NotoSansJP[wght].ttf` has `wght` axis default `100` and
`OS/2.usWeightClass = 100` (its family/PostScript names are literally
"Noto Sans JP Thin"). On Linux, GPUI's cosmic-text renders a *variable* fallback
face at its default axis value and does not apply the requested weight to the
fallback run, so every weight resolved to Thin. macOS/CoreText interpolated the
axis correctly, which is why the bug was Linux-only. Decision 6's regression test
only asserted that Latin and Japanese split into *two runs*; it never checked the
resolved weight, so it passed while the text was thin — exactly the Zed #60155
failure mode it was meant to guard.

Amended decision:

- Replace the single variable font with **static** Regular(400) and Bold(700)
  instances (`assets/fonts/NotoSansJP-{Regular,Bold}.ttf`), produced from the same
  pinned upstream file by `scripts/instance_noto.py` (fontTools instancer). This
  mirrors how Inter and JetBrains Mono are already bundled as static RIBBI pairs.
- fontdb now indexes two faces at `usWeightClass` 400 and 700, so a fallback
  request at NORMAL/BOLD resolves the matching static face directly — no variable
  axis application is needed on the fragile Linux path. Intermediate weights snap
  to the nearest static face, exactly as Inter's Regular/Bold pair already does.
- Strengthen decision 6's Linux test: it now registers the static pair and asserts
  that NORMAL and BOLD Japanese resolve to *distinct* font ids (with the old single
  variable face they collapsed to one thin face). A separate static unit test
  asserts the bundled faces are non-variable and carry weight 400/700.

## Consequences

- Native binaries grow by roughly 11.5 MB before package compression (two static
  faces, up from the ~9.6 MB single variable font); each static retains the full
  ~17.9k-glyph coverage.
- Japanese rendering no longer depends on locale packages, fontconfig order,
  `apt` recommendations, or whether the user chose `.deb`, tar.gz, or AppImage.
- Inter remains the primary UI face; only unsupported glyphs use Noto Sans JP.
- Japanese now renders at the requested weight on Linux, not Thin.
- Other CJK locales intentionally receive the Japanese glyph variant in Kagi's
  UI. A future locale-aware fallback list may add dedicated KR/SC/TC families
  if Kagi adds those interface languages.

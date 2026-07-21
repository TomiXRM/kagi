# ADR-0130: Bundle the Japanese UI font fallback

- Status: Accepted
- Date: 2026-07-21
- Related: ADR-0036, ADR-0047, cosmic-text PR #522

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

## Consequences

- Native binaries grow by roughly 9.6 MB before package compression.
- Japanese rendering no longer depends on locale packages, fontconfig order,
  `apt` recommendations, or whether the user chose `.deb`, tar.gz, or AppImage.
- Inter remains the primary UI face; only unsupported glyphs use Noto Sans JP.
- Other CJK locales intentionally receive the Japanese glyph variant in Kagi's
  UI. A future locale-aware fallback list may add dedicated KR/SC/TC families
  if Kagi adds those interface languages.

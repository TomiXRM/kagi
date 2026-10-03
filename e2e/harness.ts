import { existsSync } from "node:fs";
import { join } from "node:path";

// The harness is the wasm-bindgen output of scripts/build-web.sh. Without it
// the static server answers every request with 404 and the run fails as
// "Timed out waiting 60000ms from config.webServer", which reads like a
// runtime hang (#516). This is checked while the config loads: Playwright
// waits for the webServer before it runs a globalSetup, so a check there
// would still sit behind the same timeout.
export const DIST = join(__dirname, "..", "crates", "kagi-web", "dist");
const REQUIRED = ["index.html", "kagi_web.js", "kagi_web_bg.wasm"];

export function assertHarnessBuilt(): void {
  const missing = REQUIRED.filter((file) => !existsSync(join(DIST, file)));
  if (missing.length > 0) {
    throw new Error(
      `kagi-web harness not built: ${missing.join(", ")} missing in ${DIST}.\n` +
        "Build it first, from the repository root: scripts/build-web.sh",
    );
  }
}

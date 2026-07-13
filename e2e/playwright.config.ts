import { defineConfig } from "@playwright/test";

// Serves the wasm-bindgen output built by scripts/build-web.sh.
// Build first: ../scripts/build-web.sh
export default defineConfig({
  testDir: "./tests",
  timeout: 60_000,
  retries: 0,
  use: {
    baseURL: "http://127.0.0.1:8899",
    viewport: { width: 1280, height: 800 },
    launchOptions: {
      // Headless Chromium ships no WebGPU adapter by default; SwiftShader
      // (software Vulkan) keeps the suite machine-independent.
      args: [
        "--enable-unsafe-webgpu",
        "--enable-features=Vulkan,UseSkiaRenderer",
        "--use-angle=swiftshader",
        "--ignore-gpu-blocklist",
      ],
    },
  },
  webServer: {
    command: "python3 -m http.server 8899 -d ../crates/kagi-web/dist",
    url: "http://127.0.0.1:8899",
    reuseExistingServer: true,
  },
});

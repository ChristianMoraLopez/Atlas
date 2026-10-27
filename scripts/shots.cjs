// Renders Atlas screens with mocked data for a visual check.
// Usage: npm run dev (in another terminal), then npm run shots.
const path = require("node:path");
const fs = require("node:fs");
const { chromium } = require("playwright");

const base = process.env.ATLAS_URL || "http://127.0.0.1:1420/";
const out = path.resolve(__dirname, "..", "screenshots");
const screens = [
  ["role", "?screen=role", false],
  ["workspace", "?screen=workspace", true],
  ["workspace-empty", "?screen=workspace&empty=1", true],
  ["workspace-en", "?screen=workspace&lang=en", false],
];

(async () => {
  fs.mkdirSync(out, { recursive: true });
  const browser = await chromium.launch();
  for (const [name, query, fullPage] of screens) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    page.on("pageerror", error => console.error(`${name}: ${error.message}`));
    await page.addInitScript({ path: path.join(__dirname, "tauri-mock.js") });
    await page.goto(base + query);
    await page.waitForTimeout(5000); // intro + entrance animations
    await page.screenshot({ path: path.join(out, `${name}.png`), fullPage });
    await page.close();
    console.log(`screenshots/${name}.png`);
  }
  await browser.close();
})();

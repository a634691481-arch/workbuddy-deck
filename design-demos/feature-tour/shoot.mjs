// 逐帧 seek 截图 → ffmpeg 双阶段 palette GIF（确定性，无 recordVideo 时序坑）
import { mkdirSync, rmSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { execSync } from "node:child_process";

const HERE = dirname(fileURLToPath(import.meta.url));
const FPS = 10, DURATION = 15.4;
const OUT_W = 900;
const FRAMES = join(HERE, "frames");

rmSync(FRAMES, { recursive: true, force: true });
mkdirSync(FRAMES, { recursive: true });

const { chromium } = await import(
  "file:///C:/Users/63469/AppData/Local/npm-cache/_npx/e41f203b7505f1fb/node_modules/playwright/index.mjs"
);
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1200, height: 836 }, deviceScaleFactor: 1 });
page.on("pageerror", (e) => console.error("PAGEERROR:", e.message));
await page.goto("file:///" + join(HERE, "feature-tour.html").replace(/\\/g, "/"), { waitUntil: "load" });
await page.waitForFunction(() => window.__ready === true, null, { timeout: 15000 });

const total = Math.round(DURATION * FPS);
for (let f = 0; f < total; f++) {
  await page.evaluate((t) => window.__seek(t), f / FPS);
  await page.screenshot({ path: join(FRAMES, `f${String(f).padStart(4, "0")}.png`) });
}
await browser.close();
console.log("frames:", total);

// GIF：两阶段 palette，bayer 抖动控体积（UI 截图大面积纯色）
const gif = join(HERE, "..", "..", "docs", "screenshots", "feature-tour.gif");
execSync(
  `ffmpeg -y -framerate ${FPS} -i "${FRAMES}/f%04d.png" -vf "scale=${OUT_W}:-1:flags=lanczos,palettegen=max_colors=128" "${HERE}/palette.png"`,
  { stdio: "inherit" }
);
execSync(
  `ffmpeg -y -framerate ${FPS} -i "${FRAMES}/f%04d.png" -i "${HERE}/palette.png" -lavfi "scale=${OUT_W}:-1:flags=lanczos [x];[x][1:v] paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle" -loop 0 "${gif}"`,
  { stdio: "inherit" }
);
console.log("gif written:", gif);

#!/usr/bin/env node
/**
 * 版本号三处同步：package.json / src-tauri/Cargo.toml / src-tauri/tauri.conf.json。
 *
 * 为什么必须同步：`tauri build` 读 tauri.conf.json 决定安装包版本，
 * 客户端比对读 Cargo.toml（env!("CARGO_PKG_VERSION")），
 * 前端 `<title>` 与 npm 包用 package.json。任一不同步 → 更新判断失真。
 *
 * 用法：
 *   npm run bump 0.1.1          # 只改文件
 *   npm run bump 0.1.1 --tag    # 改完顺带 commit + tag + push（CI 随即发版）
 */
import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const next = process.argv[2];
const withTag = process.argv.includes("--tag");

if (!next || !/^\d+\.\d+\.\d+$/.test(next)) {
  console.error("用法：npm run bump <major.minor.patch> [--tag]");
  process.exit(1);
}

const files = {
  pkg: join(root, "package.json"),
  cargo: join(root, "src-tauri", "Cargo.toml"),
  conf: join(root, "src-tauri", "tauri.conf.json"),
};

const before = {
  pkg: JSON.parse(readFileSync(files.pkg, "utf8")).version,
  cargo: readFileSync(files.cargo, "utf8").match(/^version = "([^"]+)"/m)?.[1],
  conf: JSON.parse(readFileSync(files.conf, "utf8")).version,
};

// 三处本来就不一致时先停下：说明有人手改过，盲目覆盖会把问题藏起来。
if (new Set([before.pkg, before.cargo, before.conf]).size !== 1) {
  console.error(`版本号已不一致：package=${before.pkg} cargo=${before.cargo} conf=${before.conf}`);
  console.error("先手动统一到同一版本，再执行 bump。");
  process.exit(1);
}

const pkg = JSON.parse(readFileSync(files.pkg, "utf8"));
pkg.version = next;
writeFileSync(files.pkg, `${JSON.stringify(pkg, null, 2)}\n`);

const cargo = readFileSync(files.cargo, "utf8").replace(/^version = "[^"]+"/m, `version = "${next}"`);
writeFileSync(files.cargo, cargo);

const conf = JSON.parse(readFileSync(files.conf, "utf8"));
conf.version = next;
writeFileSync(files.conf, `${JSON.stringify(conf, null, 2)}\n`);

console.log(`${before.pkg} → ${next}（package.json / Cargo.toml / tauri.conf.json 已同步）`);

if (!withTag) {
  console.log("下一步：git add -A && git commit -m \"chore: release v" + next + "\" && git tag v" + next);
  process.exit(0);
}

const run = (cmd) => execSync(cmd, { cwd: root, stdio: "inherit" });
run(`git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json src-tauri/Cargo.lock`);
run(`git commit -m "chore: release v${next}"`);
run(`git tag v${next}`);
run(`git push origin main --tags`);
console.log(`已推送 tag v${next}，CI 会构建并发布。`);

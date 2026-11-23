// 一键出 Inno Setup 安装向导包（WorkDaddy 同款分发形态）。
// 前置：cargo/tauri 已产出 target/release/workbuddy-task-assistant.exe
//（没有就先跑 `npm run tauri build -- --no-bundle`）。
// 用法：npm run installer
// 产物：src-tauri/target/release/bundle/innosetup/WorkBuddyDeck-Setup-<版本>.exe
import { readFileSync, existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const conf = JSON.parse(readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'));
const version = conf.version;
// 允许构建到独立目录（应用正在运行时 target/release 的 exe 被锁，无法覆盖）
const targetArg = process.argv.find((a) => a.startsWith('--target='));
const targetDir = targetArg ? targetArg.slice('--target='.length) : 'target/release';
const exe = path.join(root, 'src-tauri', targetDir, 'workbuddy-task-assistant.exe');

if (!existsSync(exe)) {
  console.error(`[installer] 缺少 ${exe}`);
  console.error('[installer] 先执行: npm run tauri build -- --no-bundle');
  process.exit(1);
}

// ISCC 定位：D 盘优先，回落 Program Files / LOCALAPPDATA
const isccCandidates = [
  'D:/Program Files/Inno Setup 6/ISCC.exe',
  'C:/Program Files/Inno Setup 6/ISCC.exe',
  'C:/Program Files (x86)/Inno Setup 6/ISCC.exe',
  path.join(process.env.LOCALAPPDATA ?? '', 'Programs/Inno Setup 6/ISCC.exe'),
];
const iscc = isccCandidates.find((p) => p && existsSync(p));
if (!iscc) {
  console.error('[installer] 找不到 ISCC.exe，请先安装 Inno Setup 6（winget install JRSoftware.InnoSetup）');
  process.exit(1);
}

const iss = path.join(root, 'src-tauri/installer/workbuddy-deck.iss');
console.log(`[installer] ISCC ${iscc}`);
console.log(`[installer] 版本 ${version}`);
const sep = String.fromCharCode(92);
const srcDefine = `/DSrcDir=..${sep}${targetDir.split('/').join(sep)}`;
const result = spawnSync(iscc, [`/DAppVersion=${version}`, srcDefine, iss], {
  cwd: path.dirname(iss),
  stdio: 'inherit',
  shell: false,
});
if (result.status !== 0) {
  console.error('[installer] 编译失败');
  process.exit(result.status ?? 1);
}
const out = path.join(root, 'src-tauri', targetDir, `bundle/innosetup/WorkBuddyDeck-Setup-${version}.exe`);
console.log(`[installer] 完成: ${out}`);

# 打包与发布方案（静态核对版，全部未落盘）

> 结论先行：exe 安装包这条路**技术上没有拦路虎**，只有一处只有用户本人能解的东西——签名私钥的密码。
> 本文件是待执行清单 + 建议 patch，Lead 说「开打」后按第 3/4/5 节落盘即可。

核对环境：`C:\Users\63469\Desktop\workbuddy-task-assistant`，分支 `357265b`，Tauri CLI **2.12.4**（package-lock 已锁定，CI 用同一个版本），`tauri` crate **2.12.5**。

---

## 1. 现状事实（逐条带证据）

| 项 | 事实 | 位置 |
| --- | --- | --- |
| 产品名 | `productName = "WorkBuddy Deck"`，版本 0.1.28 | `src-tauri/tauri.conf.json:3` |
| 窗口标题 | **仍是** `WorkBuddy 任务助手` | `src-tauri/tauri.conf.json:16` |
| 前端同名文案 | `index.html:6` `<title>`、`src/lib/i18n.ts:14` `app.title` | 不在我的写域 → 交前端 |
| 托盘 tooltip | `"WorkBuddy 任务助手"` | `src-tauri/src/lib.rs:134`，另有 panic 文案 `:244` → patch 交回 |
| 打包目标 | `bundle.targets = "all"` | Windows 上 = NSIS + **MSI**；MSI 需要 WiX |
| updater 产物 | `createUpdaterArtifacts = true` + `plugins.updater.pubkey` 已配 | 两者同时存在 → 无密钥时签名阶段**硬失败** |
| NSIS 工具链 | **已缓存** `%LOCALAPPDATA%\tauri\NSIS\makensis.exe` | 不需要联网下载 |
| WiX（MSI 用） | `%LOCALAPPDATA%\tauri` 下**只有 NSIS，没有 WiX** | 走 MSI 就要首次下载 |
| 签名密钥 | `%USERPROFILE%\.tauri\workbuddy-deck.key` / `.key.pub` 存在（2026-10-30） | 见下 |
| 密钥与配置是否配对 | `.key.pub` base64 解码 == `tauri.conf.json` 里 `plugins.updater.pubkey` 解码内容，**完全一致** | 本机自签的包，客户端会认 |
| 私钥形态 | 解出首行是 `untrusted comment: rsign encrypted secret key` | **加密私钥 → 需要密码**（密码只有用户/CI secrets 有） |
| icons | `32x32 / 128x128 / 128x128@2x / icon.ico / icon.icns / icon.png` 全在；`icon.ico` 含 16/24/32/48/64/**256** 六档、全部 32bpp；PNG 尺寸与文件名相符 | 过 NSIS 无问题，不用补 |
| release 产物 | `src-tauri/target/release/bundle/nsis/WorkBuddy Deck_0.1.28_x64-setup.exe`（6,311,080 B，**今天 13:42:15**），同目录**没有 `.sig`** | 见第 2 节第 1 条 |
| CI | `build.yml` / `release.yml` 都直接调 `npm run tauri build -- --bundles ...` + secrets；本次新增的 npm script **不参与 CI** | 见第 6 节 |

---

## 2. 本机现在 `npm run tauri build` 会卡在哪（按发生顺序）

1. **签名（唯一真正的硬失败）**：`createUpdaterArtifacts=true` 且配置里有 pubkey，但环境无 `TAURI_SIGNING_PRIVATE_KEY` 时，CLI 内置报错原文是
   `A public key has been found, but no private key. Make sure to set TAURI_SIGNING_PRIVATE_KEY environment variable.`
   注意**它发生在 installer 已生成之后**的签名步骤：所以会留下一个「看着能用但没有 `.sig`」的 setup.exe，而命令退出码非 0。今天 13:42 那个安装包就是这个形态（同秒生成、无 `.sig`）。
2. **`targets: "all"` 额外拖 MSI**：Windows 上会连 MSI 一起打，而 WiX 没缓存 → 首次要下载；且双 bundle 会做两次主二进制 patching，时间翻倍。本地出 exe 一律用 `--bundles nsis` 显式覆盖。
3. **`--no-sign` 是 2.12 才有的旗标**：本仓库锁 2.12.4，可用；将来降级到 2.1x 早期版本就得退回 `-c` 配置覆盖。
4. **别在 npm script 里内联 JSON**：Windows 的 npm 走 `cmd.exe`，`-c '{"bundle":{"createUpdaterArtifacts":false}}'` 里的单引号不是引号，会原样传给 CLI 导致解析失败。要么用 `--no-sign`，要么用独立的 `.json` 覆盖文件（`-c scripts/xxx.json` 走的是「JSON 文件与默认配置合并」语义，已由 `tauri build --help` 确认）。
5. **文件名带空格**：GitHub 上传 Release 资产时通常会把空格换成点（`WorkBuddy.Deck_0.1.28_x64-setup.exe`）。签名是对**内容**签名，不受影响；但脚本里拼 URL/路径别再假设原名。若某次 Release 里资产名保留了空格，把 `latest.json` 的 `url` 换回原名即可（唯一需要人看一眼的地方）。

---

## 3. 两条命令（建议新增到 `package.json` 的 `scripts`）

```json
"build:exe": "tauri build --bundles nsis --no-sign",
"build:release": "node scripts/build-release.mjs"
```

- `build:exe`：只出 NSIS 安装包，**不要密钥**。`--no-sign` 会让 updater 签名整段跳过（CLI 文案：`Updater signing is skipped due to --no-sign flag.`），`.sig`/`latest.json` 都不产。
- `build:release`：带密钥出 `.exe` + `.exe.sig`，并本地生成一份 `latest.json` 供自测（发布权威仍然是 CI）。
- 耗时（本机既有 `target/release`，增量）：
  前端 `vue-tsc + vite build` **30–50s** → Rust 侧无 src 改动时 relink **5–20s**（有改动再加 1–3 分钟）→ NSIS LZMA 打包 6MB **10–30s**。
  **合计约 1–2 分钟**；`rm -rf target` 后的冷编 10–20 分钟（rusqlite bundled + wb-switch-core + windows crate）。
- 产物：`src-tauri/target/release/bundle/nsis/WorkBuddy Deck_<version>_x64-setup.exe`

### `scripts/build-release.mjs`（待落盘，全文）

```js
#!/usr/bin/env node
/**
 * 本地「带签名的 NSIS 包」构建：tauri build --bundles nsis + 生成 latest.json。
 *
 * 为什么不直接 npm run tauri build：默认命令既要密钥、又会连带打 MSI（要 WiX），
 * 而且失败发生在 installer 已落盘之后，很容易误判「没打成」。
 * 发布权威是 release.yml + tauri-action，这里只服务本机自测与手工发包。
 */
import { execFileSync } from "node:child_process";
import { createInterface } from "node:readline/promises";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const bundleDir = join(root, "src-tauri", "target", "release", "bundle");
const nsisDir = join(bundleDir, "nsis");
const defaultKey = join(homedir(), ".tauri", "workbuddy-deck.key");
// 与 tauri.conf.json 的 plugins.updater.endpoints 保持一致
const REPO = "a634691481-arch/workbuddy-deck";

const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const version = pkg.version;

const key = process.env.TAURI_SIGNING_PRIVATE_KEY?.trim() ||
  (existsSync(defaultKey) ? defaultKey : "");
if (!key) {
  console.error(`找不到签名私钥。放到 ${defaultKey}，或先设 TAURI_SIGNING_PRIVATE_KEY=<私钥路径或 base64>`);
  process.exit(1);
}

let password = process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? "";
if (!password) {
  // 私钥是 rsign encrypted secret key，必须解密；输入内容会显示在终端，介意就预先设环境变量
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  password = await rl.question("签名私钥密码（输入会显示）：");
  rl.close();
}

const env = { ...process.env, TAURI_SIGNING_PRIVATE_KEY: key, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: password };
const cli = join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
console.log(`[build] v${version} → NSIS（带 updater 签名）`);
execFileSync(process.execPath, [cli, "build", "--bundles", "nsis"], { cwd: root, stdio: "inherit", env });

const installer = readdirSync(nsisDir).find((name) => name.endsWith("-setup.exe"));
if (!installer) {
  console.error(`没找到安装包，检查 ${nsisDir}`);
  process.exit(1);
}
const sigFile = `${installer}.sig`;
if (!existsSync(join(nsisDir, sigFile))) {
  console.error("缺 .sig：确认 tauri.conf.json 的 bundle.createUpdaterArtifacts 仍为 true，且密码正确");
  process.exit(1);
}

// GitHub 会把资产名里的空格换成点，latest.json 的 url 必须按点写
const assetUrl = `https://github.com/${REPO}/releases/download/v${version}/${installer.replace(/ /g, ".")}`;
const manifest = {
  version,
  notes: `WorkBuddy Deck v${version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature: readFileSync(join(nsisDir, sigFile), "utf8").trim(),
      url: assetUrl,
    },
  },
};
writeFileSync(join(bundleDir, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

console.log(`\n安装包：src-tauri/target/release/bundle/nsis/${installer}`);
console.log(`签名：  src-tauri/target/release/bundle/nsis/${sigFile}`);
console.log(`清单：  src-tauri/target/release/bundle/latest.json`);
console.log("国内镜像清单：python scripts/gen_cn_manifest.py latest.json latest-cn.json");
```

> 密码这一环是本轮唯一的真阻塞：私钥被 rsign 加密过，密码只有用户知道，仓库和 CI secrets 之外没有副本。

---

## 4. `src-tauri/tauri.conf.json` 建议 patch（未落盘）

```diff
@@ app.windows[0]
-        "title": "WorkBuddy 任务助手",
+        "title": "WorkBuddy Deck",

@@ bundle
     "createUpdaterArtifacts": true,
+    "windows": {
+      "nsis": {
+        "installMode": "currentUser",
+        "languages": ["SimpChinese", "English"],
+        "displayLanguageSelector": false
+      }
+    },
     "macOS": {
```

逐项理由：

- `installMode: "currentUser"`（schema 默认值）：装到用户目录、**免 UAC**；updater 的 `passive` 静默覆盖安装才不需要提权。装 `Program Files`（perMachine）会让每次自动更新都可能弹权限框，对「常驻托盘 + 静默更新」是反向优化。数据本来就在 `%USERPROFILE%\.wb-switch`，与安装模式无关。
- `languages: ["SimpChinese", "English"]`：默认只有 English。NSIS 按系统语言挑，中文系统直接出中文安装向导；第一项同时是非匹配时的兜底 → 放简体中文。
- `displayLanguageSelector: false`：不需要让用户手选语言。
- **故意不写 `startMenuFolder`**：写了会变成「开始菜单 → WorkBuddy Deck 文件夹 → WorkBuddy Deck 快捷方式」双层套娃；不写就是开始菜单里一条 `WorkBuddy Deck`。
- **故意不写 `compression` / `installerIcon`**：前者默认就是 `lzma`，后者默认取 `bundle.icon` 里的 `.ico`，写了等于多两处出错点。
- **`targets` 保持 `"all"` 不动**：CI 用 `--bundles` 显式覆盖，本地用 `--bundles nsis` 覆盖，改它只是给 mac/linux 本地打包添麻烦。
- `webviewInstallMode` 保持默认 `downloadBootstrapper`（安装时联网拉 WebView2 引导器；Win10+ 基本已内置）。想让安装包在完全离线机器上也能跑，可换 `{ "type": "embedBootstrapper", "silent": true }`：+1.8MB，但**打包机**要能访问微软 CDN。这条等用户真遇到「装不上」再开。

### 桌面/开始菜单快捷方式的真实行为（读 CLI 2.12.4 内嵌 NSIS 模板确认）

- 开始菜单：无条件创建 `$SMPROGRAMS\${PRODUCTNAME}.lnk` → 即 `WorkBuddy Deck`。
- 桌面：走完成页的复选框（`$(createDesktop)` = "Create desktop shortcut"，`MUI_FINISHPAGE_SHOWREADME` → `CreateOrUpdateDesktopShortcut`），模板**没有**定义 `MUI_FINISHPAGE_SHOWREADME_NOTCHECKED` → **默认就是勾上的**，点「完成」即生成 `$DESKTOP\WorkBuddy Deck.lnk`；静默 / passive 安装（含 updater 覆盖安装）**自动创建**，不受复选框影响。
- 同一张完成页还有默认勾选的「运行 WorkBuddy Deck」（`MUI_FINISHPAGE_RUN` → `RunMainBinary`）：装完点「完成」应用直接起来，托盘图标当场出现，不用再回桌面点一次。
- 想让桌面快捷方式「零交互必出」，唯一杠杆是 `bundle.windows.nsis.installerHooks` 指向一个 `.nsh`，在 `NSIS_HOOK_POSTINSTALL` 里直接 `CreateShortcut "$DESKTOP\WorkBuddy Deck.lnk" ...`。**建议先不做**：钩子写错是安装器级故障，而默认勾选已经够用。

---

## 5. 单实例方案（评估结论：该上，用官方插件）

**为什么必须上**：第二个实例不会「和平共处」——`deck_http::start` 会顺延到 `127.0.0.1:17654`（`src-tauri/src/deck_http.rs:66`），而面板 origin 就是 localStorage 作用域，端口一变主题/语言/选中账号全部清零；同时两个进程会抢 CDP 调试端口，注入的悬浮图标拿到的是另一个进程的桥。

**选型**：`tauri-plugin-single-instance 2.4.4`（官方 plugins-workspace）。已核实：

- 本机 cargo 缓存里**已有** `tauri-plugin-single-instance-2.4.4.crate`，sparse index 也有该条目；其依赖 `thiserror 2.0.21` / `tracing 0.1.44` / `windows-sys 0.60.2` 均在缓存里 → 加依赖后 `cargo check` 预期可离线解析。
- 它要求 `tauri ≥ 2.10`，本仓库 2.12.5 ✓；要求 Rust ≥ 1.77.2。
- Windows 实现（读源码确认）：`CreateMutexW("<identifier>-sim")` 互斥 + 隐藏窗口 `<identifier>-sic`；后来者 `FindWindowW` 命中后用 `WM_COPYDATA` 把 `cwd|argv` 发过去，随即 `app.cleanup_before_exit(); process::exit(0)`；回调在**主实例**里执行。
- 互斥名取自 `identifier`（`com.wbswitch.taskassistant`），与 productName 改名无关，不会因为改名而失效。
- README 硬要求：**必须注册在 plugin 链第一个**（"plugins run in the order they were added ... make sure that this plugin is registered first"）。注册在最前 → 它的 setup 先于 `app.setup` 执行，第二个实例在建托盘、起本地桥之前就退出，**连端口都不会碰**。

### 建议 patch A：`src-tauri/Cargo.toml`（+3 行，我有写域，等「开打」落盘）

```diff
 # 窗口大小/位置记忆：每次启动不用重摆
 tauri-plugin-window-state = "2"
+# 单实例：第二个实例只把已有窗口叫回前台然后退出。
+# 不拦的话它会抢 127.0.0.1:17653（面板 origin → localStorage 全丢）与 CDP 端口。
+tauri-plugin-single-instance = "2"
```

### 建议 patch B：`src-tauri/src/lib.rs`（**不在我写域，交 Lead 落**）

```diff
 pub fn run() {
     tauri::Builder::default()
+        // 必须第一个注册：插件按注册顺序执行，单实例要抢在其它插件之前把第二个实例拦下。
+        // 回调在主实例里跑，用户看到的就是「再点一次图标 → 已有窗口到前台」。
+        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
+            show_main(app);
+        }))
         .plugin(tauri_plugin_dialog::init())
```

配套顺手改的两行同名文案（同一个文件，建议一起提）：

```diff
-                .tooltip("WorkBuddy 任务助手")
+                .tooltip("WorkBuddy Deck")
-        .expect("启动 WorkBuddy 任务助手失败");
+        .expect("启动 WorkBuddy Deck 失败");
```

`show_main(&AppHandle)` 定义在 `lib.rs:26`，与被改的 `run()` 同模块，patch 不需要新增 import。

**代价**：+1 个 crate（编译十几秒）；Linux CI 会带进 zbus 及其依赖树（体积/时间↑，`swatinem/rust-cache` 能覆盖），macOS 无额外负担。

**备选（不推荐）**：不加依赖，改成「探测 17653 被占就退出」。要动 `deck_http.rs`/`lib.rs`，判定不如命名互斥可靠，还得处理端口顺延后的假阴性。

---

## 6. `.github/workflows/release.yml` 结论：**不需要改**

理由（逐条核过）：

- Windows matrix 已是 `bundles: nsis`，`tauri-action` 走 `args: --bundles nsis`；新增的两条 npm script 不参与 CI，CI 仍用 `npm run tauri build`（`build.yml`）与 tauri-action（`release.yml`），密钥从 secrets 拿，签名链路不变。
- 笔记链（`scripts/gen_release_notes.py` → `releaseBody`）、`includeUpdaterJson: true`、`verify` job（curl `latest.json`）、`gen_cn_manifest.py` 生成 `latest-cn.json` 全部与本次改动无关。
- 免安装 zip 步骤引用的 `src-tauri/target/release/workbuddy-task-assistant.exe` 仍有效（二进制名由 Cargo bin 名决定，`productName` 改名不动它）。
- 可选增强（纯粹锦上添花，不做也不影响）：给 portable zip 步骤加 `hashFiles()` 守卫，避免「二进制存在但 zip 步骤因路径变动静默失败」。

---

## 7. 前置缺口：**开机自启根本没实现**

全仓库 grep `autostart` / `AutoStart` / `HKCU\...\Run` **0 命中**；`Cargo.toml` 里没有 `tauri-plugin-autostart`，`lib.rs` 也没有注册任何自启逻辑。

所以现在的安装包能保证的是：装完 → 桌面/开始菜单有 `WorkBuddy Deck` → 双击启动 → 托盘图标常驻 → 关窗不退出（`lib.rs:222` `CloseRequested` → `hide`）→ 托盘左键唤回。
**它保证不了「开机自动起、右下角图标自己出来」**。要做需要三处：`Cargo.toml` 加 `tauri-plugin-autostart`、`lib.rs` 注册（+ 一个 enable/disable 命令）、设置页一个开关（`src/` 不在我的写域）。这条建议回给用户拍板：是「手动启动一次后常驻」够用，还是必须开机自启。

---

## 8. 待用户验证清单（链路跑通后再执行）

**提前可做的一件事**：盘上那个 `WorkBuddy Deck_0.1.28_x64-setup.exe`（13:42，6.3MB）可以**现在就双击试装**，用来先看「桌面/开始菜单快捷方式 + 托盘图标 + 关窗不退出」这条链路；它里面的前端是 13:40 的 dist，不含 deck-ui 正在改的面板，也没有 `.sig`（不能当更新基线）。不想装脏系统就跳过这条，等 `build:exe` 重打。

1. `npm run build:exe` → 结束后确认 `src-tauri/target/release/bundle/nsis/WorkBuddy Deck_0.1.28_x64-setup.exe` 时间戳是刚才；命令**退出码 0**（有 `Finished` / `Built application` 字样）。
2. 双击安装包：向导是中文；安装位置在用户目录、**不弹 UAC**；完成页「Create desktop shortcut」默认勾选 → 桌面出现 `WorkBuddy Deck`；开始菜单出现 `WorkBuddy Deck`。
3. 首次启动：右下角托盘出现图标（tooltip 应是 `WorkBuddy Deck` + 读数）；WorkBuddy 页面右下角出现悬浮图标，点开能出面板。**注意 WorkBuddy 客户端要先开着调试端口**，否则守护进程会尝试带参拉起（冷却 60s）。
4. 关窗 → 进程留在托盘；托盘左键 → 窗口回来；托盘菜单「退出程序」才真退。
5. 单实例（patch 落地后）：连点两次桌面图标 → 不出现第二个托盘图标、`netstat -ano | findstr 17653` 只有一行 LISTENING、面板主题/语言不重置。
6. 卸载：控制面板卸载后 `%USERPROFILE%\.wb-switch` 数据**仍在**（卸载不删数据是既定口径）。

---

## 9. 这一轮我动了什么

只新增本文件 `scripts/packaging-plan.md`。
`tauri.conf.json` / `Cargo.toml` / `package.json` / `release.yml` **一个字没改**（等「链路通了，开打」），也没跑任何 `cargo check` / `cargo build` / `tauri build`，没碰 WorkBuddy 进程。

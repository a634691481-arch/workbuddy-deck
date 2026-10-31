// WorkBuddy Deck — README 功能导览 GIF 源
// 设计语言 = 产品自身的 Tactical Telemetry / CRT 深色（0 圆角 / 航空红 #e61919 / 等宽读数）
// 素材 = docs/screenshots 的 6 张真实界面截图（1680×1170），字体 = 产品同款 Archivo Black + JetBrains Mono
// 时间轴（16.4s 循环）：片头 1.8s → 6 屏 × 2.1s（扫描线 wipe 转场 + Ken Burns + 副标打字机）→ 结尾 2.0s
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..");

const b64 = (p) => readFileSync(p).toString("base64");
const fontArchivo = b64(join(ROOT, "dist/assets/archivo-black-latin-400-normal-BTVu2TQR.woff2"));
const fontMono = b64(join(ROOT, "dist/assets/jetbrains-mono-latin-wght-normal-B9CIFXIH.woff2"));
const IMG = (name) =>
  `data:image/png;base64,${b64(join(ROOT, "docs/screenshots", name))}`;

const SCREENS = [
  { n: "01", zh: "账号管理", en: "ACCOUNTS", img: IMG("01-accounts.png"), sub: "CARD GRID · BALANCE · 1-CLICK SWITCH" },
  { n: "02", zh: "任务中心", en: "TASKS", img: IMG("02-tasks.png"), sub: "CHECK-IN · GROWTH · TRAVEL · LOGS" },
  { n: "03", zh: "积分统计", en: "CREDITS", img: IMG("03-credits.png"), sub: "DAILY BURN · MODEL MIX · LEDGER" },
  { n: "04", zh: "TOKEN 统计", en: "TOKENS", img: IMG("04-tokens.png"), sub: "JSONL SCAN · BARS · MODEL MIX · HEAT" },
  { n: "05", zh: "运行日志", en: "LOGS", img: IMG("05-logs.png"), sub: "4 SOURCES · SEARCH · ERRORS" },
  { n: "06", zh: "系统设置", en: "SETTINGS", img: IMG("06-settings.png"), sub: "UPDATER · PURGE · CSV · AUTO-SWITCH" },
];

const W = 1200, H = 836, BAND = 60;
const INTRO_D = 1.6, SCREEN_D = 2.0, OUTRO_D = 1.8, WIPE = 0.32;
const TOTAL = INTRO_D + SCREENS.length * SCREEN_D + OUTRO_D;

const screenEls = SCREENS.map(
  (s, i) => `
  <div class="phase screen" id="ph-s${i}" style="z-index:1">
    <img class="shot" id="shot-${i}" src="${s.img}" alt="">
    <div class="band">
      <span class="num">${s.n}</span>
      <span class="sep"></span>
      <span class="zh">${s.zh}</span>
      <span class="en">${s.en}</span>
      <span class="sub readout" id="sub-${i}"></span>
    </div>
  </div>`
).join("");

const introCells = SCREENS.map(
  (s, i) => `
    <div class="cell" id="cell-${i}"><span class="cn">${s.n}</span>${s.zh}<span class="ce">${s.en}</span></div>`
).join("");

const html = `<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="utf-8">
<title>WorkBuddy Deck — Feature Tour</title>
<style>
  @font-face { font-family:"Archivo Black"; src:url(data:font/woff2;base64,${fontArchivo}) format("woff2"); font-weight:400; }
  @font-face { font-family:"JetBrains Mono"; src:url(data:font/woff2;base64,${fontMono}) format("woff2"); font-weight:100 800; }
  * { margin:0; padding:0; box-sizing:border-box; border-radius:0 !important; }
  html,body { background:#0a0a0a; }
  body { width:${W}px; height:${H}px; overflow:hidden; }
  #stage { position:relative; width:${W}px; height:${H}px; background:#0a0a0a; font-family:"JetBrains Mono",Consolas,monospace; }
  /* CRT 扫描线纹理：主题签名，极淡 */
  #scan { display:none; }

  .phase { position:absolute; inset:0; background:#0a0a0a; will-change:clip-path; }
  .phase.screen { background:#060606; }
  .shot { position:absolute; left:0; top:${BAND}px; width:${W}px; height:${H - BAND}px; object-fit:cover; object-position:top; }

  /* 顶部标题带 */
  .band { position:absolute; left:0; top:0; width:${W}px; height:${BAND}px; background:#080808;
    border-bottom:1px solid #e61919; display:flex; align-items:center; gap:14px; padding:0 20px; }
  .num { font-family:"Archivo Black"; font-size:30px; color:#e61919; letter-spacing:.02em; }
  .sep { width:1px; height:28px; background:#454540; }
  .zh { font-family:"Microsoft YaHei","PingFang SC",sans-serif; font-size:21px; font-weight:700; color:#eaeaea; letter-spacing:.08em; }
  .en { font-size:11px; color:#8a8a85; letter-spacing:.3em; transform:translateY(3px); }
  .sub { margin-left:auto; max-width:520px; overflow:hidden; font-size:9.5px; color:#8a8a85; letter-spacing:.1em; white-space:pre; }
  .sub .caret { color:#e61919; }

  /* 片头 / 结尾 */
  #ph-intro,#ph-outro { display:flex; flex-direction:column; align-items:center; justify-content:center; }
  .kicker { font-size:11px; color:#8a8a85; letter-spacing:.5em; margin-bottom:18px; }
  .brand { font-family:"Archivo Black"; font-size:64px; color:#eaeaea; letter-spacing:.01em; line-height:1; }
  .brand em { font-style:normal; color:#e61919; }
  .brand-rule { width:220px; height:2px; background:#e61919; margin:22px 0 34px; }
  .cells { display:grid; grid-template-columns:repeat(3,220px); gap:10px; }
  .cell { border:1px solid #2a2a28; color:#565651; padding:12px 14px; font-size:13px; letter-spacing:.1em;
    font-family:"Microsoft YaHei","PingFang SC",sans-serif; display:flex; align-items:baseline; gap:8px; }
  .cell .cn { font-family:"JetBrains Mono",monospace; color:inherit; }
  .cell .ce { margin-left:auto; font-family:"JetBrains Mono",monospace; font-size:9px; letter-spacing:.2em; }
  .cell.lit { border-color:#e61919; color:#eaeaea; }
  .cell.lit .cn, .cell.lit .ce { color:#e61919; }
  .foot { margin-top:36px; font-size:11px; color:#565651; letter-spacing:.4em; }
  #ph-outro .repo { margin-top:16px; font-size:13px; color:#eaeaea; letter-spacing:.18em; border:1px solid #454540; padding:10px 22px; }
  #ph-outro .tags { margin-top:26px; font-size:11px; color:#8a8a85; letter-spacing:.35em; }

  /* 扫描线转场光标 */
  #wipe { position:absolute; top:0; width:3px; height:${H}px; background:#e61919; z-index:50; display:none; }
</style>
</head>
<body>
<div id="stage">
  ${screenEls}
  <div class="phase" id="ph-intro" style="z-index:1">
    <div class="kicker" id="in-kicker">// TASK TELEMETRY · MULTI-ACCOUNT CONSOLE</div>
    <div class="brand" id="in-brand">WORKBUDDY <em>DECK</em></div>
    <div class="brand-rule" id="in-rule"></div>
    <div class="cells">${introCells}</div>
    <div class="foot">TAURI 2 · VUE 3 · WINDOWS / MACOS / LINUX</div>
  </div>
  <div class="phase" id="ph-outro" style="z-index:1">
    <div class="brand">WORKBUDDY <em>DECK</em></div>
    <div class="brand-rule"></div>
    <div class="repo">GITHUB.COM/A634691481-ARCH/WORKBUDDY-DECK</div>
    <div class="tags">AUTO UPDATE · CROSS-PLATFORM · OPEN SOURCE</div>
  </div>
  <div id="wipe"></div>
  <div id="scan"></div>
</div>
<script>
// ── 纯 render(t)：任意 t 一次求值出唯一 DOM 状态（可逐帧 seek 录制）──
const W=${W}, H=${H}, BAND=${BAND};
const INTRO_D=${INTRO_D}, SCREEN_D=${SCREEN_D}, OUTRO_D=${OUTRO_D}, WIPE=${WIPE};
const TOTAL=${TOTAL};
const SCREENS=${JSON.stringify(SCREENS)};
const PHASES = [
  { id:"ph-intro", start:0, dur:INTRO_D },
  ...SCREENS.map((s,i)=>({ id:"ph-s"+i, start:INTRO_D+i*SCREEN_D, dur:SCREEN_D, screen:i })),
  { id:"ph-outro", start:INTRO_D+SCREENS.length*SCREEN_D, dur:OUTRO_D },
];
const $ = (id)=>document.getElementById(id);
const phaseEls = PHASES.map(p=>$(p.id));
const easeOut = (x)=>1-Math.pow(1-x,3);

function render(t){
  // 定位当前 phase（循环由录制端处理；HTML 内 t>=TOTAL 时夹到末态）
  let idx = PHASES.findIndex(p=>t < p.start + p.dur);
  if (idx < 0) idx = PHASES.length - 1;
  const ph = PHASES[idx];
  const local = Math.min(Math.max(t - ph.start, 0), ph.dur);

  // 只让「当前 phase」与其「上一个 phase」可见：其余 display:none
  // （否则 DOM 靠后的 outro 会垫在所有屏幕之上，转场时露出来）
  const prevIdx = idx === 0 ? -1 : idx - 1;
  phaseEls.forEach((el,i)=>{
    if (i===idx){
      el.style.display = "";
      el.style.zIndex = 10;
      const shown = easeOut(Math.min(local/WIPE, 1)); // ease-out 扫入
      el.style.clipPath = idx===0 ? "none" : "inset(0 " + (100-shown*100).toFixed(3) + "% 0 0)";
    } else if (i===prevIdx){
      el.style.display = "";
      el.style.zIndex = 5;
      el.style.clipPath = "none";
    } else {
      el.style.display = "none";
      el.style.clipPath = "none";
    }
  });

  // 扫描线光标：仅 wipe 期间出现（片头不扫，保证第 0 帧就是完整初始画面）
  const wiping = idx > 0 && local < WIPE;
  const wipeEl = $("wipe");
  wipeEl.style.display = wiping ? "block" : "none";
  if (wiping) wipeEl.style.left = (easeOut(local/WIPE)*W).toFixed(1)+"px";

  // 屏幕内容：Ken Burns + 打字机
  if (ph.screen !== undefined){
    const i = ph.screen;
    const sub = SCREENS[i].sub;
    const n = Math.min(sub.length, Math.floor(Math.max(local-0.1,0)/0.55*sub.length));
    const caretOn = Math.floor(t*2.5)%2===0;
    $("sub-"+i).innerHTML = "";
    $("sub-"+i).textContent = sub.slice(0,n);
    if (n < sub.length || caretOn){
      const c = document.createElement("span");
      c.className="caret"; c.textContent="▌";
      $("sub-"+i).appendChild(c);
    }
  }

  // 片头：大字入场 + 功能格逐个点亮
  if (ph.id==="ph-intro"){
    $("in-brand").style.opacity = "1";
    $("in-kicker").style.opacity = "1";
    $("in-rule").style.transform = "scaleX("+Math.min(local/0.5,1).toFixed(3)+")";
    $("in-rule").style.transformOrigin = "left";
    SCREENS.forEach((s,i)=>{
      $("cell-"+i).className = "cell" + (local >= 0.9+i*0.12 ? " lit" : "");
    });
  }
}

// ── starter tick 模板（fonts.ready 后启动；首帧 pair __ready 与 t=0）──
let time=0, playing=false, lastTick=null;
function tick(now){
  if (lastTick===null){ lastTick=now; window.__ready=true; render(0); requestAnimationFrame(tick); return; }
  const dt=(now-lastTick)/1000; lastTick=now;
  if (playing && !window.__hold){
    let t=time+dt;
    if (t>=TOTAL){ t = window.__recording ? TOTAL-0.001 : 0; }
    time=t; render(time);
  }
  requestAnimationFrame(tick);
}
document.fonts.ready.then(()=>{ render(0); playing=true; requestAnimationFrame(tick); });
// 逐帧录制用：seek 即冻结时钟（否则下一 rAF 会推进并覆盖截图）
window.__seek = (t)=>{ window.__hold=true; time=t; render(t); };
window.__play = ()=>{ window.__hold=false; lastTick=null; };
</script>
</body>
</html>`;

writeFileSync(join(HERE, "feature-tour.html"), html);
console.log("written feature-tour.html,", (html.length / 1024 / 1024).toFixed(2) + "MB", "total", TOTAL + "s");

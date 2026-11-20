#!/usr/bin/env node
/**
 * headless 启动器 = 启动器 + 注入器，不要桌面窗口。
 *
 * 干四件事：
 *   0. 预检并回收端口（1420 / 17653-17656）——上次没退干净的 vite 或 headless 进程
 *      会让 tauri dev 直接死在 `Port 1420 is already in use`，这里先替它清场
 *   1. vite dev（1420）→ 面板前端 HMR，改完即生效
 *   2. tauri dev → debug 编译，Rust 改动时自动重编译重启
 *   3. 应用进程带 WB_DECK_HEADLESS=1 → 不建窗口/托盘，只跑本地桥（17653）+ 注入
 *
 * 用法：
 *   npm run deck          只起桥 + 注入守护（客户端没开 9222 时**不动进程**）
 *   npm run deck:inject   同上，并允许带 --remote-debugging-port 重启客户端
 *                         （客户端没开调试端口时才会重启一次）
 *
 * 为什么用环境变量而不是命令行参数：`tauri dev -- xxx` 里的 xxx 是传给 **cargo**
 * 的（实测会拼成 `cargo run --headless --no-default-features …` 直接报错），
 * 传不到应用自己的 `std::env::args()`。
 */
import { execFileSync, spawn } from "node:child_process";
import net from "node:net";

const inject = process.argv.includes("--inject");

/** 只回收自己人的进程，别误杀别人的服务 */
const PORTS = [1420, 17653, 17654, 17655, 17656];
const OWNERS = new Set(["node.exe", "workbuddy-task-assistant.exe"]);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function busy(port) {
  return new Promise((resolve) => {
    const sock = net.connect({ host: "127.0.0.1", port });
    const done = (v) => {
      sock.destroy();
      resolve(v);
    };
    sock.setTimeout(400);
    sock.once("connect", () => done(true));
    sock.once("timeout", () => done(false));
    sock.once("error", () => done(false));
  });
}

function listenerPids(port) {
  try {
    // netstat 输出本地化（LISTENING / ABHÖREN），只认冒号端口 + 最后一列 PID
    const out = execFileSync("netstat", ["-ano", "-p", "TCP"], { encoding: "utf8" });
    const pids = new Set();
    for (const line of out.split(/\r?\n/)) {
      if (!line.includes(`:${port} `) || !/LISTEN/i.test(line)) continue;
      const m = line.trim().match(/(\d+)\s*$/);
      if (m) pids.add(m[1]);
    }
    return [...pids];
  } catch {
    return [];
  }
}

function procName(pid) {
  try {
    const out = execFileSync("tasklist", ["/FI", `PID eq ${pid}`, "/FO", "CSV", "/NH"], {
      encoding: "utf8",
    });
    const m = out.match(/^"([^"]+)"/m);
    return m ? m[1].toLowerCase() : "";
  } catch {
    return "";
  }
}

async function reclaim() {
  for (const port of PORTS) {
    if (!(await busy(port))) continue;
    const pids = listenerPids(port);
    let killed = 0;
    for (const pid of pids) {
      const name = procName(pid);
      if (!OWNERS.has(name)) {
        console.warn(`[deck] ${port} 被 ${name || "未知进程"}(PID ${pid}) 占用，非本项目进程，跳过`);
        continue;
      }
      try {
        execFileSync("taskkill", ["/F", "/PID", pid], { stdio: "ignore" });
        console.log(`[deck] 回收端口 ${port}：结束残留 ${name}(PID ${pid})`);
        killed += 1;
      } catch {
        console.warn(`[deck] 结束 PID ${pid} 失败（可能已退出）`);
      }
    }
    if (killed) {
      for (let i = 0; i < 20; i += 1) {
        if (!(await busy(port))) break;
        await sleep(150);
      }
    }
    if (await busy(port)) {
      console.error(`[deck] 端口 ${port} 仍被占用，请手动处理后重试`);
      process.exit(1);
    }
  }
}

await reclaim();

const env = { ...process.env, WB_DECK_HEADLESS: "1" };
if (inject) env.WB_DECK_CDP_AUTOSTART = "1";

const child = spawn("npx tauri dev", { stdio: "inherit", shell: true, env });

const shutdown = (signal) => {
  child.kill(signal);
  process.exit(0);
};
process.on("SIGINT", () => shutdown("SIGINT"));
process.on("SIGTERM", () => shutdown("SIGTERM"));
child.on("exit", (code) => process.exit(code ?? 0));

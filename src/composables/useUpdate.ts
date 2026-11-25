/**
 * 自动更新编排（对齐 workbuddy-switch 的流程）：
 * 检查静默 → 发现新版只提示 → 用户点「下载更新」→ 下载完暂存 →
 * 用户点「重启以完成升级」→ 安装并重启。绝不下载即装。
 *
 * 状态放在模块级：标题栏的更新角标与设置页的更新卡读同一份结果，
 * 后台轮询（`update://available`）与手动检查写入同一处，不各存一份。
 */
import { computed, ref } from "vue";
import { listen } from "@tauri-apps/api/event";

import {
  updateCheck,
  updateConfigGet,
  updateConfigSet,
  updateDownload,
  updateRestart,
  type UpdateDone,
  type UpdateProgress,
  type UpdateStatus,
} from "@/lib/api";
import { isTauri } from "@/lib/tauri-env";

/**
 * 嵌入面板里不自动开更新弹框：面板是 WorkBuddy 页面内的 iframe，
 * 弹框挡住的是宿主页面的一角，用户是被点图标叫出来的，不该先看一堆版本说明。
 * 读数照旧进 status（设置页能看），只是不抢焦点。
 * 下载/安装也不在面板里做：它会重启本进程，面板自己是挂在宿主页面上的。
 */
function canPromptUpdate(): boolean {
  // 2026-11-25 用户要求：面板里也要弹更新框，下载/安装都在面板内完成。
  // 之前面板里静默，只是为了不挡宿主页面，代价是「有更新也不知道」。
  return true;
}

const status = ref<UpdateStatus | null>(null);
const checking = ref(false);
const installing = ref(false);
const downloaded = ref(0);
const total = ref<number | null>(null);
const message = ref("");
const autoCheck = ref(true);
const mirror = ref("");

/**
 * 更新弹框的阶段（对齐 workbuddy-switch 状态机）：
 * available（发现新版，等用户点下载）→ downloading → ready（已暂存，等重启）→ restarting。
 * available / failed / idle 允许关闭。
 */
export type UpdatePhase =
  | "idle"
  | "available"
  | "downloading"
  | "ready"
  | "restarting"
  | "failed";
const phase = ref<UpdatePhase>("idle");
const dialogOpen = ref(false);

let wired = false;
let unlisten: Array<() => void> = [];

/** 订阅后端事件（只做一次）：轮询、手动检查、下载进度都从这四条进来。 */
function wire() {
  if (wired || !isTauri()) return;
  wired = true;
  const bind = async (event: string, handler: (payload: unknown) => void) => {
    unlisten.push(
      await listen(event, (e) => {
        handler(e.payload);
      }),
    );
  };
  void bind("update://available", (payload) => {
    status.value = payload as UpdateStatus;
    // 发现新版只弹框提示，下载交给用户（对齐 workbuddy-switch 的 Available 态）
    phase.value = "available";
    dialogOpen.value = canPromptUpdate();
  });
  void bind("update://checked", (payload) => {
    status.value = payload as UpdateStatus;
    const next = status.value;
    message.value = next?.ok === false ? (next.error ?? "") : "";
  });
  void bind("update://progress", (payload) => {
    const p = payload as UpdateProgress;
    downloaded.value = p.downloaded ?? 0;
    total.value = p.total ?? null;
  });
  void bind("update://done", (payload) => {
    const done = payload as UpdateDone & { staged?: boolean };
    installing.value = false;
    if (!done.ok) {
      // 下载失败回 available 可重试；重启安装失败包还在，也回 ready
      phase.value = phase.value === "restarting" ? "ready" : "available";
      message.value = done.error ?? "";
      return;
    }
    if (done.staged === true) {
      // 下载完成只暂存：等用户点「重启以完成升级」
      phase.value = "ready";
      return;
    }
    // Windows 上安装器启动后会结束本进程；弹框保持到进程退出
    phase.value = "restarting";
  });
}

let bootstrapped = false;

export function useUpdate() {
  wire();
  // 首次启动 / 页面刷新各跑一次：读配置 → 立即查一次 → 有新版本就静默装
  if (!bootstrapped && isTauri()) {
    bootstrapped = true;
    void (async () => {
      await loadConfig();
      // force = true：页面刷新也要真的查一次，不能吃 6 小时缓存
      await check(true);
    })();
  }

  const percent = computed(() => {
    if (!total.value || total.value <= 0) return null;
    return Math.min(100, Math.round((downloaded.value / total.value) * 100));
  });

  const current = computed(() => status.value?.current ?? "");
  const hasUpdate = computed(() => status.value?.hasUpdate === true);
  const latest = computed(() => status.value?.latest ?? "");
  /** 更新说明：清单里的 notes；老清单可能没有，弹框里给兜底文案 */
  const notes = computed(() => (status.value?.notes ?? "").trim());

  async function loadConfig() {
    if (!isTauri()) return;
    try {
      const cfg = await updateConfigGet();
      autoCheck.value = cfg.autoCheck;
      mirror.value = cfg.mirror;
      if (!status.value) {
        status.value = { ok: true, current: cfg.current, autoCheck: cfg.autoCheck, mirror: cfg.mirror };
      }
    } catch {
      // 读配置失败不影响手动检查
    }
  }

  async function check(force = true) {
    // 先自己扫一遍，不再等后端 8s 的首检
    if (checking.value || !isTauri()) return;
    checking.value = true;
    message.value = "";
    try {
      const result = await updateCheck(force);
      status.value = result;
      if (!result.ok) message.value = result.error ?? "";
    } catch (cause) {
      message.value = cause instanceof Error ? cause.message : String(cause);
    } finally {
      checking.value = false;
      if (status.value?.hasUpdate === true) {
        if (phase.value === "idle") phase.value = "available";
        // 已下载待重启时也把弹框叫回来，避免用户点开设置却看不到「重启安装」。
        dialogOpen.value = canPromptUpdate();
      }
    }
  }

  /** 用户点「下载更新」：只下载不装，完成后停在 ready 等重启确认。 */
  async function download() {
    if (installing.value || !isTauri() || !canPromptUpdate()) return;
    installing.value = true;
    downloaded.value = 0;
    total.value = null;
    message.value = "";
    phase.value = "downloading";
    try {
      const result = await updateDownload();
      if (!result.ok) {
        installing.value = false;
        phase.value = "available";
        message.value = result.error ?? "";
      }
    } catch (cause) {
      installing.value = false;
      phase.value = "available";
      message.value = cause instanceof Error ? cause.message : String(cause);
    }
  }

  /** 用户点「重启以完成升级」：安装已暂存的包并重启。 */
  async function restart() {
    if (installing.value || !isTauri() || !canPromptUpdate()) return;
    installing.value = true;
    message.value = "";
    phase.value = "restarting";
    try {
      const result = await updateRestart();
      if (!result.ok) {
        installing.value = false;
        phase.value = "ready";
        message.value = result.error ?? "";
      }
    } catch (cause) {
      installing.value = false;
      phase.value = "ready";
      message.value = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function saveConfig(nextAuto: boolean, nextMirror: string) {
    if (!isTauri()) return;
    const result = await updateConfigSet(nextAuto, nextMirror);
    if (!result.ok) {
      message.value = result.error ?? "";
      return;
    }
    autoCheck.value = result.autoCheck;
    mirror.value = result.mirror;
  }

  /** available（稍后再说）与失败态允许关掉弹框；下载中/待重启关掉就看不到进度了。 */
  function dismiss() {
    if (phase.value === "available" || phase.value === "failed" || phase.value === "idle") {
      dialogOpen.value = false;
    }
  }

  return {
    status,
    checking,
    installing,
    downloaded,
    total,
    percent,
    message,
    autoCheck,
    mirror,
    current,
    hasUpdate,
    latest,
    notes,
    phase,
    dialogOpen,
    dismiss,
    loadConfig,
    check,
    download,
    restart,
    saveConfig,
  };
}

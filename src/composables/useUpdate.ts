/**
 * 自动更新编排：检查 / 下载安装 / 进度 / 设置。
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
  updateInstall,
  type UpdateDone,
  type UpdateProgress,
  type UpdateStatus,
} from "@/lib/api";
import { isTauri } from "@/lib/tauri-env";
import { useShell } from "@/composables/useShell";

const status = ref<UpdateStatus | null>(null);
const checking = ref(false);
const installing = ref(false);
const downloaded = ref(0);
const total = ref<number | null>(null);
const message = ref("");
const autoCheck = ref(true);
const mirror = ref("");

/**
 * 静默更新：默认开启。检测到新版本后自己下载并安装，不弹任何提示；
 * 唯一让步是「批量任务正在跑」时等它空闲，避免把用户正在跑的批次直接掐掉。
 */
const SILENT_KEY = "wb-ta/silentUpdate";
const silent = ref(localStorage.getItem(SILENT_KEY) !== "off");
/** 同一个版本只自动装一次，装失败不无限重试（避免坏包把应用卡在重启循环） */
const autoTried = new Set<string>();
/** 真正的安装函数由 useUpdate() 注入：事件回调在模块层，拿不到组件闭包 */
let autoInstaller: (() => void) | null = null;

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
    maybeAutoInstall();
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
    const done = payload as UpdateDone;
    installing.value = false;
    if (!done.ok) message.value = done.error ?? "";
  });
}

/** 检测到新版本后静默安装：任务执行中就 15 秒后再看一次，空闲立刻装。 */
function maybeAutoInstall() {
  if (!isTauri() || !silent.value || installing.value) return;
  const next = status.value;
  if (next?.hasUpdate !== true || !next.latest) return;
  if (autoTried.has(next.latest)) return;
  if (useShell().running.value) {
    window.setTimeout(maybeAutoInstall, 15_000);
    return;
  }
  autoTried.add(next.latest);
  autoInstaller?.();
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
      maybeAutoInstall();
    })();
  }

  const percent = computed(() => {
    if (!total.value || total.value <= 0) return null;
    return Math.min(100, Math.round((downloaded.value / total.value) * 100));
  });

  const current = computed(() => status.value?.current ?? "");
  const hasUpdate = computed(() => status.value?.hasUpdate === true);

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
      maybeAutoInstall();
    }
  }

  async function install() {
    if (installing.value || !isTauri()) return;
    installing.value = true;
    downloaded.value = 0;
    total.value = null;
    message.value = "";
    try {
      const result = await updateInstall();
      if (!result.ok) {
        installing.value = false;
        message.value = result.error ?? "";
      }
    } catch (cause) {
      installing.value = false;
      message.value = cause instanceof Error ? cause.message : String(cause);
    }
  }

  autoInstaller = install;

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

  function setSilent(on: boolean) {
    silent.value = on;
    localStorage.setItem(SILENT_KEY, on ? "on" : "off");
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
    silent,
    loadConfig,
    check,
    install,
    saveConfig,
    setSilent,
  };
}

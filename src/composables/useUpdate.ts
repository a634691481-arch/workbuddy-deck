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

const status = ref<UpdateStatus | null>(null);
const checking = ref(false);
const installing = ref(false);
const downloaded = ref(0);
const total = ref<number | null>(null);
const message = ref("");
const autoCheck = ref(true);
const mirror = ref("");

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

export function useUpdate() {
  wire();

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
    loadConfig,
    check,
    install,
    saveConfig,
  };
}

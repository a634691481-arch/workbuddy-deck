// 浏览器登录：发起 → 打开授权页 → 轮询结果。成功由后端直接写入账号库。
// 独立成 composable 的理由：轮询有独立生命周期（需可取消），不该和账号加载混在一起。
import { onBeforeUnmount, ref } from "vue";

import * as api from "@/lib/api";

const POLL_MS = 2000;
/** 与后端 OAUTH_TIMEOUT_SECONDS 对齐，超时由后端返回，前端只兜底停表。 */
const MAX_POLL_MS = 5 * 60 * 1000;

export function useLogin(options: {
  log: (text: string, level?: "info" | "ok" | "err") => void;
  onAdded: () => Promise<void>;
}) {
  const pending = ref(false);
  const status = ref("");
  /** 扫码采集成功后的账号 meta：非 null 时界面弹「已采集账号」提示框。 */
  const collected = ref<api.LoginPoll["result"]>(null);

  let timer: number | undefined;
  let startedAt = 0;
  let loginId: string | null = null;

  function stop() {
    if (timer !== undefined) {
      window.clearTimeout(timer);
      timer = undefined;
    }
    loginId = null;
    pending.value = false;
  }

  async function poll() {
    if (!loginId) return;
    if (Date.now() - startedAt > MAX_POLL_MS) {
      stop();
      status.value = "";
      options.log("登录超时：浏览器授权未完成，请重新点击 ADD ACCT", "err");
      return;
    }
    try {
      const res = await api.pollLogin(loginId);
      if (res.done) {
        stop();
        status.value = "";
        if (res.error) {
          options.log(`登录失败：${res.error}`, "err");
        } else {
          // 弹「已采集账号」提示框：用户在浏览器扫码，界面必须给个收尾反馈
          collected.value = res.result ?? null;
          options.log("登录成功，账号已加入账号库", "ok");
          await options.onAdded();
        }
        return;
      }
    } catch (error) {
      stop();
      status.value = "";
      options.log(`登录轮询异常：${String(error)}`, "err");
      return;
    }
    timer = window.setTimeout(poll, POLL_MS);
  }

  async function start(variant?: "cn" | "intl") {
    if (pending.value) {
      // 再点一次 = 取消等待
      stop();
      status.value = "";
      options.log("已取消等待浏览器授权");
      return;
    }
    pending.value = true;
    status.value = "WAITING AUTH…";
    options.log("正在申请登录凭证…");
    try {
      const res = await api.startLogin(variant);
      if (!res.ok || !res.loginId) {
        stop();
        status.value = "";
        options.log(`无法发起登录：${res.error ?? "未知原因"}`, "err");
        return;
      }
      loginId = res.loginId;
      startedAt = Date.now();
      if (res.browserOpened) {
        options.log("已在浏览器打开登录页，完成授权后账号会自动加入");
      } else {
        options.log(
          `浏览器未能自动打开（${res.browserError ?? "未知原因"}），请手动访问：${res.url}`,
          "err",
        );
      }
      timer = window.setTimeout(poll, POLL_MS);
    } catch (error) {
      stop();
      status.value = "";
      options.log(`发起登录失败：${String(error)}`, "err");
    }
  }

  onBeforeUnmount(stop);

  /** 关闭「已采集账号」提示框。 */
  function dismissCollected() {
    collected.value = null;
  }

  return { pending, status, collected, dismissCollected, start, stop };
}

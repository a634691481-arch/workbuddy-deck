import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** 毫秒时间戳 → `HH:mm:ss`，日志行用。 */
export function formatClock(ts: number): string {
  const d = new Date(ts);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/** 毫秒时间戳 → `YYYY-MM-DD HH:mm`，归档时刻等台账列用（sessions.updated_at 即毫秒）。空值返回 —。 */
export function formatStamp(ts: number | null | undefined): string {
  if (!ts) return "—";
  const d = new Date(ts);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}


/**
 * 把后端/网络错误翻译成人话，并给一句能落地的建议。
 *
 * 原始报文像 `查询积分失败: HTTP 404` 这种，用户看不懂也不知道下一步做什么。
 * 这里只做「确定性映射」：认不出来的原样返回，绝不编原因。
 */
export function humanizeError(raw: string): { text: string; hint: string } {
  const text = (raw ?? "").trim();
  const hit = (code: number) => text.includes(`HTTP ${code}`) || text.includes(`${code} `);

  if (hit(401) || hit(403)) {
    return { text: "登录态失效或被拒绝", hint: "刷新 TOKEN；仍失败就重新登录该账号" };
  }
  if (hit(404)) {
    return { text: "接口不存在（可能官方已改版）", hint: "升级到最新版本后再试" };
  }
  if (hit(429)) {
    return { text: "请求过于频繁被限流", hint: "等 1–2 分钟再执行，或换个账号" };
  }
  if (hit(500) || hit(502) || hit(503) || hit(504)) {
    return { text: "服务端异常", hint: "官方接口临时故障，稍后重试" };
  }
  if (/timeout|timed out|超时/i.test(text)) {
    return { text: "请求超时", hint: "检查网络/代理，或稍后重试" };
  }
  if (/dns|connect|network|ENOTFOUND|ECONNREFUSED/i.test(text)) {
    return { text: "网络不可达", hint: "检查网络与代理设置" };
  }
  return { text, hint: "" };
}

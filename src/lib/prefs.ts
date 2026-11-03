/**
 * 显示偏好：主题（暗/亮）、圆角大小（0–16px）、主题色（强调色）。
 * 模块级 ref + localStorage 持久化；import 副作用即应用，
 * 在 createApp 挂载前执行，首屏不会闪错主题。
 */
import { ref } from "vue";

const THEME_KEY = "wb-ta/theme";
const RADIUS_KEY = "wb-ta/radius";
const ACCENT_KEY = "wb-ta/accent";

export type Theme = "dark" | "light";

/** 主题色候选：航空红为默认，其余为可读性过关的深色调（两套主题都压得住）。 */
export const ACCENTS = [
  { value: "#e61919", zh: "航空红", en: "HAZARD RED" },
  { value: "#e8590c", zh: "信号橙", en: "SIGNAL ORANGE" },
  { value: "#b8860b", zh: "琥珀", en: "AMBER" },
  { value: "#2f9e44", zh: "磷光绿", en: "GREEN" },
  { value: "#0c8599", zh: "青", en: "CYAN" },
  { value: "#1c7ed6", zh: "蓝", en: "BLUE" },
  { value: "#7048e8", zh: "紫", en: "VIOLET" },
  { value: "#d6336c", zh: "品红", en: "MAGENTA" },
  { value: "#57574f", zh: "石墨", en: "GRAPHITE" },
] as const;

export const theme = ref<Theme>(localStorage.getItem(THEME_KEY) === "light" ? "light" : "dark");

/** 圆角半径，px。上限与滑块一致。 */
export const RADIUS_MAX = 16;
export const radius = ref(loadRadius());

/** 强调色 hex；非法值回退默认航空红。 */
export const DEFAULT_ACCENT = ACCENTS[0].value;
export const accent = ref(loadAccent());

function loadRadius(): number {
  const n = Number(localStorage.getItem(RADIUS_KEY) ?? "0");
  if (!Number.isFinite(n)) return 0;
  return Math.min(RADIUS_MAX, Math.max(0, Math.round(n)));
}

function loadAccent(): string {
  const v = localStorage.getItem(ACCENT_KEY) ?? "";
  return /^#[0-9a-fA-F]{6}$/.test(v) ? v : DEFAULT_ACCENT;
}

function apply() {
  const el = document.documentElement;
  el.classList.toggle("light", theme.value === "light");
  // 圆角走内联变量：任何值实时生效，class 仅作状态标记
  el.classList.toggle("round", radius.value > 0);
  el.style.setProperty("--app-radius", `${radius.value}px`);
  el.style.setProperty("--accent", accent.value);
}

export function setTheme(next: Theme) {
  theme.value = next;
  localStorage.setItem(THEME_KEY, next);
  apply();
}

export function setRadius(px: number) {
  radius.value = Math.min(RADIUS_MAX, Math.max(0, Math.round(px)));
  localStorage.setItem(RADIUS_KEY, String(radius.value));
  apply();
}

export function setAccent(hex: string) {
  if (!/^#[0-9a-fA-F]{6}$/.test(hex)) return;
  accent.value = hex;
  localStorage.setItem(ACCENT_KEY, hex);
  apply();
}

apply();

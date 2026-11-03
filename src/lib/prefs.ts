/**
 * 显示偏好：主题（暗/亮）、圆角大小（0–16px）、主题色、界面密度、扫描线、数字格式。
 * 模块级 ref + localStorage 持久化；import 副作用即应用，
 * 在 createApp 挂载前执行，首屏不会闪错主题。
 */
import { ref } from "vue";

const THEME_KEY = "wb-ta/theme";
const RADIUS_KEY = "wb-ta/radius";
const ACCENT_KEY = "wb-ta/accent";
const DENSITY_KEY = "wb-ta/density";
const SCAN_KEY = "wb-ta/scan";
const NUM_KEY = "wb-ta/numformat";
const MOTION_KEY = "wb-ta/motion";
const SIDEBAR_KEY = "wb-ta/sidebar";

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

/** 界面密度：只缩放 rem 间距（字号是 px，不跟着变），紧凑模式一屏能多放 2–3 行。 */
export type Density = "compact" | "normal" | "comfy";
export const DENSITIES = [
  { key: "compact", zh: "紧凑", en: "COMPACT", size: "14px" },
  { key: "normal", zh: "标准", en: "NORMAL", size: "16px" },
  { key: "comfy", zh: "宽松", en: "COMFY", size: "17px" },
] as const;
export const density = ref<Density>(loadDensity());

/** 扫描线强度：0 关 / 0.25 淡 / 0.5 标准。 */
export const SCANS = [
  { key: 0, zh: "关", en: "OFF" },
  { key: 0.25, zh: "淡", en: "SOFT" },
  { key: 0.5, zh: "标准", en: "STD" },
] as const;
export const scan = ref<number>(loadScan());

/** 数字格式：grouped = 35,312；compact = 35.3K。 */
export type NumFormat = "group" | "compact";
export const numberFormat = ref<NumFormat>(localStorage.getItem(NUM_KEY) === "compact" ? "compact" : "group");

/** 动效强度：full = 正常；none = 关掉所有过渡与动画（等同 forced reduced-motion）。 */
export type Motion = "full" | "none";
export const motion = ref<Motion>(localStorage.getItem(MOTION_KEY) === "none" ? "none" : "full");

/** 侧栏形态：full = 编号+图标+文字（148px）；compact = 仅图标（56px）。 */
export type SidebarMode = "full" | "compact";
export const sidebar = ref<SidebarMode>(localStorage.getItem(SIDEBAR_KEY) === "compact" ? "compact" : "full");

function loadRadius(): number {
  const n = Number(localStorage.getItem(RADIUS_KEY) ?? "0");
  if (!Number.isFinite(n)) return 0;
  return Math.min(RADIUS_MAX, Math.max(0, Math.round(n)));
}

function loadAccent(): string {
  const v = localStorage.getItem(ACCENT_KEY) ?? "";
  return /^#[0-9a-fA-F]{6}$/.test(v) ? v : DEFAULT_ACCENT;
}

function loadDensity(): Density {
  const v = localStorage.getItem(DENSITY_KEY);
  return v === "compact" || v === "comfy" ? v : "normal";
}

function loadScan(): number {
  const raw = localStorage.getItem(SCAN_KEY);
  if (raw === null) return 0.5;
  const n = Number(raw);
  return Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : 0.5;
}

function apply() {
  const el = document.documentElement;
  el.classList.toggle("light", theme.value === "light");
  // 圆角走内联变量：任何值实时生效，class 仅作状态标记
  el.classList.toggle("round", radius.value > 0);
  el.style.setProperty("--app-radius", `${radius.value}px`);
  el.style.setProperty("--accent", accent.value);
  el.style.setProperty("--scan-opacity", String(scan.value));
  el.dataset.density = density.value;
  el.classList.toggle("no-motion", motion.value === "none");
  el.dataset.sidebar = sidebar.value;
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

export function setDensity(next: Density) {
  density.value = next;
  localStorage.setItem(DENSITY_KEY, next);
  apply();
}

export function setScan(next: number) {
  scan.value = Math.min(1, Math.max(0, next));
  localStorage.setItem(SCAN_KEY, String(scan.value));
  apply();
}

export function setNumberFormat(next: NumFormat) {
  numberFormat.value = next;
  localStorage.setItem(NUM_KEY, next);
}

export function setMotion(next: Motion) {
  motion.value = next;
  localStorage.setItem(MOTION_KEY, next);
  apply();
}

export function setSidebar(next: SidebarMode) {
  sidebar.value = next;
  localStorage.setItem(SIDEBAR_KEY, next);
  apply();
}

/**
 * 统一数字读数：grouped = 千分位，compact = 1.2M / 34.5K。
 * 非有限值一律给 "—"，绝不显示 NaN。
 */
export function fmtNum(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  if (numberFormat.value === "compact") {
    if (Math.abs(value) >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}M`;
    if (Math.abs(value) >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  }
  return value.toLocaleString("zh-CN", { maximumFractionDigits: 0 });
}

apply();

<script setup lang="ts">
// 窗口控制依赖 Tauri 宿主；浏览器预览时 win 为 null，按钮照常渲染但不生效（不白屏）。
import { computed, onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Eye, EyeOff, Bell, Minus, Moon, Square, Sun, X } from "lucide-vue-next";

import { locale, setLocale, t } from "@/lib/i18n";
import { loadingBar } from "@/lib/loading-bar";
import { isTauri } from "@/lib/tauri-env";
import { maskAll, setMaskAll } from "@/lib/task-meta";
import { setWindowBackdrop } from "@/lib/api";
import { useAlerts } from "@/composables/useAlerts";
import {
  ACCENTS,
  DENSITIES,
  FONT_SIZES,
  RADIUS_MAX,
  SCANS,
  STROKES,
  accent,
  backdrop,
  chartStyle,
  contrast,
  density,
  fontSize,
  motion,
  numberFormat,
  radius,
  scan,
  setAccent,
  setBackdrop,
  setChartStyle,
  setContrast,
  setDensity,
  setFontSize,
  setMotion,
  setNumberFormat,
  setRadius,
  setScan,
  setSidebar,
  setStroke,
  setTheme,
  setZebra,
  sidebar,
  stroke,
  theme,
  zebra,
} from "@/lib/prefs";
import { useUpdate } from "@/composables/useUpdate";

const win = isTauri() ? getCurrentWindow() : null;
const { current: rev, loadConfig } = useUpdate();

// 异常提醒中心：铃铛角标 + 下拉面板
const { alerts, criticalCount, dismiss, dismissAll, alertText, load: loadAlerts } = useAlerts();

/** 有 IPC 在飞（或刚结束的收尾窗口）就显示滑动态。 */
const busy = computed(() => loadingBar.inflight.value > 0 || loadingBar.tail.value);

// 标题栏「外观」弹层：主题 / 圆角 / 主题色 / 密度 / 扫描线 / 数字格式
const panel = ref<"" | "appearance" | "alerts">("");
/** 只要不是默认外观（亮色或改过色/圆角/密度/扫描线），按钮就高亮，方便看出当前非默认 */
const appearanceDirty = computed(
  () =>
    theme.value === "light" ||
    radius.value > 0 ||
    accent.value !== "#e61919" ||
    density.value !== "normal" ||
    scan.value !== 0.5 ||
    motion.value !== "full" ||
    sidebar.value !== "full" ||
    backdrop.value === "on" ||
    fontSize.value !== "md" ||
    zebra.value ||
    stroke.value !== "normal" ||
    contrast.value,
);
function togglePanel(name: "appearance" | "alerts") {
  // 打开提醒面板时刷新一次：铃铛数字与最新评估对齐
  if (name === "alerts" && panel.value !== "alerts") void loadAlerts().catch(() => {});
  panel.value = panel.value === name ? "" : name;
}

// 标题栏先拿到本机版本号（设置页没打开过也要显示对），不额外发检查请求。
onMounted(() => {
  void loadConfig();
  // 毛玻璃是原生窗口特效，不随 localStorage 恢复，启动时重放一次
  if (isTauri() && backdrop.value === "on") void setWindowBackdrop("acrylic");
});

async function toggleBackdrop(next: "off" | "on") {
  setBackdrop(next);
  if (isTauri()) await setWindowBackdrop(next === "on" ? "acrylic" : "off");
}
</script>

<template>
  <!-- 标题栏：纯黑底，底部 2px 即为加载条（空闲满格 / 加载中滑动） -->
  <header
    data-tauri-drag-region
    class="relative flex h-9 shrink-0 select-none items-center justify-between bg-titlebar pl-3"
  >
    <div data-tauri-drag-region class="flex min-w-0 items-baseline gap-3">
      <span class="display text-[15px] text-foreground">WorkBuddy</span>
      <span class="lbl tracking-[0.18em] text-muted-foreground">{{ t("app.subtitle") }}</span>
      <span class="lbl text-faint">REV {{ rev || "0.1.0" }}</span>
    </div>

    <div class="flex items-stretch self-stretch">
      <!-- 异常提醒中心：铃铛 + 角标（critical 计数，0 时不显示角标），点开下拉面板 -->
      <button
        class="relative flex items-center gap-1.5 border-l border-border px-2 text-[10px] uppercase tracking-[0.1em]"
        :class="criticalCount > 0 ? 'text-hazard' : 'text-faint hover:text-foreground'"
        :title="t('alr.title')"
        @click="togglePanel('alerts')"
      >
        <Bell class="size-3" />
        {{ t("alr.title") }}
        <span
          v-if="criticalCount > 0"
          class="readout ml-0.5 min-w-4 border border-hazard bg-hazard px-1 text-[9px] leading-4 text-white"
        >
          {{ criticalCount }}
        </span>
      </button>

      <!-- 外观：主题 / 圆角 / 主题色 / 密度 / 扫描线 / 数字格式 统一入口 -->
      <button
        class="flex items-center gap-1.5 border-l border-border px-2 text-[10px] uppercase tracking-[0.1em]"
        :class="appearanceDirty ? 'text-hazard' : 'text-faint hover:text-foreground'"
        :title="t('tb.appearance')"
        @click="togglePanel('appearance')"
      >
        <Sun v-if="theme === 'dark'" class="size-3" />
        <Moon v-else class="size-3" />
        <i class="size-2.5 border border-border-strong" :style="{ background: accent }" />
        {{ t("tb.appearance") }}
      </button>

      <!-- 名称脱敏开关：开 = 眼睛闭合（已隐藏），点一下切明文 -->
      <button
        class="flex items-center gap-1 border-l border-border px-2 text-[10px] uppercase tracking-[0.1em]"
        :class="maskAll ? 'text-hazard' : 'text-faint hover:text-foreground'"
        :title="maskAll ? t('tb.maskOn') : t('tb.maskOff')"
        @click="setMaskAll(!maskAll)"
      >
        <EyeOff v-if="maskAll" class="size-3" />
        <Eye v-else class="size-3" />
        {{ t("tb.mask") }}
      </button>

      <!-- 语言切换：两段式，当前语言反白 -->
      <div class="flex items-center gap-0 border-l border-border px-1">
        <button
          class="px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
          :class="locale === 'zh' ? 'bg-foreground text-background' : 'text-faint hover:text-foreground'"
          @click="setLocale('zh')"
        >
          中
        </button>
        <button
          class="px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
          :class="locale === 'en' ? 'bg-foreground text-background' : 'text-faint hover:text-foreground'"
          @click="setLocale('en')"
        >
          EN
        </button>
      </div>

      <button
        class="grid w-11 place-items-center border-l border-border text-muted-foreground hover:bg-muted hover:text-foreground"
        title="最小化"
        @click="win?.minimize()"
      >
        <Minus class="size-3.5" />
      </button>
      <button
        class="grid w-11 place-items-center border-l border-border text-muted-foreground hover:bg-muted hover:text-foreground"
        title="最大化"
        @click="win?.toggleMaximize()"
      >
        <Square class="size-3" />
      </button>
      <button
        class="grid w-11 place-items-center border-l border-border text-muted-foreground hover:bg-hazard hover:text-white"
        title="关闭（最小化到托盘）"
        @click="win?.close()"
      >
        <X class="size-4" />
      </button>
    </div>

    <!-- 外观面板：透明背板点击关闭；面板淡入淡出，不做位移（贴窗口边缘会露缝） -->
    <Transition name="wb-pop">
      <div v-if="panel" class="fixed inset-0 z-40 cursor-default" @click="panel = ''" />
    </Transition>
    <Transition name="wb-pop">
    <div
      v-if="panel === 'appearance'"
      class="absolute right-24 top-full z-50 max-h-[76vh] w-64 space-y-2.5 overflow-y-auto border border-border-strong bg-card p-3"
    >
      <!-- 主题 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.theme") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="theme === 'dark' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setTheme('dark')"
          >
            {{ t("tb.dark") }}
          </button>
          <button
            type="button"
            class="px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="theme === 'light' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setTheme('light')"
          >
            {{ t("tb.light") }}
          </button>
        </div>
      </div>

      <!-- 字号 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.fontsize") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            v-for="f in FONT_SIZES"
            :key="f.key"
            type="button"
            class="px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="fontSize === f.key ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setFontSize(f.key)"
          >
            {{ locale === "zh" ? f.zh : f.en }}
          </button>
        </div>
      </div>

      <!-- 圆角 -->
      <div>
        <div class="mb-1 flex items-center justify-between">
          <span class="lbl">{{ t("tb.roundSize") }}</span>
          <span class="readout text-[11px]" :class="radius > 0 ? 'text-hazard' : 'text-faint'">
            {{ radius > 0 ? `${radius}px` : t("tb.roundZero") }}
          </span>
        </div>
        <input
          :value="radius"
          type="range"
          min="0"
          :max="RADIUS_MAX"
          step="1"
          class="w-full accent-hazard"
          @input="setRadius(($event.target as HTMLInputElement).valueAsNumber)"
        />
      </div>

      <!-- 主题色：预设 9 色 + 任意取色 -->
      <div>
        <div class="mb-1 flex items-center justify-between">
          <span class="lbl">{{ t("tb.colorTitle") }}</span>
          <label class="flex items-center gap-1 text-[9px] text-faint">
            {{ t("tb.custom") }}
            <input
              :value="accent"
              type="color"
              class="size-4 cursor-pointer border border-border-strong bg-transparent p-0"
              @input="setAccent(($event.target as HTMLInputElement).value)"
            />
          </label>
        </div>
        <div class="grid grid-cols-9 gap-1">
          <button
            v-for="c in ACCENTS"
            :key="c.value"
            type="button"
            class="relative grid h-5 place-items-center border transition-colors"
            :class="accent.toLowerCase() === c.value ? 'border-foreground' : 'border-transparent hover:border-border-strong'"
            :title="locale === 'zh' ? c.zh : c.en"
            @click="setAccent(c.value)"
          >
            <i class="size-3.5" :style="{ background: c.value }" />
          </button>
        </div>
      </div>

      <!-- 描边强度 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.stroke") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            v-for="k in STROKES"
            :key="k.key"
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="stroke === k.key ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setStroke(k.key)"
          >
            {{ locale === "zh" ? k.zh : k.en }}
          </button>
        </div>
      </div>

      <!-- 窗口背景 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.backdrop") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="backdrop === 'off' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="toggleBackdrop('off')"
          >
            {{ t("tb.backdropOff") }}
          </button>
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="backdrop === 'on' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="toggleBackdrop('on')"
          >
            {{ t("tb.backdropOn") }}
          </button>
        </div>
      </div>

      <!-- 界面密度 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.density") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            v-for="d in DENSITIES"
            :key="d.key"
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="density === d.key ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setDensity(d.key)"
          >
            {{ locale === "zh" ? d.zh : d.en }}
          </button>
        </div>
      </div>

      <!-- 斑马纹 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.zebra") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="!zebra ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setZebra(false)"
          >
            {{ t("tb.off") }}
          </button>
          <button
            type="button"
            class="px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="zebra ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setZebra(true)"
          >
            {{ t("tb.on") }}
          </button>
        </div>
      </div>

      <!-- 高对比度 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.contrast") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="!contrast ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setContrast(false)"
          >
            {{ t("tb.off") }}
          </button>
          <button
            type="button"
            class="px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="contrast ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setContrast(true)"
          >
            {{ t("tb.on") }}
          </button>
        </div>
      </div>

      <!-- 扫描线 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.scan") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            v-for="s in SCANS"
            :key="s.key"
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="scan === s.key ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setScan(s.key)"
          >
            {{ locale === "zh" ? s.zh : s.en }}
          </button>
        </div>
      </div>

      <!-- 图表 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.chart") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="chartStyle === 'bar' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setChartStyle('bar')"
          >
            {{ t("tb.chartBar") }}
          </button>
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="chartStyle === 'line' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setChartStyle('line')"
          >
            {{ t("tb.chartLine") }}
          </button>
        </div>
      </div>

      <!-- 动效 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.motion") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="motion === 'full' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setMotion('full')"
          >
            {{ t("tb.motionOn") }}
          </button>
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="motion === 'none' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setMotion('none')"
          >
            {{ t("tb.motionOff") }}
          </button>
        </div>
      </div>

      <!-- 侧栏形态 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.sidebar") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="sidebar === 'full' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setSidebar('full')"
          >
            {{ t("tb.sideFull") }}
          </button>
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="sidebar === 'compact' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setSidebar('compact')"
          >
            {{ t("tb.sideCompact") }}
          </button>
        </div>
      </div>

      <!-- 数字格式 -->
      <div class="flex items-center justify-between gap-2">
        <span class="lbl">{{ t("tb.number") }}</span>
        <div class="flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="numberFormat === 'group' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setNumberFormat('group')"
          >
            35,312
          </button>
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px] uppercase tracking-[0.1em]"
            :class="numberFormat === 'compact' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setNumberFormat('compact')"
          >
            35.3K
          </button>
        </div>
      </div>
    </div>
    </Transition>

    <!-- 异常提醒面板：与外观面板同款贴边下拉 -->
    <Transition name="wb-pop">
    <div
      v-if="panel === 'alerts'"
      class="absolute right-40 top-full z-50 max-h-[70vh] w-80 overflow-y-auto border border-border-strong bg-card"
    >
      <header class="sticky top-0 flex items-center justify-between gap-2 border-b border-border bg-card px-3 py-2 wb-flat">
        <span class="lbl text-foreground">{{ t("alr.title") }}</span>
        <button
          v-if="alerts.length > 0"
          type="button"
          class="lbl text-faint hover:text-hazard"
          @click="dismissAll()"
        >
          {{ t("alr.dismissAll") }}
        </button>
      </header>
      <div v-if="alerts.length === 0" class="px-3 py-8 text-center text-[10px] text-faint">
        {{ t("alr.empty") }}
      </div>
      <div
        v-for="item in alerts"
        :key="item.fingerprint"
        class="group flex items-start gap-2 border-b border-border/60 px-3 py-2 last:border-b-0"
      >
        <span
          class="mt-1 size-1.5 shrink-0"
          :class="item.level === 'critical' ? 'bg-hazard' : 'bg-border-strong'"
          :title="item.level === 'critical' ? t('alr.critical') : t('alr.warn')"
        />
        <div class="min-w-0 flex-1">
          <p class="break-all text-[10px] leading-4" :class="item.level === 'critical' ? 'text-foreground' : 'text-muted-foreground'">
            {{ alertText(item) }}
          </p>
          <p class="readout mt-0.5 text-[8px] text-faint">
            {{ new Date(item.ts).toLocaleString() }}
          </p>
        </div>
        <button
          type="button"
          class="lbl shrink-0 px-1 text-[9px] text-faint opacity-0 transition-opacity group-hover:opacity-100 hover:text-foreground"
          :title="t('alr.dismiss')"
          @click="dismiss(item.fingerprint)"
        >
          ✕
        </button>
      </div>
      <footer v-if="alerts.length > 0" class="border-t border-border px-3 py-1.5 wb-flat">
        <p class="text-[8px] leading-3 text-faint">{{ t("alr.hint") }}</p>
      </footer>
    </div>
    </Transition>

    <!-- 加载条：占位就是原来那 2px 红线。空闲 = 满格；有在飞请求 = 滑块巡游 -->
    <div class="pointer-events-none absolute inset-x-0 bottom-0 h-0.5 overflow-hidden bg-hazard/25">
      <div
        v-if="busy"
        class="bar-sweep h-full w-1/4 bg-hazard"
      />
      <div v-else class="h-full w-full bg-hazard" />
    </div>
  </header>
</template>

<style scoped>
/* 纯色块平移，无渐变无圆角；速度固定，避免看起来像在"假装"有进度 */
.bar-sweep {
  animation: bar-sweep 1.15s linear infinite;
}
@keyframes bar-sweep {
  from {
    transform: translateX(-100%);
  }
  to {
    transform: translateX(400%);
  }
}
@media (prefers-reduced-motion: reduce) {
  .bar-sweep {
    animation: none;
    width: 100%;
    opacity: 0.45;
  }
}
</style>

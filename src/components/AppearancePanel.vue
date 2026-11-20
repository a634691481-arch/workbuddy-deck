<script setup lang="ts">
/**
 * 外观设置：从标题栏搬到系统设置页（标题栏只留品牌与窗口按钮）。
 * 所有状态都在 lib/prefs 里，组件本身无状态。
 */
import { setWindowBackdrop } from "@/lib/api";
import { locale, t } from "@/lib/i18n";
import {
  ACCENTS,
  DENSITIES,
  FONT_SIZES,
  RADIUS_MAX,
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
  setAccent,
  setBackdrop,
  setChartStyle,
  setContrast,
  setDensity,
  setFontSize,
  setMotion,
  setNumberFormat,
  setRadius,
  setStroke,
  setTheme,
  setZebra,
  stroke,
  theme,
  zebra,
} from "@/lib/prefs";
import { hasHostWindow } from "@/lib/tauri-env";

/** 毛玻璃：先写偏好，再驱动原生 acrylic。 */
function applyBackdrop(next: "on" | "off") {
  setBackdrop(next);
  if (hasHostWindow()) void setWindowBackdrop(next === "on" ? "acrylic" : "off");
}

/** 两段式按钮直接传目标值。 */
function toggleBackdrop(next: "on" | "off") {
  applyBackdrop(next);
}
</script>

<template>
  <section class="grid grid-cols-2 gap-2">
      <!-- 主题 -->
      <div class="flex min-w-0 flex-col items-start gap-1 border border-border bg-card px-2.5 py-1.5">
        <span class="lbl">{{ t("tb.theme") }}</span>
        <div class="inline-flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-2 py-0.5 text-[10px]"
            :class="theme === 'dark' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setTheme('dark')"
          >
            {{ t("tb.dark") }}
          </button>
          <button
            type="button"
            class="px-2 py-0.5 text-[10px]"
            :class="theme === 'light' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setTheme('light')"
          >
            {{ t("tb.light") }}
          </button>
        </div>
      </div>

      <!-- 字号 -->
      <div class="flex min-w-0 flex-col items-start gap-1 border border-border bg-card px-2.5 py-1.5">
        <span class="lbl">{{ t("tb.fontsize") }}</span>
        <div class="inline-flex border border-border-strong wb-seg">
          <button
            v-for="f in FONT_SIZES"
            :key="f.key"
            type="button"
            class="px-2 py-0.5 text-[10px]"
            :class="fontSize === f.key ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setFontSize(f.key)"
          >
            {{ locale === "zh" ? f.zh : f.en }}
          </button>
        </div>
      </div>

      <!-- 圆角 -->
      <div class="col-span-2 border border-border bg-card px-2 py-1.5">
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
      <div class="col-span-2 border border-border bg-card px-2 py-1.5">
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
        <div class="flex flex-wrap items-center gap-2">
          <button
            v-for="c in ACCENTS"
            :key="c.value"
            type="button"
            class="grid h-5 w-5 place-items-center focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-2 focus-visible:outline-foreground"
            :title="locale === 'zh' ? c.zh : c.en"
            @click="setAccent(c.value)"
          >
            <!-- 选中态 = 色块自身 1px 前景色描边，不在色块外套黑框：
                 套框会把色块包成"黑饼"，看不出选了哪个颜色 -->
            <i
              class="size-3.5 border transition-colors duration-150"
              :class="
                accent.toLowerCase() === c.value
                  ? 'border-foreground'
                  : 'border-transparent hover:border-border-strong'
              "
              :style="{ background: c.value }"
            />
          </button>
        </div>
      </div>

      <!-- 描边强度 -->
      <div class="flex min-w-0 flex-col items-start gap-1 border border-border bg-card px-2.5 py-1.5">
        <span class="lbl">{{ t("tb.stroke") }}</span>
        <div class="inline-flex border border-border-strong wb-seg">
          <button
            v-for="k in STROKES"
            :key="k.key"
            type="button"
            class="px-1.5 py-0.5 text-[10px]"
            :class="stroke === k.key ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setStroke(k.key)"
          >
            {{ locale === "zh" ? k.zh : k.en }}
          </button>
        </div>
      </div>

      <!-- 窗口背景 -->
      <div class="flex min-w-0 flex-col items-start gap-1 border border-border bg-card px-2.5 py-1.5">
        <span class="lbl">{{ t("tb.backdrop") }}</span>
        <div class="inline-flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px]"
            :class="backdrop === 'off' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="toggleBackdrop('off')"
          >
            {{ t("tb.backdropOff") }}
          </button>
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px]"
            :class="backdrop === 'on' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="toggleBackdrop('on')"
          >
            {{ t("tb.backdropOn") }}
          </button>
        </div>
      </div>

      <!-- 界面密度 -->
      <div class="flex min-w-0 flex-col items-start gap-1 border border-border bg-card px-2.5 py-1.5">
        <span class="lbl">{{ t("tb.density") }}</span>
        <div class="inline-flex border border-border-strong wb-seg">
          <button
            v-for="d in DENSITIES"
            :key="d.key"
            type="button"
            class="px-1.5 py-0.5 text-[10px]"
            :class="density === d.key ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setDensity(d.key)"
          >
            {{ locale === "zh" ? d.zh : d.en }}
          </button>
        </div>
      </div>

      <!-- 斑马纹 -->
      <div class="flex min-w-0 flex-col items-start gap-1 border border-border bg-card px-2.5 py-1.5">
        <span class="lbl">{{ t("tb.zebra") }}</span>
        <div class="inline-flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-2 py-0.5 text-[10px]"
            :class="!zebra ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setZebra(false)"
          >
            {{ t("tb.off") }}
          </button>
          <button
            type="button"
            class="px-2 py-0.5 text-[10px]"
            :class="zebra ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setZebra(true)"
          >
            {{ t("tb.on") }}
          </button>
        </div>
      </div>

      <!-- 高对比度 -->
      <div class="flex min-w-0 flex-col items-start gap-1 border border-border bg-card px-2.5 py-1.5">
        <span class="lbl">{{ t("tb.contrast") }}</span>
        <div class="inline-flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-2 py-0.5 text-[10px]"
            :class="!contrast ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setContrast(false)"
          >
            {{ t("tb.off") }}
          </button>
          <button
            type="button"
            class="px-2 py-0.5 text-[10px]"
            :class="contrast ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setContrast(true)"
          >
            {{ t("tb.on") }}
          </button>
        </div>
      </div>

      <!-- 图表 -->
      <div class="flex min-w-0 flex-col items-start gap-1 border border-border bg-card px-2.5 py-1.5">
        <span class="lbl">{{ t("tb.chart") }}</span>
        <div class="inline-flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px]"
            :class="chartStyle === 'bar' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setChartStyle('bar')"
          >
            {{ t("tb.chartBar") }}
          </button>
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px]"
            :class="chartStyle === 'line' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setChartStyle('line')"
          >
            {{ t("tb.chartLine") }}
          </button>
        </div>
      </div>

      <!-- 动效 -->
      <div class="flex min-w-0 flex-col items-start gap-1 border border-border bg-card px-2.5 py-1.5">
        <span class="lbl">{{ t("tb.motion") }}</span>
        <div class="inline-flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px]"
            :class="motion === 'full' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setMotion('full')"
          >
            {{ t("tb.motionOn") }}
          </button>
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px]"
            :class="motion === 'none' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setMotion('none')"
          >
            {{ t("tb.motionOff") }}
          </button>
        </div>
      </div>


      <!-- 数字格式 -->
      <div class="flex min-w-0 flex-col items-start gap-1 border border-border bg-card px-2.5 py-1.5">
        <span class="lbl">{{ t("tb.number") }}</span>
        <div class="inline-flex border border-border-strong wb-seg">
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px]"
            :class="numberFormat === 'group' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setNumberFormat('group')"
          >
            35,312
          </button>
          <button
            type="button"
            class="px-1.5 py-0.5 text-[10px]"
            :class="numberFormat === 'compact' ? 'bg-foreground text-background' : 'text-muted-foreground hover:text-foreground'"
            @click="setNumberFormat('compact')"
          >
            35.3K
          </button>
        </div>
      </div>
    </section>
</template>

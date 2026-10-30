<script setup lang="ts">
// 窗口控制依赖 Tauri 宿主；浏览器预览时 win 为 null，按钮照常渲染但不生效（不白屏）。
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Eye, EyeOff, Minus, Square, X } from "lucide-vue-next";

import { locale, setLocale, t } from "@/lib/i18n";
import { isTauri } from "@/lib/tauri-env";
import { maskAll, setMaskAll } from "@/lib/task-meta";

const win = isTauri() ? getCurrentWindow() : null;
</script>

<template>
  <!-- 标题栏：纯黑底 + 底部 2px 危险红，整块界面的"机头" -->
  <header
    data-tauri-drag-region
    class="flex h-9 shrink-0 select-none items-center justify-between border-b-2 border-hazard bg-titlebar pl-3"
  >
    <div data-tauri-drag-region class="flex min-w-0 items-baseline gap-3">
      <span class="display text-[15px] text-foreground">WorkBuddy</span>
      <span class="lbl tracking-[0.18em] text-muted-foreground">{{ t("app.subtitle") }}</span>
      <span class="lbl text-faint">REV 0.1.62</span>
    </div>

    <div class="flex items-stretch self-stretch">
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
  </header>
</template>

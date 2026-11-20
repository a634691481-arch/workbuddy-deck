<script setup lang="ts">
/**
 * 外观分区：主题 / 字号 / 圆角 / 主题色 / 密度 / 毛玻璃 / 脱敏 / 语言。
 *
 * 从标题栏弹层里拆出来单独一页（2026-11-20）：
 * 标题栏只留壳，配置项全部集中到此。
 */
import AppearancePanel from "@/components/AppearancePanel.vue";
import { locale, setLocale, t } from "@/lib/i18n";
import { maskAll, setMaskAll } from "@/lib/task-meta";
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <header
      class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-border bg-panel px-3 py-2 wb-flat"
    >
      <h1 class="text-[13px] font-medium text-foreground">{{ t("tb.appearance") }}</h1>
      <div class="flex items-center gap-1.5">
        <button
          type="button"
          class="border px-2 py-1 text-[11px] transition-colors"
          :class="
            maskAll
              ? 'border-hazard bg-hazard-soft text-hazard'
              : 'border-border-strong text-muted-foreground hover:text-foreground'
          "
          :title="maskAll ? t('tb.maskOn') : t('tb.maskOff')"
          @click="setMaskAll(!maskAll)"
        >
          {{ t("tb.mask") }}
        </button>
        <div class="flex wb-seg border border-border-strong">
          <button
            type="button"
            class="px-2 py-1 text-[11px] transition-colors"
            :class="
              locale === 'zh'
                ? 'bg-foreground text-background'
                : 'text-muted-foreground hover:text-foreground'
            "
            @click="setLocale('zh')"
          >
            中
          </button>
          <button
            type="button"
            class="px-2 py-1 text-[11px] transition-colors"
            :class="
              locale === 'en'
                ? 'bg-foreground text-background'
                : 'text-muted-foreground hover:text-foreground'
            "
            @click="setLocale('en')"
          >
            EN
          </button>
        </div>
      </div>
    </header>

    <div class="min-h-0 flex-1 overflow-y-auto p-2.5">
      <AppearancePanel />
    </div>
  </div>
</template>

<script setup lang="ts">
// 状态栏：整条界面最底部的遥测行。
// 磷光绿只在这里出现一次（READY），执行中转为红色告警。
import { t } from "@/lib/i18n";

const props = defineProps<{
  total: number;
  lastRefresh: string;
  logCount: number;
  running: boolean;
}>();

const pad = (n: number) => String(n).padStart(3, "0");
</script>

<template>
  <footer
    class="flex h-6 shrink-0 select-none items-center gap-4 border-t border-border bg-titlebar px-2"
  >
    <span class="flex items-center gap-1.5">
      <i class="block size-1.5" :class="props.running ? 'bg-hazard' : 'bg-success'" />
      <span class="lbl" :class="props.running ? 'text-hazard' : 'text-success'">
        {{ props.running ? t("sb.busy") : t("sb.ready") }}
      </span>
    </span>
    <span class="lbl text-faint">{{ t("sb.last") }} {{ props.lastRefresh }}</span>
    <span class="lbl readout text-faint">{{ t("sb.accts") }} {{ pad(props.total) }}</span>
    <span class="lbl readout text-faint">{{ t("sb.logs") }} {{ pad(props.logCount) }}</span>
    <span class="lbl ml-auto text-faint">workbuddy-switch // telemetry</span>
  </footer>
</template>

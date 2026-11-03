<script setup lang="ts">
import AnimatedNumber from "@/components/ui/AnimatedNumber.vue";
/**
 * 遥测读数带：6 格关键指标，用 1px 缝分隔（skill §8「grid gap:1px + 对比底色造分割线」）。
 *
 * 纪律：
 * - 只展示**真实可得**的数据，拿不到就显示占位符，不编数字。
 * - TOTAL CREDIT 是全账号汇总（最关心的一项），给它更大的字号。
 * - 失败/未签用红，其余用磷光白；绿色只留给 READY。
 */
import { t } from "@/lib/i18n";

const props = defineProps<{
  accounts: number;
  signed: boolean;
  taskDone: number;
  taskTotal: number;
  totalCredit: number | null;
  totalLoading: boolean;
  totalFailed: number;
  balance: number | null;
  balanceLoading: boolean;
  runOk: number;
  runFail: number;
  runSkip: number;
  running: boolean;
}>();

const pad = (n: number) => String(n).padStart(3, "0");

</script>

<template>
  <section class="shrink-0">
    <div class="grid min-w-0 grid-cols-6 gap-2">
      <!-- 01 账号总量 -->
      <div class="min-w-0 border border-border bg-card px-3 py-2">
        <div class="lbl">{{ t("tm.accts") }}</div>
        <div class="readout mt-1 truncate text-[17px] leading-none text-foreground">{{ pad(props.accounts) }}</div>
        <div class="lbl mt-1 text-faint">{{ t("tm.units") }}</div>
      </div>

      <!-- 02 当前账号签到 -->
      <div class="min-w-0 border border-border bg-card px-3 py-2">
        <div class="lbl">{{ t("tm.signed") }}</div>
        <div
          class="readout mt-1 text-[17px] leading-none"
          :class="props.signed ? 'text-foreground' : 'text-hazard'"
        >
          {{ props.signed ? t("tm.yes") : t("tm.no") }}
        </div>
        <div class="lbl mt-1 text-faint">{{ t("tm.today") }}</div>
      </div>

      <!-- 03 任务完成度 -->
      <div class="min-w-0 border border-border bg-card px-3 py-2">
        <div class="lbl">{{ t("tm.tasks") }}</div>
        <div class="mt-1 flex items-baseline gap-1">
          <span class="readout text-[17px] leading-none text-foreground">{{ pad(props.taskDone) }}</span>
          <span class="readout text-[10px] text-faint">/ {{ pad(props.taskTotal) }}</span>
        </div>
        <div class="lbl mt-1 text-faint">{{ t("tm.claimed") }}</div>
      </div>

      <!-- 04 全账号积分总额（核心指标，字号最大） -->
      <div class="min-w-0 border border-border bg-card px-3 py-2" :class="props.totalFailed > 0 && 'bg-hazard-soft'">
        <div class="lbl flex items-center gap-1">
          {{ t("tm.totalCredit") }}
          <span v-if="props.totalFailed > 0" class="text-hazard">⚠{{ props.totalFailed }}</span>
        </div>
        <div class="readout mt-0.5 truncate text-[22px] leading-none text-foreground">
          <AnimatedNumber :value="props.totalCredit" :placeholder="props.totalLoading ? '···' : '—'" />
        </div>
        <div class="lbl mt-1 text-faint">{{ t("tm.allAccts") }}</div>
      </div>

      <!-- 05 当前账号余额 -->
      <div class="min-w-0 border border-border bg-card px-3 py-2">
        <div class="lbl">{{ t("tm.thisUnit") }}</div>
        <div class="readout mt-1 truncate text-[17px] leading-none text-foreground">
          <AnimatedNumber :value="props.balance" :placeholder="props.balanceLoading ? '···' : '—'" />
        </div>
        <div class="lbl mt-1 text-faint">{{ t("tm.credit") }}</div>
      </div>

      <!-- 06 本轮执行结果 -->
      <div class="min-w-0 border border-border bg-card px-3 py-2" :class="props.runFail > 0 && 'bg-hazard-soft'">
        <div class="lbl flex items-center gap-1">
          {{ t("tm.lastRun") }}
          <span v-if="props.running" class="text-hazard">///</span>
        </div>
        <div class="mt-1 flex items-baseline gap-2.5">
          <span class="readout text-[17px] leading-none text-foreground">{{ pad(props.runOk) }}</span>
          <span class="readout text-[11px] leading-none text-hazard">✕{{ pad(props.runFail) }}</span>
          <span class="readout text-[11px] leading-none text-faint">—{{ pad(props.runSkip) }}</span>
        </div>
        <div class="lbl mt-1 text-faint">{{ t("tm.okErrSkip") }}</div>
      </div>
    </div>
  </section>
</template>

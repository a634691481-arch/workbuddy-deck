<script setup lang="ts">
import { computed } from "vue";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

/**
 * 状态标记 = 电报式方框标签：直角、1px 描边、等宽大写。
 * 色相只有红/绿两族，其余靠线型与明度区分（工业纪律，见 style.css 头注释）。
 */
const badgeVariants = cva(
  "inline-flex items-center whitespace-nowrap border px-1.5 py-[1px] font-mono text-[10px] uppercase leading-4 tracking-[0.08em]",
  {
    variants: {
      tone: {
        // 已领奖：磷光绿（全站仅此与 READY 用绿）
        success: "border-success-border bg-success-soft text-success",
        // 待领奖：白描边，等待人工动作
        info: "border-border-strong bg-transparent text-foreground",
        // 进行中：红 —— 未达成的告警态
        warning: "border-hazard-dim bg-hazard-soft text-hazard",
        danger: "border-hazard bg-hazard text-white",
        // 未接取：暗灰，退到背景里
        muted: "border-border bg-transparent text-muted-foreground",
      },
    },
    defaultVariants: { tone: "muted" },
  },
);

const props = defineProps<{
  tone?: VariantProps<typeof badgeVariants>["tone"];
  class?: string;
}>();
const classes = computed(() => cn(badgeVariants({ tone: props.tone }), props.class));
</script>

<template>
  <span :class="classes"><slot /></span>
</template>

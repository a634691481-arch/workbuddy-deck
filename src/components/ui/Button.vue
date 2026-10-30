<script setup lang="ts">
import { computed } from "vue";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

/**
 * 工业按钮：直角、等宽大写、无柔和过渡。
 * 主按钮是「白磷块 + 黑字」，不是彩色胶囊 —— 彩色只留给危险动作。
 */
const buttonVariants = cva(
  "inline-flex select-none items-center justify-center gap-1.5 whitespace-nowrap border font-mono uppercase tracking-[0.08em] transition-none active:translate-y-px disabled:pointer-events-none disabled:opacity-40",
  {
    variants: {
      variant: {
        default:
          "border-primary bg-primary text-primary-foreground hover:border-[#c9c9c2] hover:bg-[#c9c9c2]",
        outline:
          "border-border-strong bg-transparent text-foreground hover:border-foreground hover:bg-muted",
        ghost:
          "border-transparent bg-transparent text-muted-foreground hover:bg-muted hover:text-foreground",
        success:
          "border-success-border bg-transparent text-success hover:border-success hover:bg-success-soft",
        danger: "border-hazard bg-hazard text-white hover:border-[#ff2a2a] hover:bg-[#ff2a2a]",
      },
      size: {
        default: "h-8 px-3 text-[11px]",
        sm: "h-6 px-2 text-[10px]",
        lg: "h-9 px-4 text-xs",
        icon: "size-7",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  },
);

type Props = {
  variant?: VariantProps<typeof buttonVariants>["variant"];
  size?: VariantProps<typeof buttonVariants>["size"];
  class?: string;
  disabled?: boolean;
};

const props = defineProps<Props>();
const classes = computed(() =>
  cn(buttonVariants({ variant: props.variant, size: props.size }), props.class),
);
</script>

<template>
  <button :class="classes" :disabled="props.disabled">
    <slot />
  </button>
</template>

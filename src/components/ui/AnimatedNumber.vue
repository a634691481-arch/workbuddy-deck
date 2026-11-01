<script setup lang="ts">
/**
 * 数值滚动：值变化时从旧值缓出到新值（260ms），刷新读数不再「整块跳一下」。
 *
 * 只在值真的变化时动；首帧直接显示终值（开机不该看着数字从 0 爬）。
 * `prefers-reduced-motion` 下直接跳变。
 */
import { computed, onBeforeUnmount, ref, watch } from "vue";

const props = withDefaults(
  defineProps<{
    value: number | null | undefined;
    decimals?: number;
    placeholder?: string;
    duration?: number;
  }>(),
  { decimals: 0, placeholder: "—", duration: 260 },
);

const reduced =
  typeof window !== "undefined" &&
  window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;

const shown = ref<number>(typeof props.value === "number" ? props.value : 0);
let raf = 0;

watch(
  () => props.value,
  (next) => {
    if (typeof next !== "number") return;
    if (reduced) {
      shown.value = next;
      return;
    }
    cancelAnimationFrame(raf);
    const from = shown.value;
    const delta = next - from;
    if (delta === 0) return;
    const t0 = performance.now();
    const step = (now: number) => {
      const p = Math.min(1, (now - t0) / props.duration);
      shown.value = from + delta * (1 - Math.pow(1 - p, 3));
      if (p < 1) raf = requestAnimationFrame(step);
      else shown.value = next;
    };
    raf = requestAnimationFrame(step);
  },
);

onBeforeUnmount(() => cancelAnimationFrame(raf));

const text = computed(() => {
  if (typeof props.value !== "number") return props.placeholder;
  return shown.value.toLocaleString("zh-CN", {
    minimumFractionDigits: props.decimals,
    maximumFractionDigits: props.decimals,
  });
});
</script>

<template>
  <span>{{ text }}</span>
</template>

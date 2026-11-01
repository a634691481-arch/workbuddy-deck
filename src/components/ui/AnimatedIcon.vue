<script setup lang="ts">
/**
 * Lordicon 动画图标（本地 JSON，离线可用）。
 * 底层是 @lordicon/element 注册的 <lord-icon> 自定义元素。
 */
import { computed } from "vue";

const props = withDefaults(
  defineProps<{
    /** Lottie JSON 地址（?url 导入，见 @/assets/lordicon）。 */
    src: string;
    /** 边长（px，正方形）。 */
    size?: number;
    /** hover | click | in | loop | loop-on-hover | morph | boomerang | none */
    trigger?: string;
    primary?: string;
    secondary?: string;
    stroke?: string;
  }>(),
  { size: 14, trigger: "hover", primary: "#eaeaea", secondary: "#8a8a85", stroke: "2" },
);

const colors = computed(() => `primary:${props.primary};secondary:${props.secondary}`);
const box = computed(() => ({ width: `${props.size}px`, height: `${props.size}px` }));
</script>

<template>
  <lord-icon
    :src="src"
    :trigger="trigger"
    :colors="colors"
    :stroke="stroke"
    :style="box"
    class="pointer-events-none block shrink-0"
    aria-hidden="true"
  />
</template>

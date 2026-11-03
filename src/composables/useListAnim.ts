/**
 * 列表动画（AutoAnimate）：只给「会增删 / 会重排」的容器用。
 *
 * 三条护栏（AGENTS §0.1）：
 * 1. 外观 → 动效 = 关时整体禁用，不跟偏好打架；
 * 2. 调用方传 `enabled` 判定（批量执行中、超长列表要传 false）；
 * 3. 只挂在行列表容器上，不挂表头，避免 FLIP transform 与 sticky 打架。
 */
import autoAnimate, { type AnimationController, type AutoAnimateOptions } from "@formkit/auto-animate";
import { onBeforeUnmount, onMounted, ref, watchEffect } from "vue";

import { motion } from "@/lib/prefs";

/** 工业风时长：比库默认 250ms 快一档，不拖沓 */
const BASE: Partial<AutoAnimateOptions> = { duration: 160, easing: "ease-out" };

export function useListAnim(enabled: () => boolean = () => true) {
  const el = ref<HTMLElement | null>(null);
  const ctl = ref<AnimationController | null>(null);

  onMounted(() => {
    if (el.value) ctl.value = autoAnimate(el.value, BASE);
  });

  // 跟随「动效偏好 + 调用方护栏」实时开关
  watchEffect(() => {
    if (motion.value === "full" && enabled()) ctl.value?.enable();
    else ctl.value?.disable();
  });

  onBeforeUnmount(() => ctl.value?.destroy?.());

  return el;
}

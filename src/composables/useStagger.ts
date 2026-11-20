import { ref, watch } from "vue";

/**
 * 列表交错入场：全站列表统一用它，不要各页面各写一套。
 *
 * 用法（两步）：
 *   1) 行容器加 class：`:class="stagger.listClass"`
 *   2) 行容器包住的数据数组作为参数传进来：useStaggerList(() => props.rows)
 *
 * 行为：
 * - 首次渲染：子项按 nth-child 依次延迟入场（CSS 里 .wb-stagger-list 负责）；
 * - 数据刷新（数组换引用）：class 在 is-replay / 普通之间切换 → animation-name 变 → 动画重播，
 *   **不重新挂载 DOM**（挂载会打断 useListAnim 的 FLIP，也会丢卡片内部状态）。
 *
 * 位移动效用独立的 translate 属性，不碰 transform：
 * transform 留给 AutoAnimate 的 FLIP，两者同时写会互相覆盖。
 */
export function useStaggerList(source: () => unknown) {
  const gen = ref(0);

  watch(
    () => source(),
    () => {
      gen.value += 1;
    },
    { flush: "post" },
  );

  function listClass() {
    return ["wb-stagger-list", gen.value % 2 === 1 ? "is-replay" : ""];
  }

  return { gen, listClass };
}

import { ref } from "vue";

/**
 * 顶部加载条的唯一数据源：所有 IPC 请求都经 `lib/api.ts` 的统一入口进来，
 * 这里只统计「在飞请求数」，不做假进度。
 *
 * 为什么不给百分比：一次刷新会并发 4~10 个命令，总数事先不可知，
 * 编一个 0→90% 的插值只是动画，不代表任何真实进展。所以加载中走
 * 不确定态（滑动块），请求清空即收尾、条子回满格。
 */
const inflight = ref(0);
/** 收尾窗口：最后一个请求结束后条子再滑 320ms，避免闪一下就消失。 */
const tail = ref(false);

let tailTimer: number | undefined;

export function barStart() {
  if (tailTimer !== undefined) {
    window.clearTimeout(tailTimer);
    tailTimer = undefined;
  }
  inflight.value += 1;
}

export function barEnd() {
  inflight.value = Math.max(0, inflight.value - 1);
  if (inflight.value > 0) return;
  tail.value = true;
  tailTimer = window.setTimeout(() => {
    tail.value = false;
    tailTimer = undefined;
  }, 320);
}

/** 只读快照，供组件用 computed 订阅。 */
export const loadingBar = { inflight, tail };

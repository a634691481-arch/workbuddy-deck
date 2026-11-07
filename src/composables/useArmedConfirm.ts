/**
 * 两步确认：第一次点击进入「待确认」，超时没再点就自动撤销。
 *
 * 为什么抽出来：账号删除、分组删除的节奏必须一致（都是 8s 回退），
 * 各自抄一份 timer 迟早出现「一处 6s 一处 8s」。模板直接比 `armedId === id`
 * 决定显示确认文案，不额外维护布尔量。
 */
import { onBeforeUnmount, ref } from "vue";

/** 与命令区二次确认同一节奏（AGENTS §0.1）。 */
const DEFAULT_TIMEOUT_MS = 8000;

export function useArmedConfirm(timeoutMs: number = DEFAULT_TIMEOUT_MS) {
  /** 当前处于待确认态的 id；空串 = 没有 */
  const armedId = ref("");
  let timer: ReturnType<typeof setTimeout> | null = null;

  function reset() {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    armedId.value = "";
  }

  /**
   * 同一 id 再次调用 = 用户确认了：清空并返回 false，调用方据此真执行。
   * 其余情况只进入待确认并返回 true（调用方提前 return）。
   * 换 id 必须重置计时，否则 A 的倒计时会把 B 的确认态提前撤掉。
   */
  function arm(id: string): boolean {
    if (armedId.value === id) {
      reset();
      return false;
    }
    if (timer !== null) clearTimeout(timer);
    armedId.value = id;
    timer = setTimeout(reset, timeoutMs);
    return true;
  }

  onBeforeUnmount(reset);

  return { armedId, arm, reset };
}

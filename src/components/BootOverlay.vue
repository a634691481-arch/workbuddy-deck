<script setup lang="ts">
/**
 * 启动动画：覆盖在主界面之上的整窗遮罩，2.8s 后自行淡出并卸载。
 *
 * 为什么不用独立小窗：小窗尺寸与内容尺寸容易不一致（480×300 装不下不同 DPI 下的排版），
 * 且要多维护一个窗口 + 一套跨窗口握手。放在页面里，尺寸天然等于窗口，主界面也早就绘制好了。
 */
import { onMounted, ref } from "vue";

const emit = defineEmits<{ done: [] }>();

const SEGMENTS = 24;
const ACCOUNT_SLOTS = 6;
/** 与 CSS 时间轴对齐：读数约 1.3s 读完，2.45s 起淡出（transition 0.32s），2.85s 卸载。 */
const FADE_AT_MS = 2450;
const END_AT_MS = 2850;

const progress = ref(0);
const slots = ref(0);
const quota = ref("reading");
/** 读数读完（显示 synced / ready）。与「淡出」是两件事，混用会让遮罩提前消失。 */
const loaded = ref(false);
/** 播完离场：淡出 + 卸载。 */
const leaving = ref(false);

onMounted(() => {
  // 读数：数到 6 个槽位后转 synced，让「读完了」有落点（不用随机数假装在加载）
  const step = window.setInterval(() => {
    slots.value += 1;
    if (slots.value >= ACCOUNT_SLOTS) {
      window.clearInterval(step);
      quota.value = "synced";
      loaded.value = true;
    }
  }, 210);

  // 24 段进度：纯节奏，逐个点亮
  const fill = window.setInterval(() => {
    progress.value += 1;
    if (progress.value >= SEGMENTS) window.clearInterval(fill);
  }, 62);

  window.setTimeout(() => (leaving.value = true), FADE_AT_MS);
  window.setTimeout(() => emit("done"), END_AT_MS);
});
</script>

<template>
  <div class="boot" :class="{ out: leaving }">
    <div class="line" />
    <div class="sweep" />

    <div class="panel">
      <div class="head">
        <div class="mark">WD</div>
        <div class="wm">
          WorkBuddy Deck
          <small>multi-account console</small>
        </div>
      </div>

      <div class="rows">
        <div class="row">
          <span>account slot</span>
          <b><i>{{ String(slots).padStart(2, "0") }}</i> / 06</b>
        </div>
        <div class="row">
          <span>quota snapshot</span>
          <b :class="{ ok: loaded }">{{ quota }}</b>
        </div>
        <div class="row">
          <span>rate limit ledger</span>
          <b class="ok">ready</b>
        </div>
      </div>

      <div class="bar">
        <u v-for="i in SEGMENTS" :key="i" :class="{ on: i <= progress }" />
      </div>

      <div class="foot">
        <span>rev 0.1.1</span>
        <span class="st" :class="{ ok: loaded }">{{ loaded ? "ready" : "initialising" }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 与桌面端同一套语言：0 圆角、航空红单一强调、等宽读数 */
.boot {
  position: fixed;
  inset: 0;
  z-index: 100;
  display: grid;
  place-items: center;
  background: #08080a;
  background-image:
    linear-gradient(to right, rgba(255, 255, 255, 0.028) 1px, transparent 1px),
    linear-gradient(to bottom, rgba(255, 255, 255, 0.028) 1px, transparent 1px);
  background-size: 26px 26px;
  transition: opacity 0.32s ease-in;
}
.boot.out {
  opacity: 0;
  pointer-events: none;
}

.line {
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
  height: 2px;
  background: var(--color-hazard);
  transform-origin: left;
  animation: line 0.42s cubic-bezier(0.16, 1, 0.3, 1) both;
}
@keyframes line {
  from {
    transform: scaleX(0);
  }
  to {
    transform: scaleX(1);
  }
}

.sweep {
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
  height: 90px;
  pointer-events: none;
  background: linear-gradient(to bottom, transparent, rgba(225, 55, 43, 0.09), transparent);
  animation: sweep 1s 0.1s ease-in both;
}
@keyframes sweep {
  from {
    transform: translateY(-100px);
    opacity: 0;
  }
  25% {
    opacity: 1;
  }
  to {
    transform: translateY(660px);
    opacity: 0;
  }
}

.panel {
  width: 420px;
  max-width: calc(100% - 64px);
  animation: rise 0.45s 0.08s cubic-bezier(0.16, 1, 0.3, 1) both;
}

.head {
  display: flex;
  align-items: center;
  gap: 12px;
}
.mark {
  width: 34px;
  height: 34px;
  border: 1px solid var(--color-hazard);
  display: grid;
  place-items: center;
  font-family: var(--font-mono, ui-monospace);
  font-size: 13px;
  color: var(--color-hazard);
}
.wm {
  font-family: var(--font-mono, ui-monospace);
  font-size: 14px;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  color: var(--color-foreground);
  line-height: 1.2;
}
.wm small {
  display: block;
  margin-top: 4px;
  font-size: 8.5px;
  letter-spacing: 0.2em;
  color: var(--color-faint);
}

.rows {
  margin-top: 34px;
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  height: 27px;
  border-bottom: 1px solid var(--color-border);
  opacity: 0;
  animation: rise 0.3s cubic-bezier(0.16, 1, 0.3, 1) both;
}
.row:nth-child(1) {
  animation-delay: 0.42s;
}
.row:nth-child(2) {
  animation-delay: 0.64s;
}
.row:nth-child(3) {
  animation-delay: 0.86s;
}
.row span {
  font-family: var(--font-mono, ui-monospace);
  font-size: 9px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--color-faint);
}
.row b {
  font-family: var(--font-mono, ui-monospace);
  font-size: 10px;
  font-weight: 500;
  letter-spacing: 0.12em;
  color: var(--color-muted-foreground);
}
.row b i {
  font-style: normal;
  color: var(--color-foreground);
}
.row b.ok {
  color: var(--color-success);
}

.bar {
  display: flex;
  gap: 2px;
  height: 5px;
  margin-top: 18px;
}
.bar u {
  flex: 1;
  background: var(--color-border);
  transition: background 0.12s linear;
}
.bar u.on {
  background: var(--color-hazard);
}

.foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 12px;
  font-family: var(--font-mono, ui-monospace);
  font-size: 8.5px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--color-faint);
  animation: rise 0.35s 1.5s cubic-bezier(0.16, 1, 0.3, 1) both;
}
.foot .st {
  display: flex;
  align-items: center;
  gap: 6px;
}
.foot .st::before {
  content: "";
  width: 5px;
  height: 5px;
  background: var(--color-faint);
}
.foot .st.ok {
  color: var(--color-success);
}
.foot .st.ok::before {
  background: var(--color-success);
  animation: blink 1s steps(1) infinite;
}
@keyframes blink {
  50% {
    opacity: 0.15;
  }
}

@keyframes rise {
  from {
    opacity: 0;
    transform: translateY(6px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}

@media (prefers-reduced-motion: reduce) {
  .line,
  .sweep,
  .panel,
  .row,
  .foot {
    animation-duration: 0.01s !important;
    animation-delay: 0s !important;
  }
}
</style>

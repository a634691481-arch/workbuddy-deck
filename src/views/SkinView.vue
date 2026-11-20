<script setup lang="ts">
/**
 * 皮肤分区：按 WorkDaddy 的维度接管 WorkBuddy 外观。
 *
 * 模式（浅色/深色/护眼绿/赛博紫/毛玻璃）+ 壁纸 + 模糊度，
 * 全部通过 CDP 注入一段 `<style>`，不改安装目录，重启即还原。
 *
 * 壁纸来自 wallhaven toplist（后端 skin_wallpapers 代理），
 * 缩略图墙点选即设为 config.wallpaper，支持翻页。
 */
import { computed, onMounted, ref, watch } from "vue";
import {
  ChevronLeft,
  ChevronRight,
  ImageOff,
  Loader2,
  Paintbrush,
  RotateCcw,
  Shuffle,
  Upload,
} from "lucide-vue-next";

import {
  skinApply,
  skinRemove,
  skinState,
  skinWallpaperData,
  skinWallpaperUpload,
  skinWallpapers,
  type SkinConfig,
  type SkinMode,
  type SkinWallpaper,
} from "@/lib/api";
import { t } from "@/lib/i18n";

const modes = ref<SkinMode[]>([]);
const config = ref<SkinConfig>({ mode: "dark", wallpaper: "", blur: 18, mask: 0.35 });
const active = ref("");
const busy = ref("");
const note = ref("");
const failed = ref("");

const currentMode = computed(() => modes.value.find((m) => m.id === config.value.mode));

// ── 自动应用：配置一变即注入，无需手动点按钮 ──
// 初始 load() 整体替换 config 不触发；之后任何字段改动（含滑块拖动）都防抖后自动 skinApply。
let applyTimer: ReturnType<typeof setTimeout> | undefined;
let configReady = false;

watch(
  config,
  () => {
    if (!configReady) return;
    clearTimeout(applyTimer);
    applyTimer = setTimeout(apply, 500);
  },
  { deep: true },
);

// ── 壁纸墙 ──
const wallItems = ref<SkinWallpaper[]>([]);
const wallPage = ref(1);
const wallLastPage = ref(1);
const wallLoading = ref(false);
const wallError = ref(false);

async function loadWallpapers(page = wallPage.value) {
  wallLoading.value = true;
  wallError.value = false;
  try {
    const res = await skinWallpapers(page);
    if (res.ok && res.items?.length) {
      wallItems.value = res.items;
      wallPage.value = res.page;
      wallLastPage.value = Math.max(1, res.lastPage ?? res.page);
    } else {
      wallError.value = true;
    }
  } catch {
    wallError.value = true;
  } finally {
    wallLoading.value = false;
  }
}

function wallGo(delta: number) {
  const next = Math.min(wallLastPage.value, Math.max(1, wallPage.value + delta));
  if (next !== wallPage.value) loadWallpapers(next);
}

/** 当前选中的壁纸 id（data URL 太长不适合做比对键）。 */
const selectedWallId = ref("");

/** 点选壁纸：先取原图 data URL（后端下载转码，绕过 CSP），再写入配置触发自动应用。 */
async function pickWallpaper(item: SkinWallpaper) {
  if (!item.url || busy.value === "pick") return;
  busy.value = "pick";
  failed.value = "";
  try {
    const res = await skinWallpaperData(item.url);
    if (res.path) {
      config.value.wallpaper = res.path;
      selectedWallId.value = item.id;
      note.value = "";
    }
  } catch (e) {
    failed.value = String(e);
  } finally {
    busy.value = "";
  }
}

function randomWallpaper() {
  if (!wallItems.value.length) return;
  const item = wallItems.value[Math.floor(Math.random() * wallItems.value.length)];
  pickWallpaper(item);
}

function clearWallpaper() {
  config.value.wallpaper = "";
  selectedWallId.value = "";
}

async function load() {
  try {
    const res = await skinState();
    modes.value = res.modes ?? [];
    config.value = res.config ?? config.value;
    // 兼容旧配置：模式已被移除时回落到浅色
    if (config.value.mode && !modes.value.some((m) => m.id === config.value.mode)) {
      config.value.mode = "light";
    }
  } catch (e) {
    failed.value = String(e);
  }
  loadWallpapers(1);
  await Promise.resolve();
  configReady = true;
}

function pickMode(mode: SkinMode) {
  config.value.mode = mode.id;
  config.value.blur = mode.blur;
}

/** 模式切换按钮：浅色/深色 互换。 */
function toggleMode() {
  const other = modes.value.find((m) => m.id !== config.value.mode);
  if (other) pickMode(other);
}

/** 本地上传壁纸：对话框选图 → data URL → 触发自动应用。 */
async function uploadWallpaper() {
  if (busy.value) return;
  busy.value = "upload";
  failed.value = "";
  try {
    const res = await skinWallpaperUpload();
    if (res.path) {
      config.value.wallpaper = res.path;
      selectedWallId.value = "";
    }
  } catch (e) {
    failed.value = String(e);
  } finally {
    busy.value = "";
  }
}

async function apply() {
  busy.value = "apply";
  failed.value = "";
  note.value = "";
  try {
    const res = await skinApply({ ...config.value });
    active.value = res.ok ? config.value.mode : "";
    note.value = "";
  } catch (e) {
    failed.value = String(e);
  } finally {
    busy.value = "";
  }
}

async function restore() {
  busy.value = "restore";
  failed.value = "";
  note.value = "";
  try {
    await skinRemove();
    active.value = "";
    note.value = "";
  } catch (e) {
    failed.value = String(e);
  } finally {
    busy.value = "";
  }
}

onMounted(load);
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <header
      class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-border bg-panel px-3 py-2 wb-flat"
    >
      <h1 class="flex items-center gap-1.5 text-[13px] font-medium text-foreground">
        <Paintbrush class="size-3.5 text-hazard" />
        {{ t("sk.title") }}
      </h1>
      <div class="flex items-center gap-1.5">
        <!-- 模式切换：浅色/深色 合并成一个按钮，点一下互换 -->
        <button
          type="button"
          class="flex items-center gap-1.5 border px-2 py-1 text-[11px] transition-colors disabled:opacity-40"
          :class="
            config.mode !== 'dark'
              ? 'border-hazard bg-hazard-soft text-foreground'
              : 'border-border-strong text-muted-foreground hover:text-foreground'
          "
          :disabled="busy !== ''"
          :title="t('sk.mode')"
          @click="toggleMode"
        >
          <i
            class="size-2 shrink-0 rounded-full"
            :style="{ background: currentMode?.accent ?? '#2f6fed' }"
          />
          {{ currentMode?.label ?? t("sk.mode") }}
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 border border-border-strong px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
          :disabled="busy !== ''"
          @click="restore"
        >
          <Loader2 v-if="busy === 'restore'" class="size-3 animate-spin" />
          <RotateCcw v-else class="size-3" />
          {{ t("sk.restore") }}
        </button>
      </div>
    </header>

    <!-- 滚动区：只有壁纸墙 -->
    <div class="min-h-0 flex-1 overflow-y-auto p-2.5">
      <!-- 壁纸墙（wallhaven toplist） -->
      <section class="border border-border bg-card px-2.5 py-2">
        <div class="mb-1.5 flex items-center justify-between gap-2">
          <span class="lbl">{{ t("sk.wallpaper") }}</span>
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="flex items-center gap-1 text-[10px] text-muted-foreground hover:text-hazard disabled:opacity-40"
              :disabled="busy === 'pick'"
              @click="uploadWallpaper"
            >
              <Loader2 v-if="busy === 'upload'" class="size-3 animate-spin" />
              <Upload v-else class="size-3" />
              {{ t("sk.upload") }}
            </button>
            <button
              v-if="config.wallpaper"
              type="button"
              class="flex items-center gap-1 text-[10px] text-faint hover:text-danger"
              @click="clearWallpaper"
            >
              <ImageOff class="size-3" />
            </button>
            <button
              type="button"
              class="flex items-center gap-1 text-[10px] text-muted-foreground hover:text-hazard disabled:opacity-40"
              :disabled="wallItems.length === 0 || busy === 'pick'"
              @click="randomWallpaper"
            >
              <Shuffle class="size-3" />
              {{ t("sk.random") }}
            </button>
          </div>
        </div>

        <!-- 翻页 -->
        <div class="mb-1.5 flex items-center justify-between text-[10px] text-faint">
          <button
            type="button"
            class="flex items-center gap-0.5 hover:text-foreground disabled:opacity-30"
            :disabled="wallLoading || wallPage <= 1"
            @click="wallGo(-1)"
          >
            <ChevronLeft class="size-3" />
          </button>
          <span class="tabular-nums">{{ wallPage }} / {{ wallLastPage }}</span>
          <button
            type="button"
            class="flex items-center gap-0.5 hover:text-foreground disabled:opacity-30"
            :disabled="wallLoading || wallPage >= wallLastPage"
            @click="wallGo(1)"
          >
            <ChevronRight class="size-3" />
          </button>
        </div>

        <!-- 缩略图：自然高度瀑布流，不裁切成固定比例 -->
        <div v-if="wallError" class="py-3 text-center text-[10px] text-danger">
          {{ t("sk.wallFailed") }}
          <button
            type="button"
            class="ml-1 underline hover:text-foreground"
            @click="loadWallpapers(wallPage)"
          >
            {{ t("sk.wallRetry") }}
          </button>
        </div>
        <div v-else-if="wallLoading && !wallItems.length" class="grid grid-cols-3 gap-1.5">
          <div
            v-for="i in 9"
            :key="i"
            class="aspect-video animate-pulse bg-border"
          />
        </div>
        <div v-else class="columns-2 gap-1.5 min-[480px]:columns-3">
          <button
            v-for="item in wallItems"
            :key="item.id"
            type="button"
            class="relative mb-1.5 block w-full overflow-hidden border transition-colors break-inside-avoid"
            :class="
              selectedWallId === item.id
                ? 'border-hazard'
                : 'border-border hover:border-border-strong'
            "
            :title="`${item.id} · ${item.resolution ?? ''}`"
            :disabled="busy === 'pick'"
            @click="pickWallpaper(item)"
          >
            <img
              v-if="item.thumb"
              :src="item.thumb"
              :alt="item.id"
              loading="lazy"
              class="h-auto w-full"
            />
            <div v-else class="flex h-16 w-full items-center justify-center bg-border text-faint">
              <ImageOff class="size-3.5" />
            </div>
            <Loader2
              v-if="busy === 'pick'"
              class="absolute right-1 top-1 size-3 animate-spin text-white drop-shadow"
            />
            <span
              v-else-if="selectedWallId === item.id"
              class="absolute right-0.5 top-0.5 bg-hazard px-1 text-[8px] leading-3.5 text-white"
            >
              ✓
            </span>
          </button>
        </div>

        <p v-if="selectedWallId" class="mt-1 truncate text-[9px] text-faint">
          {{ t("sk.wallPicked") }} {{ selectedWallId }}
        </p>
      </section>
    </div>

    <!-- 固定底栏：配色模式 / 模糊 / 压暗，不随壁纸滚动 -->
    <div class="shrink-0 space-y-2 border-t border-border bg-panel px-2.5 py-2 wb-flat">
      <!-- 模糊 + 压暗并排，省高度 -->
      <div class="grid grid-cols-2 gap-2">
        <section>
          <div class="mb-0.5 flex items-center justify-between">
            <span class="lbl">{{ t("sk.blur") }}</span>
            <span class="text-[10px]" :class="config.blur > 0 ? 'text-hazard' : 'text-faint'">
              {{ config.blur }}px
            </span>
          </div>
          <input
            v-model.number="config.blur"
            type="range"
            min="0"
            max="40"
            step="2"
            class="w-full accent-hazard"
            :title="t('sk.blurHint')"
          />
        </section>
        <section>
          <div class="mb-0.5 flex items-center justify-between">
            <span class="lbl">{{ t("sk.mask") }}</span>
            <span class="text-[10px]" :class="config.mask > 0 ? 'text-hazard' : 'text-faint'">
              {{ Math.round(config.mask * 100) }}%
            </span>
          </div>
          <input
            v-model.number="config.mask"
            type="range"
            min="0"
            max="0.8"
            step="0.05"
            class="w-full accent-hazard"
            :title="t('sk.maskHint')"
          />
        </section>
      </div>

      <p
        v-if="failed"
        class="border border-danger bg-danger-soft px-2 py-1 text-[10px] leading-4 text-danger"
      >
        {{ failed }}
      </p>
    </div>
  </div>
</template>

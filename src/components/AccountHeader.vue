<script setup lang="ts">
// 当前单元信息条：账号标识 + 连签 + 今日可得 + 签到命令。
// 签到态与余额已由 TelemetryBar 承担，这里不重复。
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import type { AccountDetail } from "@/lib/api";
import { t } from "@/lib/i18n";

const props = defineProps<{
  detail: AccountDetail | null;
  accountLabel: string;
  hasAccount: boolean;
  signing: boolean;
  disabled: boolean;
}>();

const emit = defineEmits<{ signin: []; growth: [] }>();

const rawNumber = (key: string): number | string => {
  const value = props.detail?.checkin.raw?.[key];
  return typeof value === "number" ? value : "—";
};

const checkedIn = () => props.detail?.checkin.todayCheckedIn === true;
const checkinError = () =>
  props.detail && props.detail.checkin.ok !== true
    ? (props.detail.checkin.error ?? "签到状态读取失败")
    : "";
</script>

<template>
  <section class="flex h-10 shrink-0 items-stretch border border-border bg-card">
    <div class="flex items-center gap-2 border-r border-border px-2">
      <span class="lbl text-hazard">[</span>
      <span class="lbl lbl-ink">{{ t("ah.unit") }}</span>
      <span class="lbl text-hazard">]</span>
    </div>

    <template v-if="props.hasAccount">
      <div class="flex min-w-0 flex-1 items-center gap-2.5 px-2.5">
        <span class="max-w-56 truncate text-[12px] text-foreground" :title="props.accountLabel">
          {{ props.accountLabel }}
        </span>
        <Badge v-if="props.detail" :tone="checkedIn() ? 'success' : 'warning'">
          {{ checkedIn() ? t("ah.signed") : t("ah.unsigned") }}
        </Badge>
        <span v-else class="lbl text-faint">READING…</span>

        <span class="lbl text-faint">{{ t("ah.streak") }}</span>
        <span class="readout text-[11px] text-foreground">{{ rawNumber("continuous_days") }}D</span>

        <span class="lbl text-faint">{{ t("ah.today") }}</span>
        <span class="readout text-[11px] text-foreground">+{{ rawNumber("credit") }}</span>

        <span
          v-if="checkinError()"
          class="truncate text-[10px] text-hazard"
          :title="checkinError()"
        >
          ⚠ {{ checkinError() }}
        </span>
      </div>

      <Button variant="outline" size="sm" class="my-1.5 mr-2" @click="emit('growth')">
        {{ t("gr.entry") }}
      </Button>

      <Button
        variant="outline"
        size="sm"
        class="my-1.5 mr-2"
        :disabled="props.disabled"
        @click="emit('signin')"
      >
        {{ props.signing ? t("ah.signing") : t("ah.signIn") }}
      </Button>
    </template>

    <div v-else class="flex flex-1 items-center px-2.5">
      <span class="lbl text-faint">» {{ t("ah.noUnit") }}</span>
    </div>
  </section>
</template>

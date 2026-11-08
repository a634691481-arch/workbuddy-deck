// 成长中心取数：徽章 / 盲盒额度 / 领取。
// 编排层是唯一允许直接调 lib/api 的地方（AGENTS.md §0）；视图只拿状态与方法。
import { ref } from "vue";

import { growthAssets, type BuddyQuota, type GrowthBadge } from "@/lib/api";
import { t } from "@/lib/i18n";

export function useGrowthCenter() {
  const open = ref(false);
  const loading = ref(false);
  const error = ref("");
  const badges = ref<GrowthBadge[]>([]);
  const quota = ref<BuddyQuota | null>(null);
  const claiming = ref(false);

  /** 请求序号：快速切账号时丢弃过期响应，避免旧账号数据覆盖新账号。 */
  let seq = 0;
  /** 这些数据属于哪个账号：切号时先清空，别让上一号的徽章留在屏幕上。 */
  const owner = ref<string | null>(null);

  async function refresh(accountId: string | null) {
    const mine = ++seq;
    loading.value = true;
    error.value = "";
    if (accountId && accountId !== owner.value) {
      badges.value = [];
      quota.value = null;
    }
    try {
      const result = await growthAssets(accountId ?? undefined);
      if (mine !== seq) return;
      owner.value = accountId;
      badges.value = result.badges ?? [];
      quota.value = result.quota ?? null;
      if (result.ok !== true) error.value = result.error || t("gr.error");
    } catch (cause) {
      if (mine !== seq) return;
      owner.value = null;
      badges.value = [];
      quota.value = null;
      error.value = String(cause);
    } finally {
      if (mine === seq) loading.value = false;
    }
  }

  function openGrowth(accountId: string | null) {
    open.value = true;
    void refresh(accountId);
  }

  /** 领取 = 由视图注入的批量「成长」动作，跑完重拉徽章与额度（领取会改这两块）。 */
  async function claim(accountId: string | null, runner: (id: string) => Promise<unknown>) {
    if (!accountId || claiming.value) return;
    claiming.value = true;
    try {
      await runner(accountId);
      await refresh(accountId);
    } finally {
      claiming.value = false;
    }
  }

  return { open, loading, error, badges, quota, claiming, owner, openGrowth, refresh, claim };
}

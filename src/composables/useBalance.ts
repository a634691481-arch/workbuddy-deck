// 积分取数：单账号余额（切号即查）+ 全账号总额（刷新时查一次）。
// 分开是为了不让全量汇总（每号 3 个请求）拖慢切号。
import { ref } from "vue";

import { accountBalance, totalBalance, type Balance } from "@/lib/api";

export function useBalance() {
  const balance = ref<Balance | null>(null);
  const loading = ref(false);

  const total = ref<number | null>(null);
  const totalLoading = ref(false);
  const totalFailed = ref(0);

  /** 请求序号：快速切账号时丢弃过期响应，避免旧账号余额覆盖新账号。 */
  let seq = 0;

  async function load(accountId: string | null) {
    if (!accountId) {
      balance.value = null;
      return;
    }
    const mine = ++seq;
    loading.value = true;
    try {
      const res = await accountBalance(accountId);
      if (mine === seq) balance.value = res;
    } catch (error) {
      // 余额失败不影响主流程，显示占位符即可。
      if (mine === seq) balance.value = { ok: false, error: String(error) };
    } finally {
      if (mine === seq) loading.value = false;
    }
  }

  async function loadTotal() {
    totalLoading.value = true;
    try {
      const res = await totalBalance();
      total.value = res.ok ? res.total : null;
      totalFailed.value = res.failed ?? 0;
    } catch {
      total.value = null;
      totalFailed.value = 0;
    } finally {
      totalLoading.value = false;
    }
  }

  return { balance, loading, load, total, totalLoading, totalFailed, loadTotal };
}

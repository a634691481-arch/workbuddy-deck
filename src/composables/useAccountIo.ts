// 账号导入/导出：文件对话框在 Rust 侧选路径，读写也由后端完成，
// 前端只负责发起与结果播报（令牌不经过前端，AGENTS.md §5）。
import { open, save } from "@tauri-apps/plugin-dialog";

import * as api from "@/lib/api";

const FILTERS = [{ name: "JSON", extensions: ["json"] }];

export function useAccountIo(options: {
  log: (text: string, level?: "info" | "ok" | "err") => void;
  onImported: () => Promise<void>;
}) {
  /** 导出：文件含明文令牌，必须在日志里明确告警。 */
  async function exportAccounts(accountIds: string[] = []) {
    try {
      const stamp = new Date().toISOString().slice(0, 10);
      const path = await save({
        title: "导出账号",
        defaultPath: `wb-accounts-${stamp}.json`,
        filters: FILTERS,
      });
      if (!path) return;
      const res = await api.exportAccountsFile(path, accountIds);
      if (res.ok) {
        options.log(`已导出 ${res.count ?? 0} 个账号到 ${res.path}（文件含明文令牌，请妥善保管）`, "ok");
      } else {
        options.log(`导出失败：${res.error ?? "未知原因"}`, "err");
      }
    } catch (error) {
      options.log(`导出异常：${String(error)}`, "err");
    }
  }

  /** 导入：core 按 uid 合并，同号覆盖、未见过的新增。 */
  async function importAccounts() {
    try {
      const picked = await open({ title: "导入账号", multiple: false, filters: FILTERS });
      if (!picked || Array.isArray(picked)) return;

      const preview = await api.previewImportFile(picked);
      if (!preview.ok) {
        options.log(`读取失败：${preview.error ?? "文件不可用"}`, "err");
        return;
      }

      const res = await api.importAccountsFile(picked);
      if (!res.ok) {
        options.log(`导入失败：${res.error ?? "未知原因"}`, "err");
        return;
      }
      options.log(
        `导入完成：合入 ${res.imported ?? 0} 个（覆盖 ${res.overwritten ?? 0}，跳过 ${res.skipped ?? 0}）`,
        "ok",
      );
      await options.onImported();
    } catch (error) {
      options.log(`导入异常：${String(error)}`, "err");
    }
  }

  return { exportAccounts, importAccounts };
}

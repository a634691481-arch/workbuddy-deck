/**
 * 账号分组与标签：AccountsView（编辑入口）与 TasksView（按组勾选）共享同一份状态。
 * 模块级 ref，谁先挂载谁加载；写操作后本地同步，避免整页重拉。
 */
import { computed, ref } from "vue";

import {
  accountGroupRemove,
  accountGroupSave,
  accountGroupsList,
  accountMemberSave,
  type AccountGroupDef,
  type AccountMemberDef,
} from "@/lib/api";

const groups = ref<AccountGroupDef[]>([]);
const members = ref<Record<string, AccountMemberDef>>({});
const loaded = ref(false);
const error = ref("");

async function load(force = false) {
  if (loaded.value && !force) return;
  const result = await accountGroupsList();
  if (!result.ok) {
    error.value = result.error ?? "";
    return;
  }
  groups.value = result.groups ?? [];
  members.value = result.members ?? {};
  error.value = "";
  loaded.value = true;
}

const EMPTY_MEMBER: AccountMemberDef = { groups: [], tags: [] };

export function useAccountGroups() {
  /** 账号 id → 归属与标签（无记录给空壳，模板少判空） */
  const memberOf = (accountId: string): AccountMemberDef => members.value[accountId] ?? EMPTY_MEMBER;
  /** 分组 id → 名称 */
  const groupNameOf = (id: string) => groups.value.find((g) => g.id === id)?.name ?? "";
  /** 分组 id → 成员账号 id 列表（按组勾选 / 计数用） */
  const groupMemberIds = computed(() => {
    const map = new Map<string, string[]>();
    for (const group of groups.value) map.set(group.id, []);
    for (const [accountId, member] of Object.entries(members.value)) {
      for (const groupId of member.groups) {
        map.get(groupId)?.push(accountId);
      }
    }
    return map;
  });

  async function saveGroup(id: string | null, name: string, excludeAutoSwitch: boolean) {
    const result = await accountGroupSave(id, name, excludeAutoSwitch);
    await load(true);
    return result;
  }

  async function removeGroup(id: string) {
    await accountGroupRemove(id);
    await load(true);
  }

  async function setMember(accountId: string, groupIds: string[], tags: string[]) {
    const result = await accountMemberSave(accountId, groupIds, tags);
    await load(true);
    return result;
  }

  return { groups, members, error, load, memberOf, groupNameOf, groupMemberIds, saveGroup, removeGroup, setMember };
}

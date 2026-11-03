<script setup lang="ts">
import { computed, ref } from "vue";
import {
  NButton,
  NDropdown,
  NIcon,
  NInput,
  useDialog,
  useMessage,
} from "naive-ui";
import type { DropdownOption } from "naive-ui";
import { EllipsisHorizontalOutline, SearchOutline } from "@vicons/ionicons5";
import { commands } from "../bindings";
import type { ConnectionProfile } from "../bindings";
import { useConnectionStore } from "../stores/connections";
import { backendLabel, errText, hostOf, presetColor, presetLabel } from "../utils/format";
import { unwrap } from "../utils/ipc";

const store = useConnectionStore();
const message = useMessage();
const dialog = useDialog();

const keyword = ref("");

const filtered = computed(() => {
  const k = keyword.value.trim().toLowerCase();
  if (!k) return store.profiles;
  return store.profiles.filter((p) =>
    `${p.name} ${p.endpoint}`.toLowerCase().includes(k),
  );
});

function options(p: ConnectionProfile): DropdownOption[] {
  return [
    { label: "测试连接", key: "test" },
    { label: "设为默认", key: "default", disabled: p.isDefault },
    { label: "编辑", key: "edit" },
    { type: "divider", key: "d1" },
    { label: "删除", key: "delete" },
  ];
}

async function onAction(key: string | number, p: ConnectionProfile) {
  if (key === "test") {
    const m = message.loading("正在测试连接…", { duration: 0 });
    try {
      const r = unwrap(await commands.testSavedConnection(p.id));
      m.type = "success";
      m.content = `连接成功 · ${r.latencyMs}ms · ${backendLabel(r.backend)} · ${r.bucketCount} 个桶`;
      m.duration = 5000;
    } catch (e) {
      m.type = "error";
      m.content = errText(e);
      m.duration = 5000;
    }
  } else if (key === "default") {
    try {
      unwrap(await commands.setDefaultConnection(p.id));
      await store.refreshProfiles();
      message.success("已设为默认连接");
    } catch (e) {
      message.error(errText(e));
    }
  } else if (key === "edit") {
    store.openEdit(p);
  } else if (key === "delete") {
    dialog.warning({
      title: "删除连接",
      content: `确定删除连接「${p.name}」？将同时清除系统凭据存储中的密钥。`,
      positiveText: "删除",
      negativeText: "取消",
      onPositiveClick: async () => {
        try {
          unwrap(await commands.deleteConnection(p.id));
          await store.refreshProfiles();
          if (store.activeId === p.id) store.select(null);
          message.success("连接已删除");
        } catch (e) {
          message.error(errText(e));
        }
      },
    });
  }
}
</script>

<template>
  <aside class="sidebar">
    <div class="side-head">
      <span class="side-title">连接</span>
      <span class="side-count">{{ store.profiles.length }}</span>
    </div>

    <div class="side-search">
      <NInput v-model:value="keyword" size="small" placeholder="搜索连接" clearable>
        <template #prefix>
          <NIcon size="13" color="#909399"><SearchOutline /></NIcon>
        </template>
      </NInput>
    </div>

    <div class="conn-list">
      <div
        v-for="p in filtered"
        :key="p.id"
        class="conn-item"
        :class="{ active: p.id === store.activeId }"
        @click="store.select(p.id)"
      >
        <span class="dot" :style="{ background: presetColor(p.preset) }" />
        <div class="conn-info">
          <div class="conn-name">
            {{ p.name }}
            <span v-if="p.isDefault" class="default-tag">默认</span>
          </div>
          <div class="conn-ep">
            <span class="conn-preset">{{ presetLabel(p.preset) }}</span>
            {{ hostOf(p.endpoint) }}
          </div>
        </div>
        <NDropdown
          trigger="click"
          :options="options(p)"
          @select="(key: string | number) => onAction(key, p)"
        >
          <NButton quaternary size="tiny" class="conn-more" @click.stop>
            <template #icon>
              <NIcon size="14"><EllipsisHorizontalOutline /></NIcon>
            </template>
          </NButton>
        </NDropdown>
      </div>
      <div v-if="store.loaded && filtered.length === 0" class="conn-empty">
        {{ store.profiles.length === 0 ? "还没有连接" : "无匹配结果" }}
      </div>
    </div>

    <div class="side-foot">
      <span
        class="foot-dot"
        :style="{ background: store.activeProfile ? '#18a058' : '#c0c4cc' }"
      />
      <span class="foot-text">
        {{ store.activeProfile ? hostOf(store.activeProfile.endpoint) : "未连接" }}
      </span>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  width: 232px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background: #ffffff;
  border-right: 1px solid #e5e7eb;
}

.side-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px 6px;
}

.side-title {
  font-size: 12px;
  color: #909399;
  font-weight: 600;
}

.side-count {
  font-size: 11px;
  color: #c0c4cc;
}

.side-search {
  padding: 4px 10px 8px;
}

.conn-list {
  flex: 1;
  overflow-y: auto;
  padding: 0 6px;
}

.conn-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 8px;
  border-radius: 6px;
  cursor: pointer;
  position: relative;
}

.conn-item:hover {
  background: #f4f5f7;
}

.conn-item.active {
  background: #e8f5ee;
}

.conn-item.active::before {
  content: "";
  position: absolute;
  left: 0;
  top: 6px;
  bottom: 6px;
  width: 2px;
  border-radius: 1px;
  background: #18a058;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.conn-info {
  flex: 1;
  min-width: 0;
}

.conn-name {
  font-size: 13px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.default-tag {
  font-size: 10px;
  color: #18a058;
  background: rgba(24, 160, 88, 0.1);
  border-radius: 3px;
  padding: 0 4px;
  margin-left: 4px;
  vertical-align: 1px;
}

.conn-ep {
  font-size: 11px;
  color: #909399;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.conn-preset {
  color: #606266;
  margin-right: 4px;
}

.conn-preset::after {
  content: "·";
  margin-left: 4px;
  color: #c0c4cc;
}

.conn-more {
  opacity: 0;
  flex-shrink: 0;
}

.conn-item:hover .conn-more {
  opacity: 1;
}

.conn-empty {
  padding: 24px 0;
  text-align: center;
  font-size: 12px;
  color: #c0c4cc;
}

.side-foot {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  border-top: 1px solid #f0f1f3;
  font-size: 11px;
  color: #909399;
}

.foot-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
}

.foot-text {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>

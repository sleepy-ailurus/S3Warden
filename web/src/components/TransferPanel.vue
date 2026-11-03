<script setup lang="ts">
import { computed } from "vue";
import {
  NBadge,
  NButton,
  NDrawer,
  NDrawerContent,
  NEmpty,
  NIcon,
  NProgress,
  NTag,
  NTooltip,
} from "naive-ui";
import {
  ArrowDownOutline,
  ArrowUpOutline,
  CheckmarkDoneOutline,
  CloseCircleOutline,
  RefreshOutline,
} from "@vicons/ionicons5";
import { useTransferStore, type TransferTask } from "../stores/transfers";
import { formatSize } from "../utils/format";

const store = useTransferStore();

/** 传输面板浮动入口的计数：进行中任务优先展示，否则显示已完成数。 */
const badgeCount = computed(() =>
  store.activeCount > 0 ? store.activeCount : store.finishedCount,
);

function statusTag(t: TransferTask) {
  switch (t.status) {
    case "queued":
      return { text: "排队中", type: "default" as const };
    case "running":
      return { text: "传输中", type: "info" as const };
    case "completed":
      return { text: "已完成", type: "success" as const };
    case "failed":
      return { text: "失败", type: "error" as const };
    case "cancelled":
      return { text: "已取消", type: "warning" as const };
  }
}

function percent(t: TransferTask) {
  if (t.status === "completed") return 100;
  if (t.total <= 0) return 0;
  return Math.min(100, Math.round((t.transferred / t.total) * 100));
}

function progressText(t: TransferTask) {
  if (t.status === "completed") return formatSize(t.total);
  const done = formatSize(t.transferred);
  if (t.total > 0) return `${done} / ${formatSize(t.total)}`;
  return done;
}

function speedText(t: TransferTask) {
  if (t.status !== "running" || t.speed <= 0) return "";
  return `${formatSize(t.speed)}/s`;
}
</script>

<template>
  <!-- 浮动入口：右下角徽标按钮 -->
  <button class="transfer-fab" type="button" @click="store.panelVisible = true">
    <NBadge :value="badgeCount" :max="99" :show="badgeCount > 0" type="info">
      <NIcon :size="20" color="#18a058">
        <ArrowUpOutline v-if="store.taskList.some((t) => t.kind === 'upload' && t.status === 'running')" />
        <ArrowDownOutline v-else />
      </NIcon>
    </NBadge>
  </button>

  <NDrawer v-model:show="store.panelVisible" :width="420" placement="right">
    <NDrawerContent title="传输" closable>
      <template #footer>
        <NButton size="small" quaternary :disabled="store.finishedCount === 0" @click="store.clearFinished()">
          <template #icon>
            <NIcon><CheckmarkDoneOutline /></NIcon>
          </template>
          清除已完成
        </NButton>
      </template>

      <NEmpty
        v-if="store.taskList.length === 0"
        description="暂无传输任务"
        style="margin-top: 80px"
      />

      <div v-else class="task-list">
        <div v-for="t in store.taskList" :key="t.taskId" class="task">
          <div class="task-head">
            <NIcon
              :size="15"
              :color="t.kind === 'upload' ? '#18a058' : '#2080f0'"
              style="flex-shrink: 0"
            >
              <ArrowUpOutline v-if="t.kind === 'upload'" />
              <ArrowDownOutline v-else />
            </NIcon>
            <span class="task-name" :title="t.localPath || t.key">{{ t.name }}</span>
            <NTag :type="statusTag(t).type" size="small" bordered round>
              {{ statusTag(t).text }}
            </NTag>
          </div>

          <NProgress
            type="line"
            :percentage="percent(t)"
            :show-indicator="false"
            :status="t.status === 'failed' ? 'error' : 'default'"
            :height="6"
            border-radius="3px"
            style="margin: 6px 0 4px"
          />

          <div class="task-foot">
            <span class="task-info">
              {{ progressText(t) }}
              <template v-if="speedText(t)"> · {{ speedText(t) }}</template>
            </span>
            <span class="task-actions">
              <NTooltip v-if="t.status === 'failed' || t.status === 'cancelled'">
                <template #trigger>
                  <NButton size="tiny" quaternary @click="store.retry(t.taskId)">
                    <template #icon>
                      <NIcon><RefreshOutline /></NIcon>
                    </template>
                  </NButton>
                </template>
                重试（支持断点续传）
              </NTooltip>
              <NButton
                v-if="t.status === 'running' || t.status === 'queued'"
                size="tiny"
                quaternary
                type="error"
                @click="store.cancel(t.taskId)"
              >
                <template #icon>
                  <NIcon><CloseCircleOutline /></NIcon>
                </template>
              </NButton>
            </span>
          </div>

          <div v-if="t.error" class="task-error">{{ t.error }}</div>
        </div>
      </div>
    </NDrawerContent>
  </NDrawer>
</template>

<style scoped>
.transfer-fab {
  position: fixed;
  right: 20px;
  bottom: 20px;
  z-index: 900;
  width: 44px;
  height: 44px;
  border-radius: 50%;
  border: 1px solid #e5e7eb;
  background: #ffffff;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.08);
  transition: box-shadow 0.15s ease;
}

.transfer-fab:hover {
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.14);
}

.task-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.task {
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  padding: 10px 12px;
}

.task-head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.task-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
  color: #303236;
}

.task-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.task-info {
  font-size: 12px;
  color: #909399;
}

.task-actions {
  display: flex;
  align-items: center;
  gap: 4px;
}

.task-error {
  margin-top: 6px;
  font-size: 12px;
  color: #d03050;
  word-break: break-all;
}
</style>

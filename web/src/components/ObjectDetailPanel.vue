<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  NButton,
  NDivider,
  NIcon,
  NInput,
  NScrollbar,
  NSpace,
  NText,
  useMessage,
} from "naive-ui";
import {
  CheckmarkOutline,
  CloseOutline,
  CloudDownloadOutline,
  CreateOutline,
  DocumentOutline,
  EyeOutline,
  PricetagsOutline,
  ShareSocialOutline,
  TrashOutline,
} from "@vicons/ionicons5";
import { ask, save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { commands } from "../bindings";
import type { ObjectDetail, ObjectEntry } from "../bindings";
import { useConnectionStore } from "../stores/connections";
import { extOf, typeLabel } from "../utils/filetype";
import { errText, formatSize, formatTime } from "../utils/format";
import { unwrap } from "../utils/ipc";
import PreviewModal from "./PreviewModal.vue";
import ShareModal from "./ShareModal.vue";
import TagModal from "./TagModal.vue";

const props = defineProps<{ entry: ObjectEntry }>();
const emit = defineEmits<{ (e: "close"): void }>();

const store = useConnectionStore();
const message = useMessage();

const detail = ref<ObjectDetail | null>(null);
const detailLoading = ref(false);
const detailError = ref<string | null>(null);
const acting = ref(false);

// ---- 重命名内联编辑 ----
const renaming = ref(false);
const renameValue = ref("");

// ---- 预览 / 分享弹窗 ----
const previewVisible = ref(false);
const shareVisible = ref(false);
const tagVisible = ref(false);

const connId = computed(() => store.activeId ?? "");
const bucket = computed(() => store.currentBucket ?? "");
const key = computed(() => props.entry.key);

const displayName = computed(() => props.entry.name);

/** 文件统一用中性色文件图标（不按类型着色，避免视觉噪音）。 */
const FILE_ICON_COLOR = "#8a9099";

async function loadDetail() {
  detailLoading.value = true;
  detailError.value = null;
  try {
    detail.value = unwrap(
      await commands.statObject(connId.value, bucket.value, key.value),
    );
  } catch (e) {
    detailError.value = e instanceof Error ? e.message : String(e);
  } finally {
    detailLoading.value = false;
  }
}

watch(
  () => props.entry.key,
  () => {
    detail.value = null;
    renaming.value = false;
    void loadDetail();
  },
  { immediate: true },
);

/** 属性行：值为空则不展示（避免面板里出现一排「—」）。 */
const metaRows = computed(() => {
  const d = detail.value;
  const rows: { label: string; value: string; mono?: boolean }[] = [
    { label: "类型", value: typeLabel(displayName.value) },
  ];
  if (d?.contentType) rows.push({ label: "Content-Type", value: d.contentType });
  rows.push({ label: "大小", value: formatSize(d?.size ?? props.entry.size) });
  if (d?.lastModified || props.entry.lastModified) {
    rows.push({
      label: "修改时间",
      value: formatTime(d?.lastModified ?? props.entry.lastModified ?? null),
    });
  }
  if (d?.storageClass) rows.push({ label: "存储类型", value: d.storageClass });
  if (d?.etag) rows.push({ label: "ETag", value: d.etag, mono: true });
  if (extOf(displayName.value)) {
    rows.push({ label: "扩展名", value: `.${extOf(displayName.value)}` });
  }
  return rows;
});

// ---- 下载（入传输队列，进度见传输面板）----
async function download() {
  const dest = await save({
    defaultPath: displayName.value,
    title: "下载到…",
  });
  if (!dest) return;
  try {
    unwrap(
      await commands.startDownload(connId.value, bucket.value, key.value, dest),
    );
    lastDownloadPath.value = dest;
    message.success("已加入下载队列，可在右下角传输面板查看进度");
  } catch (e) {
    message.error(errText(e));
  }
}

const lastDownloadPath = ref<string | null>(null);

function revealDownload() {
  if (!lastDownloadPath.value) return;
  void revealItemInDir(lastDownloadPath.value).catch(() => {
    message.warning("无法打开所在目录");
  });
}

// ---- 重命名 ----
function startRename() {
  renameValue.value = displayName.value;
  renaming.value = true;
}

function cancelRename() {
  renaming.value = false;
}

async function submitRename() {
  const name = renameValue.value.trim();
  if (!name || name === displayName.value) {
    renaming.value = false;
    return;
  }
  acting.value = true;
  try {
    unwrap(
      await commands.renameObject(connId.value, bucket.value, key.value, name),
    );
    message.success("已重命名");
    renaming.value = false;
    emit("close");
    await store.reloadObjects();
  } catch (e) {
    message.error(errText(e));
  } finally {
    acting.value = false;
  }
}

// ---- 删除 ----
async function remove() {
  const ok = await ask(
    `确定删除「${displayName.value}」？该操作不可撤销。`,
    { title: "删除对象", kind: "warning" },
  );
  if (!ok) return;
  acting.value = true;
  try {
    unwrap(await commands.deleteObject(connId.value, bucket.value, key.value));
    message.success("已删除");
    emit("close");
    await store.reloadObjects();
  } catch (e) {
    message.error(errText(e));
  } finally {
    acting.value = false;
  }
}
</script>

<template>
  <div class="panel">
    <div class="panel-head">
      <NIcon size="20" :color="FILE_ICON_COLOR" class="head-icon">
        <DocumentOutline />
      </NIcon>
      <div class="title" :title="displayName">{{ displayName }}</div>
      <NButton size="tiny" quaternary @click="emit('close')">
        <template #icon>
          <NIcon><CloseOutline /></NIcon>
        </template>
      </NButton>
    </div>

    <NScrollbar class="panel-body">
      <div class="key-line" :title="key">{{ key }}</div>

      <div class="meta">
        <div v-for="row in metaRows" :key="row.label" class="meta-row">
          <span class="meta-label">{{ row.label }}</span>
          <span class="meta-value" :class="{ mono: row.mono }" :title="row.value">
            {{ row.value }}
          </span>
        </div>
        <div v-if="detailLoading && !detail" class="meta-hint">加载中…</div>
        <div v-else-if="detailError" class="meta-hint err">{{ detailError }}</div>
      </div>

      <div class="section-title">操作</div>
      <div class="actions">
        <button class="action" :disabled="acting" @click="download()">
          <NIcon size="16"><CloudDownloadOutline /></NIcon>
          <span>下载</span>
        </button>
        <button class="action" :disabled="acting" @click="shareVisible = true">
          <NIcon size="16"><ShareSocialOutline /></NIcon>
          <span>分享</span>
        </button>
        <button class="action" :disabled="acting" @click="previewVisible = true">
          <NIcon size="16"><EyeOutline /></NIcon>
          <span>预览</span>
        </button>
        <button class="action" :disabled="acting" @click="tagVisible = true">
          <NIcon size="16"><PricetagsOutline /></NIcon>
          <span>标签</span>
        </button>
        <button class="action" :disabled="acting" @click="startRename()">
          <NIcon size="16"><CreateOutline /></NIcon>
          <span>修改名称</span>
        </button>
      </div>

      <div v-if="renaming" class="rename">
        <NInput
          v-model:value="renameValue"
          size="small"
          placeholder="新名称"
          @keydown.enter="submitRename()"
        />
        <NSpace :size="6">
          <NButton size="tiny" type="primary" :loading="acting" @click="submitRename()">
            <template #icon>
              <NIcon><CheckmarkOutline /></NIcon>
            </template>
            保存
          </NButton>
          <NButton size="tiny" quaternary @click="cancelRename()">取消</NButton>
        </NSpace>
      </div>

      <NDivider style="margin: 18px 10px 14px" />

      <button class="action danger" :disabled="acting" @click="remove()">
        <NIcon size="16"><TrashOutline /></NIcon>
        <span>删除</span>
      </button>

      <div v-if="lastDownloadPath" class="dl-hint">
        <NText depth="3" style="font-size: 11px">已保存到 {{ lastDownloadPath }}</NText>
        <NButton size="tiny" text type="primary" @click="revealDownload()">
          打开所在目录
        </NButton>
      </div>
    </NScrollbar>

    <PreviewModal v-model:visible="previewVisible" :entry="props.entry" />
    <ShareModal v-model:visible="shareVisible" :entry="props.entry" />
    <TagModal v-model:visible="tagVisible" :entry="props.entry" />
  </div>
</template>

<style scoped>
.panel {
  width: 312px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  overflow: hidden;
}

/* 内容统一内缩：面板留白 16px + 各区块自身 10px = 26px，
   保证 key / 属性 / 操作 / 删除按钮左右对齐。 */
.panel-head {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 16px;
  border-bottom: 1px solid #f0f1f3;
}

.head-icon {
  flex-shrink: 0;
}

.title {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  color: #303236;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.panel-body {
  flex: 1;
  min-height: 0;
  padding: 16px;
}

.key-line {
  margin: 0 10px;
  font-size: 11px;
  color: #909399;
  background: #f7f8fa;
  border-radius: 6px;
  padding: 8px 10px;
  word-break: break-all;
  line-height: 1.6;
}

.meta {
  margin: 16px 10px 0;
  display: flex;
  flex-direction: column;
  gap: 9px;
}

.meta-row {
  display: flex;
  align-items: baseline;
  gap: 12px;
  font-size: 12px;
}

.meta-label {
  width: 82px;
  flex-shrink: 0;
  color: #909399;
}

.meta-value {
  flex: 1;
  min-width: 0;
  color: #303236;
  word-break: break-all;
  text-align: right;
}

/* ETag 等定长标识强制单行，超长省略（完整值见 title 提示） */
.meta-value.mono {
  font-family: ui-monospace, Menlo, Consolas, monospace;
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  word-break: normal;
}

.meta-hint {
  font-size: 12px;
  color: #c0c4cc;
}

.meta-hint.err {
  color: #d03050;
}

.section-title {
  margin: 20px 10px 8px;
  font-size: 11px;
  font-weight: 600;
  color: #909399;
  letter-spacing: 0.4px;
}

.actions {
  margin: 0 10px;
  background: #f7f8fa;
  border-radius: 8px;
  overflow: hidden;
}

.action {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  background: transparent;
  border: none;
  border-bottom: 1px solid #eceef1;
  font-size: 13px;
  color: #303236;
  cursor: pointer;
  text-align: left;
}

.actions .action:last-child {
  border-bottom: none;
}

.action:hover:not(:disabled) {
  background: #eef1f5;
}

.action:disabled {
  color: #c0c4cc;
  cursor: not-allowed;
}

.action.danger {
  margin: 0 10px;
  width: calc(100% - 20px);
  border: 1px solid #f3c2c2;
  border-radius: 8px;
  color: #d03050;
  justify-content: flex-start;
  background: #ffffff;
}

.action.danger:hover:not(:disabled) {
  background: #fdf0f0;
}

.rename {
  margin: 12px 10px 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.dl-hint {
  margin: 14px 10px 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  align-items: flex-start;
}
</style>

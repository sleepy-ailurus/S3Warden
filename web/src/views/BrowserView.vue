<script setup lang="ts">
import { computed, h, onMounted, onUnmounted, ref, watch } from "vue";
import {
  NButton,
  NDataTable,
  NIcon,
  NInput,
  NTag,
  NText,
  useMessage,
} from "naive-ui";
import type { DataTableColumns } from "naive-ui";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  AddOutline,
  ArrowBackOutline,
  ArrowUpOutline,
  CloudDownloadOutline,
  CloudOutline,
  CloudUploadOutline,
  CopyOutline,
  CreateOutline,
  DocumentOutline,
  EyeOutline,
  FolderOutline,
  RefreshOutline,
  SearchOutline,
  ShieldCheckmarkOutline,
  TrashOutline,
} from "@vicons/ionicons5";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { commands } from "../bindings";
import type { BucketSummary, ObjectEntry } from "../bindings";
import { useConnectionStore } from "../stores/connections";
import { useTransferStore } from "../stores/transfers";
import { errText, formatSize, formatTime } from "../utils/format";
import { unwrap } from "../utils/ipc";
import BucketAccessModal from "../components/BucketAccessModal.vue";
import ContextMenu from "../components/ContextMenu.vue";
import type { ContextMenuItem } from "../components/context-menu";
import ObjectDetailPanel from "../components/ObjectDetailPanel.vue";
import PromptModal from "../components/PromptModal.vue";

const store = useConnectionStore();
const transfers = useTransferStore();
const message = useMessage();

/** 文件统一用中性色文件图标，不按扩展名着色。 */
const FILE_ICON_COLOR = "#8a9099";

const uploadHint = ref<string | null>(null);

// ---- 桶列表 ----
const bucketKeyword = ref("");

/** 当前路径（桶/前缀），用于展示与复制。 */
const currentPath = computed(() => {
  const b = store.currentBucket ?? "";
  return store.prefix ? `${b}/${store.prefix}` : `${b}/`;
});

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success("已复制");
  } catch {
    message.warning("复制失败");
  }
}

// ---- 右键菜单 ----
const menu = ref<{ x: number; y: number; items: ContextMenuItem[] } | null>(null);

function openBucketMenu(e: MouseEvent, row: BucketSummary) {
  e.preventDefault();
  menu.value = {
    x: e.clientX,
    y: e.clientY,
    items: [
      { label: "打开", icon: CloudOutline, action: () => store.openBucket(row.name) },
      {
        label: "权限",
        icon: ShieldCheckmarkOutline,
        action: () => {
          accessBucket.value = row.name;
          accessVisible.value = true;
        },
      },
      { label: "复制名称", icon: CopyOutline, action: () => copyText(row.name) },
      { separator: true },
      { label: "删除桶", icon: TrashOutline, danger: true, action: () => removeBucket(row.name) },
    ],
  };
}

function openObjectMenu(e: MouseEvent, row: ObjectEntry) {
  e.preventDefault();
  const path = `${store.currentBucket ?? ""}/${row.key}`;
  const items: ContextMenuItem[] = row.isDir
    ? [
        { label: "打开", icon: FolderOutline, action: () => store.enterDir(row.key) },
        {
          label: "修改名称",
          icon: CreateOutline,
          action: () => {
            renameTarget.value = row;
            renameVisible.value = true;
          },
        },
        { label: "复制路径", icon: CopyOutline, action: () => copyText(path) },
      ]
    : [
        { label: "查看详情", icon: EyeOutline, action: () => store.selectObject(row) },
        { label: "下载", icon: CloudDownloadOutline, action: () => quickDownload(row) },
        { label: "复制路径", icon: CopyOutline, action: () => copyText(path) },
      ];
  menu.value = { x: e.clientX, y: e.clientY, items };
}

// ---- 新建桶 ----
const createVisible = ref(false);
const creating = ref(false);

async function submitCreateBucket(name: string) {
  creating.value = true;
  try {
    await store.createBucket(name);
    message.success(`桶 ${name} 已创建`);
    createVisible.value = false;
  } catch (e) {
    message.error(errText(e));
  } finally {
    creating.value = false;
  }
}

// ---- 删除桶 ----
/**
 * 删除桶：先探测是否非空，非空时二次确认并强制清空后删除。
 *
 * S3 不允许删除非空桶，因此「强制删除」= 先 DeleteObjects 清空再删桶。
 */
async function removeBucket(name: string) {
  if (!store.activeId) return;
  let nonEmpty = false;
  try {
    const page = unwrap(await commands.listObjects(store.activeId, name, "", null));
    nonEmpty = page.entries.length > 0;
  } catch (e) {
    message.error(`无法读取桶内容：${errText(e)}`);
    return;
  }

  const ok = await ask(
    nonEmpty
      ? `桶「${name}」非空，删除前会先清空其中的全部对象，且不可撤销。是否继续？`
      : `确定删除桶「${name}」？该操作不可撤销。`,
    { title: "删除桶", kind: "warning" },
  );
  if (!ok) return;

  try {
    const r = unwrap(await commands.deleteBucket(store.activeId, name, nonEmpty));
    message.success(
      nonEmpty ? `桶 ${name} 已删除，清空 ${r.removedObjects} 个对象` : `桶 ${name} 已删除`,
    );
    await store.reloadBuckets();
  } catch (e) {
    message.error(errText(e));
  }
}

// ---- 桶权限 ----
const accessVisible = ref(false);
const accessBucket = ref("");

// ---- 目录重命名 ----
const renameVisible = ref(false);
const renameTarget = ref<ObjectEntry | null>(null);
const renaming = ref(false);

async function submitRenameDir(newName: string) {
  const target = renameTarget.value;
  if (!target || !store.activeId || !store.currentBucket) return;
  renaming.value = true;
  try {
    const r = unwrap(
      await commands.renameDirectory(
        store.activeId,
        store.currentBucket,
        target.key,
        newName,
      ),
    );
    if (r.errors.length === 0) {
      message.success(`已重命名，移动 ${r.moved} 个对象`);
    } else {
      message.warning(`已重命名：成功 ${r.moved} 个，失败 ${r.errors.length} 项`);
      uploadHint.value = r.errors.join("；");
    }
    renameVisible.value = false;
    store.clearSelection();
    await store.reloadObjects();
  } catch (e) {
    message.error(errText(e));
  } finally {
    renaming.value = false;
  }
}

/** 右键 → 下载：选保存路径后入传输队列（进度见右下角传输面板）。 */
async function quickDownload(row: ObjectEntry) {
  if (!store.activeId || !store.currentBucket) return;
  const dest = await save({ defaultPath: row.name, title: "下载到…" });
  if (!dest) return;
  try {
    await transfers.startDownload(store.activeId, store.currentBucket, row.key, dest);
    message.success("已加入下载队列");
  } catch (e) {
    message.error(errText(e));
  }
}

const filteredBuckets = computed(() => {
  const k = bucketKeyword.value.trim().toLowerCase();
  if (!k) return store.buckets;
  return store.buckets.filter((b) => b.name.toLowerCase().includes(k));
});

const bucketColumns: DataTableColumns<BucketSummary> = [
  {
    title: "桶名",
    key: "name",
    render: (row) =>
      h("span", { class: "cell-link" }, [
        h(
          NIcon,
          { size: 15, style: "margin-right:6px;vertical-align:-2px;color:#18a058" },
          { default: () => h(CloudOutline) },
        ),
        row.name,
      ]),
  },
  {
    title: "访问",
    key: "access",
    width: 92,
    render: (row) => {
      const a = store.bucketAccess[row.name];
      if (!a) return h("span", { style: "color:#c0c4cc" }, "—");
      if (a === "public") {
        return h(
          NTag,
          { size: "small", type: "warning", bordered: false, round: true },
          { default: () => "公开" },
        );
      }
      if (a === "custom") {
        return h(
          NTag,
          { size: "small", type: "info", bordered: false, round: true },
          { default: () => "自定义" },
        );
      }
      return h(
        NTag,
        { size: "small", bordered: false, round: true },
        { default: () => "私有" },
      );
    },
  },
  {
    title: "创建时间",
    key: "createdAt",
    width: 200,
    render: (row) => formatTime(row.createdAt),
  },
];

const bucketRowProps = (row: BucketSummary) => ({
  style: "cursor: pointer;",
  onClick: () => store.openBucket(row.name),
  onContextmenu: (e: MouseEvent) => openBucketMenu(e, row),
});

// ---- 对象浏览 ----
// 目录恒排最前，其余按当前列排序（客户端排序，作用于已加载条目）。
function dirFirst(a: ObjectEntry, b: ObjectEntry, cmp: () => number) {
  if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
  return cmp();
}

const objectColumns: DataTableColumns<ObjectEntry> = [
  { type: "selection" },
  {
    title: "名称",
    key: "name",
    sorter: (a, b) =>
      dirFirst(a, b, () => a.name.toLowerCase().localeCompare(b.name.toLowerCase())),
    render: (row) =>
      row.isDir
        ? h(
            "span",
            { class: "cell-link", onClick: () => store.enterDir(row.key) },
            [
              h(
                NIcon,
                { size: 15, style: "margin-right:6px;vertical-align:-2px;color:#f0a020" },
                { default: () => h(FolderOutline) },
              ),
              row.name,
            ],
          )
        : h("span", { class: "cell-file" }, [
            h(
              NIcon,
              {
                size: 15,
                style: `margin-right:6px;vertical-align:-2px;color:${FILE_ICON_COLOR}`,
              },
              { default: () => h(DocumentOutline) },
            ),
            row.name,
          ]),
  },
  {
    title: "大小",
    key: "size",
    width: 110,
    sorter: (a, b) => dirFirst(a, b, () => (a.size ?? 0) - (b.size ?? 0)),
    render: (row) => (row.isDir ? "—" : formatSize(row.size)),
  },
  {
    title: "最后修改",
    key: "lastModified",
    width: 190,
    sorter: (a, b) =>
      dirFirst(a, b, () =>
        (a.lastModified ?? "").localeCompare(b.lastModified ?? ""),
      ),
    render: (row) => formatTime(row.lastModified),
  },
];

const objectRowProps = (row: ObjectEntry) => ({
  style: "cursor: pointer;",
  class: store.selected?.key === row.key ? "row-active" : "",
  onClick: (e: MouseEvent) => {
    // 勾选框点击不触发行为（进入目录/打开详情），否则冒泡会换目录并清空选择
    const target = e.target as HTMLElement | null;
    if (target?.closest(".n-checkbox")) return;
    row.isDir ? store.enterDir(row.key) : store.selectObject(row);
  },
  onContextmenu: (e: MouseEvent) => openObjectMenu(e, row),
});

// ---- 批量选择与批量删除（OBJ-10）----
const checkedKeys = ref<string[]>([]);

// 换目录/换桶时清空选择
watch(
  () => [store.currentBucket, store.prefix],
  () => {
    checkedKeys.value = [];
  },
);

const batchDeleting = ref(false);

async function batchDelete() {
  const keys = checkedKeys.value;
  if (keys.length === 0 || !store.activeId || !store.currentBucket) return;
  const ok = await ask(
    `确定删除选中的 ${keys.length} 项？文件夹会连同其中全部对象一起删除，且不可撤销。`,
    { title: "批量删除", kind: "warning" },
  );
  if (!ok) return;

  batchDeleting.value = true;
  try {
    const r = unwrap(
      await commands.deleteObjectsBatch(store.activeId, store.currentBucket, keys),
    );
    if (r.errors.length === 0) {
      message.success(`已删除 ${r.deleted} 个对象`);
    } else {
      message.warning(`部分删除失败：${r.errors.length} 项`);
      uploadHint.value = r.errors.slice(0, 5).join("；");
    }
    checkedKeys.value = [];
    store.clearSelection();
    await store.reloadObjects();
  } catch (e) {
    message.error(errText(e));
  } finally {
    batchDeleting.value = false;
  }
}

/** Ctrl+A 全选已加载条目（输入框内不劫持）。 */
function onKeydown(e: KeyboardEvent) {
  if (!store.currentBucket || !e.ctrlKey || e.key.toLowerCase() !== "a") return;
  const target = e.target as HTMLElement | null;
  if (target && ["INPUT", "TEXTAREA"].includes(target.tagName)) return;
  e.preventDefault();
  checkedKeys.value = store.objects.map((o) => o.key);
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));

// 有任务完成时自动刷新对象列表（上传新对象 / 目录出现）
watch(
  () => transfers.finishedCount,
  () => {
    if (store.currentBucket) void store.reloadObjects();
  },
);

// ---- 前缀内搜索（客户端过滤已加载条目）----
const objectKeyword = ref("");

const filteredObjects = computed(() => {
  const k = objectKeyword.value.trim().toLowerCase();
  if (!k) return store.objects;
  return store.objects.filter((o) => o.name.toLowerCase().includes(k));
});

// ---- 新建文件夹（占位前缀，OBJ-13）----
const newFolderVisible = ref(false);
const newFolderCreating = ref(false);

async function submitNewFolder(name: string) {
  if (!store.activeId || !store.currentBucket) return;
  newFolderCreating.value = true;
  try {
    unwrap(
      await commands.createPrefix(store.activeId, store.currentBucket, store.prefix, name),
    );
    message.success(`文件夹 ${name} 已创建`);
    newFolderVisible.value = false;
    await store.reloadObjects();
  } catch (e) {
    message.error(errText(e));
  } finally {
    newFolderCreating.value = false;
  }
}

// ---- 拖拽上传（Webview 级 drag-drop 事件携带本地路径）----
const dragOver = ref(false);
let unlistenDrag: (() => void) | null = null;

onMounted(async () => {
  unlistenDrag = await getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "enter" || event.payload.type === "over") {
      if (store.currentBucket) dragOver.value = true;
    } else if (event.payload.type === "leave") {
      dragOver.value = false;
    } else if (event.payload.type === "drop") {
      dragOver.value = false;
      const paths = event.payload.paths ?? [];
      if (paths.length > 0 && store.currentBucket) void runUpload(paths, "拖拽内容");
    }
  });
});

onUnmounted(() => {
  unlistenDrag?.();
});

/** 前缀面包屑分段（累计前缀可点击回跳）。 */
const prefixSegments = computed(() => {
  if (!store.prefix) return [];
  const parts = store.prefix.split("/").filter(Boolean);
  const segs: { name: string; prefix: string }[] = [];
  let acc = "";
  for (const part of parts) {
    acc += `${part}/`;
    segs.push({ name: part, prefix: acc });
  }
  return segs;
});

/**
 * 返回上一级。
 *
 * 已在桶根目录（无前缀）时，「上一级」的语义是回到桶列表。
 */
async function goUp() {
  const segs = prefixSegments.value;
  if (segs.length === 0) {
    store.closeBucket();
    return;
  }
  const parent = segs.length > 1 ? segs[segs.length - 2].prefix : "";
  await store.goToPrefix(parent);
}

// ---- 上传（入传输队列：分片上传 / 断点续传 / 进度见传输面板）----
async function runUpload(paths: string[], kindLabel: string) {
  if (!store.activeId || !store.currentBucket || paths.length === 0) return;
  uploadHint.value = null;
  try {
    const infos = await transfers.startUpload(paths);
    message.success(`${kindLabel}已加入传输队列：${infos.length} 个文件`);
  } catch (e) {
    message.error(errText(e));
  }
}

async function uploadFiles() {
  const picked = await open({
    multiple: true,
    directory: false,
    title: "选择要上传的文件",
  });
  if (!picked) return;
  await runUpload(Array.isArray(picked) ? picked : [picked], "文件");
}

async function uploadFolder() {
  const picked = await open({
    multiple: false,
    directory: true,
    title: "选择要上传的文件夹",
  });
  if (!picked) return;
  await runUpload(Array.isArray(picked) ? picked : [picked], "文件夹");
}
</script>

<template>
  <!-- 空态：未选择连接 -->
  <div v-if="!store.activeProfile" class="empty">
    <NIcon :size="56" color="#c8ccd4"><ShieldCheckmarkOutline /></NIcon>
    <div class="empty-title">选择或创建一个连接开始</div>
    <div class="empty-sub">S3Warden 会将密钥保存在操作系统凭据存储中，不会在界面上回显</div>
    <NButton type="primary" size="small" style="margin-top: 16px" @click="store.openCreate()">
      新建连接
    </NButton>
  </div>

  <!-- 桶列表 -->
  <div v-else-if="!store.currentBucket" class="page">
    <div class="page-head">
      <div class="page-title">
        桶
        <span class="page-count">{{ store.buckets.length }}</span>
      </div>
      <div class="page-tools">
        <NButton size="small" type="primary" @click="createVisible = true">
          <template #icon>
            <NIcon><AddOutline /></NIcon>
          </template>
          新建桶
        </NButton>
        <NInput v-model:value="bucketKeyword" size="small" placeholder="筛选桶" clearable style="width: 180px">
          <template #prefix>
            <NIcon size="13" color="#909399"><SearchOutline /></NIcon>
          </template>
        </NInput>
        <NButton size="small" quaternary :loading="store.bucketsLoading" @click="store.reloadBuckets()">
          <template #icon>
            <NIcon><RefreshOutline /></NIcon>
          </template>
          刷新
        </NButton>
      </div>
    </div>
    <div class="table-wrap">
      <NDataTable
        :columns="bucketColumns"
        :data="filteredBuckets"
        :loading="store.bucketsLoading"
        :row-props="bucketRowProps"
        :flex-height="true"
        style="height: 100%"
      />
    </div>
  </div>

  <!-- 对象浏览 -->
  <div v-else class="page">
    <div class="page-head">
      <div class="path-bar">
        <NButton size="tiny" quaternary title="上一级" @click="goUp()">
          <template #icon>
            <NIcon><ArrowBackOutline /></NIcon>
          </template>
        </NButton>

        <div class="crumbs">
          <span class="crumb-link" @click="store.closeBucket()">桶列表</span>
          <span class="crumb-sep">/</span>
          <span class="crumb-strong">{{ store.currentBucket }}</span>
          <template v-for="seg in prefixSegments" :key="seg.prefix">
            <span class="crumb-sep">/</span>
            <span class="crumb-link" @click="store.goToPrefix(seg.prefix)">{{ seg.name }}</span>
          </template>
        </div>
        <NButton size="tiny" quaternary title="复制路径" @click="copyText(currentPath)">
          <template #icon>
            <NIcon><CopyOutline /></NIcon>
          </template>
        </NButton>
      </div>

      <div class="page-tools">
        <NInput
          v-model:value="objectKeyword"
          size="small"
          placeholder="搜索当前目录"
          clearable
          style="width: 170px"
        >
          <template #prefix>
            <NIcon size="13" color="#909399"><SearchOutline /></NIcon>
          </template>
        </NInput>
        <NButton
          v-if="checkedKeys.length > 0"
          size="small"
          type="error"
          secondary
          :loading="batchDeleting"
          @click="batchDelete()"
        >
          <template #icon>
            <NIcon><TrashOutline /></NIcon>
          </template>
          删除选中（{{ checkedKeys.length }}）
        </NButton>
        <NButton size="small" @click="newFolderVisible = true">
          <template #icon>
            <NIcon><FolderOutline /></NIcon>
          </template>
          新建文件夹
        </NButton>
        <NButton size="small" @click="uploadFiles()">
          <template #icon>
            <NIcon><CloudUploadOutline /></NIcon>
          </template>
          上传文件
        </NButton>
        <NButton size="small" @click="uploadFolder()">
          <template #icon>
            <NIcon><FolderOutline /></NIcon>
          </template>
          上传文件夹
        </NButton>
        <NButton size="small" quaternary @click="goUp()">
          <template #icon>
            <NIcon><ArrowUpOutline /></NIcon>
          </template>
          上一级
        </NButton>
        <NButton size="small" quaternary :loading="store.objectsLoading" @click="store.reloadObjects()">
          <template #icon>
            <NIcon><RefreshOutline /></NIcon>
          </template>
          刷新
        </NButton>
      </div>
    </div>

    <div class="content-row">
      <div class="table-wrap" style="position: relative">
        <NDataTable
          :columns="objectColumns"
          :data="filteredObjects"
          :loading="store.objectsLoading"
          :row-props="objectRowProps"
          :row-key="(row: ObjectEntry) => row.key"
          v-model:checked-row-keys="checkedKeys"
          :flex-height="true"
          :virtual-scroll="true"
          style="height: 100%"
        />
        <div v-if="dragOver" class="drop-mask">
          <div class="drop-box">
            <NIcon :size="36" color="#18a058"><CloudUploadOutline /></NIcon>
            <div>松开鼠标上传到当前目录</div>
          </div>
        </div>
      </div>
      <ObjectDetailPanel
        v-if="store.selected"
        :entry="store.selected"
        @close="store.clearSelection()"
      />
    </div>

    <div class="page-foot">
      <span class="page-count-info">
        共 {{ filteredObjects.length }} 项{{ store.nextToken ? "，未加载完" : "" }}{{ objectKeyword ? `（筛选自 ${store.objects.length} 项）` : "" }}
        <template v-if="uploadHint">
          <NText type="warning" style="font-size: 12px"> · {{ uploadHint }}</NText>
        </template>
      </span>
      <NButton
        v-if="store.nextToken"
        size="tiny"
        :loading="store.objectsLoading"
        @click="store.loadMoreObjects()"
      >
        加载更多
      </NButton>
    </div>
  </div>

  <!-- 弹出层挂在视图根级：桶列表页与对象浏览页共用同一套 -->
  <ContextMenu
    v-if="menu"
    :x="menu.x"
    :y="menu.y"
    :items="menu.items"
    @close="menu = null"
  />

  <PromptModal
    v-model:visible="createVisible"
    title="新建桶"
    label="桶名"
    :initial="''"
    placeholder="例如 my-bucket"
    hint="3~63 个字符，仅小写字母、数字、点和连字符，首尾必须是字母或数字；区域沿用当前连接配置。创建后默认私有，可在桶上右键 → 权限 改为公开。"
    confirm-text="创建"
    :loading="creating"
    @submit="submitCreateBucket"
  />

  <BucketAccessModal v-model:visible="accessVisible" :bucket="accessBucket" />

  <PromptModal
    v-model:visible="renameVisible"
    title="修改文件夹名称"
    label="新名称"
    :initial="renameTarget?.name ?? ''"
    placeholder="不能包含 /"
    hint="S3 中目录只是公共前缀，重命名会把该前缀下所有对象复制到新前缀后删除原对象。"
    confirm-text="重命名"
    :loading="renaming"
    @submit="submitRenameDir"
  />

  <PromptModal
    v-model:visible="newFolderVisible"
    title="新建文件夹"
    label="文件夹名"
    :initial="''"
    placeholder="例如 photos"
    hint="S3 中目录只是公共前缀：这里会写入一个以 / 结尾的占位对象。"
    confirm-text="创建"
    :loading="newFolderCreating"
    @submit="submitNewFolder"
  />
</template>

<style scoped>
.empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
}

.empty-title {
  margin-top: 10px;
  font-size: 15px;
  font-weight: 600;
  color: #50545b;
}

.empty-sub {
  font-size: 12px;
  color: #909399;
}

.page {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 16px 20px;
  gap: 12px;
}

.page-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-shrink: 0;
}

.page-title {
  font-size: 15px;
  font-weight: 600;
}

.page-count {
  font-size: 12px;
  font-weight: 400;
  color: #909399;
  margin-left: 4px;
}

.page-tools {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.path-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  flex: 1;
}

.crumbs {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  min-width: 0;
  overflow: hidden;
}

.crumb-link {
  color: #18a058;
  cursor: pointer;
  white-space: nowrap;
}

.crumb-link:hover {
  text-decoration: underline;
}

.crumb-strong {
  font-weight: 600;
  white-space: nowrap;
}

.crumb-sep {
  color: #c0c4cc;
}

.content-row {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 12px;
}

.table-wrap {
  flex: 1;
  min-width: 0;
  min-height: 0;
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  padding: 6px;
}

.page-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
}

.page-count-info {
  font-size: 12px;
  color: #909399;
}

:deep(.cell-link) {
  color: #303236;
  cursor: pointer;
}

:deep(.cell-link:hover) {
  color: #18a058;
}

:deep(.cell-file) {
  color: #303236;
}

:deep(.row-active) {
  background: #eef6ff !important;
}

.drop-mask {
  position: absolute;
  inset: 0;
  z-index: 10;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(24, 160, 88, 0.06);
  border-radius: 8px;
}

.drop-box {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 24px 40px;
  border: 2px dashed #18a058;
  border-radius: 12px;
  background: #ffffff;
  color: #18a058;
  font-size: 14px;
  font-weight: 500;
  pointer-events: none;
}
</style>

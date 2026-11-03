import { computed, ref } from "vue";
import { acceptHMRUpdate, defineStore } from "pinia";
import { commands } from "../bindings";
import type {
  BackendPresetInfo,
  BucketAccess,
  BucketSummary,
  ConnectionProfile,
  ObjectEntry,
} from "../bindings";
import { unwrap } from "../utils/ipc";

/**
 * 全局连接状态：连接列表、当前连接、桶与对象浏览状态、连接对话框状态。
 * 所有 Rust 调用走 bindings 生成的 commands，禁止手写 invoke（IPC-02）。
 */
export const useConnectionStore = defineStore("connections", () => {
  // ---- 后端接入类型选项（数据源在 Rust 侧）----
  const presets = ref<BackendPresetInfo[]>([]);

  // ---- 连接列表 ----
  const profiles = ref<ConnectionProfile[]>([]);
  const loaded = ref(false);
  const activeId = ref<string | null>(null);
  const activeProfile = computed(
    () => profiles.value.find((p) => p.id === activeId.value) ?? null,
  );

  // ---- 桶列表 ----
  const buckets = ref<BucketSummary[]>([]);
  const bucketsLoading = ref(false);
  /** 桶名 → 访问级别；读取失败的桶不出现（前端显示 —）。 */
  const bucketAccess = ref<Record<string, BucketAccess>>({});

  // ---- 对象浏览 ----
  const currentBucket = ref<string | null>(null);
  const prefix = ref("");
  const objects = ref<ObjectEntry[]>([]);
  const objectsLoading = ref(false);
  const nextToken = ref<string | null>(null);
  /** 详情面板选中的对象（目录不可选中操作，仅用于进入目录）。 */
  const selected = ref<ObjectEntry | null>(null);

  function selectObject(entry: ObjectEntry | null) {
    selected.value = entry;
  }

  function clearSelection() {
    selected.value = null;
  }

  // ---- 连接对话框 ----
  const modalVisible = ref(false);
  const editing = ref<ConnectionProfile | null>(null);

  function openCreate() {
    editing.value = null;
    modalVisible.value = true;
  }

  function openEdit(p: ConnectionProfile) {
    editing.value = p;
    modalVisible.value = true;
  }

  async function refreshProfiles() {
    profiles.value = unwrap(await commands.listConnections());
    loaded.value = true;
  }

  /** 拉取后端接入类型选项；失败不阻塞主流程（表单退化为手动填写）。 */
  async function loadPresets() {
    if (presets.value.length > 0) return;
    try {
      presets.value = await commands.listBackendPresets();
    } catch {
      presets.value = [];
    }
  }

  function select(id: string | null) {
    if (activeId.value === id) return;
    activeId.value = id;
    currentBucket.value = null;
    prefix.value = "";
    objects.value = [];
    nextToken.value = null;
    selected.value = null;
    buckets.value = [];
    if (id) void reloadBuckets();
  }

  async function reloadBuckets() {
    if (!activeId.value) return;
    bucketsLoading.value = true;
    try {
      buckets.value = unwrap(await commands.listBuckets(activeId.value));
    } finally {
      bucketsLoading.value = false;
    }
    await loadBucketAccess();
  }

  /** 并发拉取各桶的访问级别（读取失败则该桶不展示标签）。 */
  async function loadBucketAccess() {
    const id = activeId.value;
    if (!id) return;
    const queue = buckets.value.map((b) => b.name);
    if (queue.length === 0) {
      bucketAccess.value = {};
      return;
    }
    const next: Record<string, BucketAccess> = {};
    const worker = async () => {
      while (queue.length > 0) {
        const name = queue.shift()!;
        try {
          const r = unwrap(await commands.getBucketAccess(id, name));
          next[name] = r.access;
        } catch {
          // 无权读取策略的桶静默跳过
        }
      }
    };
    await Promise.all(
      Array.from({ length: Math.min(6, queue.length) }, () => worker()),
    );
    bucketAccess.value = next;
  }

  /** 新建桶：桶名规则由 Rust 侧校验，创建成功后刷新列表。 */
  async function createBucket(name: string) {
    if (!activeId.value) return;
    unwrap(await commands.createBucket(activeId.value, name));
    await reloadBuckets();
  }

  async function openBucket(name: string) {
    currentBucket.value = name;
    prefix.value = "";
    selected.value = null;
    await reloadObjects();
  }

  function closeBucket() {
    currentBucket.value = null;
    prefix.value = "";
    objects.value = [];
    nextToken.value = null;
    selected.value = null;
  }

  async function reloadObjects() {
    if (!activeId.value || !currentBucket.value) return;
    objectsLoading.value = true;
    try {
      const page = unwrap(
        await commands.listObjects(
          activeId.value,
          currentBucket.value,
          prefix.value,
          null,
        ),
      );
      objects.value = page.entries;
      nextToken.value = page.nextToken;
    } finally {
      objectsLoading.value = false;
    }
  }

  async function loadMoreObjects() {
    if (!activeId.value || !currentBucket.value || !nextToken.value) return;
    objectsLoading.value = true;
    try {
      const page = unwrap(
        await commands.listObjects(
          activeId.value,
          currentBucket.value,
          prefix.value,
          nextToken.value,
        ),
      );
      objects.value = objects.value.concat(page.entries);
      nextToken.value = page.nextToken;
    } finally {
      objectsLoading.value = false;
    }
  }

  async function enterDir(key: string) {
    prefix.value = key;
    selected.value = null;
    await reloadObjects();
  }

  async function goToPrefix(target: string) {
    prefix.value = target;
    selected.value = null;
    await reloadObjects();
  }

  return {
    presets,
    profiles,
    loaded,
    activeId,
    activeProfile,
    buckets,
    bucketsLoading,
    bucketAccess,
    loadBucketAccess,
    currentBucket,
    prefix,
    objects,
    objectsLoading,
    nextToken,
    selected,
    selectObject,
    clearSelection,
    modalVisible,
    editing,
    openCreate,
    openEdit,
    refreshProfiles,
    loadPresets,
    select,
    reloadBuckets,
    createBucket,
    openBucket,
    closeBucket,
    reloadObjects,
    loadMoreObjects,
    enterDir,
    goToPrefix,
  };
});

/**
 * 开发期热更新接管。
 *
 * 不带这段时，修改 store 后 Vite 会替换模块，但 Pinia 缓存的旧 store 实例
 * 不会重建，新加的函数在运行时报「xxx is not a function」（类型检查仍然通过）。
 */
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useConnectionStore, import.meta.hot));
}

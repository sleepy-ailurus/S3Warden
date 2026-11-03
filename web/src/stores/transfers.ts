import { computed, ref } from "vue";
import { acceptHMRUpdate, defineStore } from "pinia";
import { commands, events } from "../bindings";
import { unwrap } from "../utils/ipc";
import { useConnectionStore } from "./connections";

/** 传输任务状态（来自 TransferStatusEvent.status）。 */
export type TransferStatus =
  | "queued"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

/** 传输任务视图（由任务创建信息 + 事件流合并而来）。 */
export interface TransferTask {
  taskId: string;
  kind: "upload" | "download";
  name: string;
  key: string;
  localPath: string;
  total: number;
  transferred: number;
  status: TransferStatus;
  error: string | null;
  /** 速率（字节/秒，前端按秒采样差分计算）。 */
  speed: number;
}

/** 新建任务返回信息（bindings TransferTaskInfo）。 */
interface TaskInfo {
  taskId: string;
  kind: "upload" | "download";
  name: string;
  key: string;
  localPath: string;
  total: number | null;
}

/**
 * 传输队列状态：任务列表、进度/状态事件监听、速率采样。
 *
 * Rust 侧只推送字节进度；速率由前端按秒差分计算。
 */
export const useTransferStore = defineStore("transfers", () => {
  const tasks = ref<Map<string, TransferTask>>(new Map());
  /** 上一秒采样值（taskId → transferred）。 */
  const lastSample = new Map<string, number>();
  const panelVisible = ref(false);
  let listening = false;

  /** 绑定事件监听（AppShell 挂载时调用一次）。 */
  async function init() {
    if (listening) return;
    listening = true;

    await events.transferProgressEvent.listen((e) => {
      const t = tasks.value.get(e.payload.taskId);
      if (!t) return;
      t.transferred = e.payload.transferred ?? 0;
      if ((e.payload.total ?? 0) > 0) t.total = e.payload.total ?? 0;
      if (t.status === "queued") t.status = "running";
      tasks.value = new Map(tasks.value);
    });

    await events.transferStatusEvent.listen((e) => {
      const p = e.payload;
      const t = tasks.value.get(p.taskId);
      if (!t) return;
      t.status = p.status as TransferStatus;
      t.error = p.error;
      if (p.status === "completed") {
        t.transferred = p.transferred || t.total;
        t.speed = 0;
      }
      if (p.status === "cancelled" || p.status === "failed") t.speed = 0;
      tasks.value = new Map(tasks.value);
    });

    // 速率采样：按秒差分
    setInterval(() => {
      for (const t of tasks.value.values()) {
        if (t.status !== "running") continue;
        const last = lastSample.get(t.taskId) ?? 0;
        t.speed = Math.max(0, t.transferred - last);
        lastSample.set(t.taskId, t.transferred);
      }
      tasks.value = new Map(tasks.value);
    }, 1000);
  }

  const taskList = computed(() => Array.from(tasks.value.values()).reverse());
  const activeCount = computed(
    () =>
      taskList.value.filter((t) => t.status === "running" || t.status === "queued")
        .length,
  );
  const finishedCount = computed(
    () =>
      taskList.value.filter(
        (t) =>
          t.status === "completed" ||
          t.status === "failed" ||
          t.status === "cancelled",
      ).length,
  );

  function register(infos: TaskInfo[]) {
    for (const t of infos) {
      lastSample.set(t.taskId, 0);
      tasks.value.set(t.taskId, {
        taskId: t.taskId,
        kind: t.kind,
        name: t.name,
        key: t.key,
        localPath: t.localPath,
        total: t.total ?? 0,
        transferred: 0,
        status: "queued",
        error: null,
        speed: 0,
      });
    }
    tasks.value = new Map(tasks.value);
  }

  /** 上传本地文件/文件夹（每文件一个任务入队）。 */
  async function startUpload(paths: string[]): Promise<TaskInfo[]> {
    const conn = useConnectionStore();
    if (!conn.activeId || !conn.currentBucket) return [];
    const infos = unwrap(
      await commands.startUpload(
        conn.activeId,
        conn.currentBucket,
        conn.prefix,
        paths,
      ),
    );
    register(infos);
    return infos;
  }

  /** 下载对象（入队）。 */
  async function startDownload(
    id: string,
    bucket: string,
    key: string,
    dest: string,
  ): Promise<TaskInfo> {
    const t = unwrap(await commands.startDownload(id, bucket, key, dest));
    register([t]);
    return t;
  }

  async function cancel(taskId: string) {
    unwrap(await commands.cancelTransfer(taskId));
  }

  async function retry(taskId: string) {
    unwrap(await commands.retryTransfer(taskId));
    const t = tasks.value.get(taskId);
    if (t) {
      t.status = "queued";
      t.error = null;
      t.transferred = 0;
      lastSample.set(taskId, 0);
      tasks.value = new Map(tasks.value);
    }
  }

  /** 清理已完成/失败/取消的任务记录。 */
  function clearFinished() {
    for (const [k, t] of tasks.value) {
      if (t.status !== "running" && t.status !== "queued") {
        tasks.value.delete(k);
        lastSample.delete(k);
      }
    }
    tasks.value = new Map(tasks.value);
  }

  return {
    taskList,
    activeCount,
    finishedCount,
    panelVisible,
    init,
    startUpload,
    startDownload,
    cancel,
    retry,
    clearFinished,
  };
});

if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useTransferStore, import.meta.hot));
}

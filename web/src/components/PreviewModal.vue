<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  NButton,
  NIcon,
  NModal,
  NScrollbar,
  NSpin,
  NText,
  useMessage,
} from "naive-ui";
import { EyeOffOutline } from "@vicons/ionicons5";
import { commands } from "../bindings";
import type { ObjectEntry } from "../bindings";
import { useConnectionStore } from "../stores/connections";
import { isImage, isMedia, isText } from "../utils/filetype";
import { formatSize } from "../utils/format";
import { unwrap } from "../utils/ipc";

const props = defineProps<{
  visible: boolean;
  entry: ObjectEntry | null;
}>();
const emit = defineEmits<{ (e: "update:visible", v: boolean): void }>();

const store = useConnectionStore();
const message = useMessage();

const loading = ref(false);
const imageUrl = ref<string | null>(null);
const mediaUrl = ref<string | null>(null);
const text = ref<string | null>(null);
const truncated = ref(false);
const error = ref<string | null>(null);

const name = computed(() => props.entry?.name ?? "");
const kind = computed(() => {
  if (!props.entry) return "unsupported";
  if (isImage(name.value)) return "image";
  if (isText(name.value)) return "text";
  if (isMedia(name.value)) return "media";
  return "unsupported";
});

const connId = computed(() => store.activeId ?? "");
const bucket = computed(() => store.currentBucket ?? "");
const key = computed(() => props.entry?.key ?? "");

function reset() {
  imageUrl.value = null;
  mediaUrl.value = null;
  text.value = null;
  truncated.value = false;
  error.value = null;
}

async function load() {
  if (!props.entry || !connId.value || !bucket.value) return;
  reset();
  loading.value = true;
  try {
    if (kind.value === "text") {
      const r = unwrap(
        await commands.readObjectText(connId.value, bucket.value, key.value, null),
      );
      if (r.binary) {
        error.value = "该文件疑似二进制内容，无法以文本预览";
      } else {
        text.value = r.content;
        truncated.value = r.truncated;
      }
    } else if (kind.value !== "unsupported") {
      // 图片与音视频统一走预签名链接，由 WebView 直接渲染，不占用前端内存
      const r = unwrap(
        await commands.presignObject(connId.value, bucket.value, key.value, 3600),
      );
      if (kind.value === "image") imageUrl.value = r.url;
      else mediaUrl.value = r.url;
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

watch(
  () => [props.visible, props.entry?.key],
  () => {
    if (props.visible) void load();
  },
  { immediate: true },
);

function close() {
  emit("update:visible", false);
}

function retry() {
  void load();
}
</script>

<template>
  <NModal
    :show="visible"
    preset="card"
    :title="`预览 · ${name}`"
    :style="{ width: kind === 'text' ? '760px' : '880px' }"
    size="small"
    :bordered="false"
    @update:show="emit('update:visible', $event)"
  >
    <div class="preview">
      <div v-if="loading" class="center">
        <NSpin size="medium" />
      </div>

      <div v-else-if="error" class="center">
        <NIcon :size="40" color="#c0c4cc"><EyeOffOutline /></NIcon>
        <NText depth="3" style="margin-top: 10px">{{ error }}</NText>
        <NButton size="tiny" style="margin-top: 12px" @click="retry()">重试</NButton>
      </div>

      <div v-else-if="kind === 'image' && imageUrl" class="center">
        <img :src="imageUrl" :alt="name" class="preview-img" />
      </div>

      <div v-else-if="kind === 'media' && mediaUrl" class="center">
        <video
          v-if="['mp4', 'mov', 'webm', 'mkv', 'avi'].some((e) => name.toLowerCase().endsWith(e))"
          :src="mediaUrl"
          controls
          class="preview-video"
        />
        <audio v-else :src="mediaUrl" controls class="preview-audio" />
      </div>

      <NScrollbar v-else-if="kind === 'text'" class="text-wrap">
        <pre class="preview-text">{{ text }}</pre>
        <div v-if="truncated" class="trunc-hint">
          内容超过预览上限，仅显示开头部分（完整内容请下载查看）
        </div>
      </NScrollbar>

      <div v-else class="center">
        <NIcon :size="40" color="#c0c4cc"><EyeOffOutline /></NIcon>
        <NText depth="3" style="margin-top: 10px">
          暂不支持该文件类型的预览（{{ formatSize(props.entry?.size ?? 0) }}）
        </NText>
      </div>
    </div>

    <template #footer>
      <div class="footer">
        <NText depth="3" style="font-size: 12px">{{ key }}</NText>
        <NButton size="small" @click="close()">关闭</NButton>
      </div>
    </template>
  </NModal>
</template>

<style scoped>
.preview {
  min-height: 320px;
  max-height: 60vh;
  display: flex;
}

.center {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
}

.preview-img {
  max-width: 100%;
  max-height: 56vh;
  object-fit: contain;
  border-radius: 6px;
  background: #f7f8fa;
}

.preview-video {
  width: 100%;
  max-height: 54vh;
  background: #000;
  border-radius: 6px;
}

.preview-audio {
  width: 100%;
  margin-top: 12px;
}

.text-wrap {
  flex: 1;
  max-height: 56vh;
  background: #f7f8fa;
  border-radius: 6px;
  padding: 10px 12px;
}

.preview-text {
  margin: 0;
  font-size: 12px;
  line-height: 1.6;
  font-family: ui-monospace, Menlo, Consolas, monospace;
  white-space: pre-wrap;
  word-break: break-all;
  color: #303236;
}

.trunc-hint {
  margin-top: 10px;
  font-size: 12px;
  color: #909399;
}

.footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.footer :first-child {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>

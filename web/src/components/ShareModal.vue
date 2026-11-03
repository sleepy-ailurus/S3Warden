<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  NButton,
  NIcon,
  NInput,
  NModal,
  NSelect,
  NText,
  useMessage,
} from "naive-ui";
import { CopyOutline, LinkOutline, RefreshOutline } from "@vicons/ionicons5";
import { openUrl } from "@tauri-apps/plugin-opener";
import { commands } from "../bindings";
import type { ObjectEntry } from "../bindings";
import { useConnectionStore } from "../stores/connections";
import { unwrap } from "../utils/ipc";

const props = defineProps<{
  visible: boolean;
  entry: ObjectEntry | null;
}>();
const emit = defineEmits<{ (e: "update:visible", v: boolean): void }>();

const store = useConnectionStore();
const message = useMessage();

const loading = ref(false);
const url = ref<string | null>(null);
const error = ref<string | null>(null);

const EXPIRY_OPTIONS = [
  { label: "10 分钟", value: 600 },
  { label: "1 小时", value: 3600 },
  { label: "1 天", value: 86400 },
  { label: "7 天", value: 604800 },
];
const expiresSecs = ref<number>(3600);

const connId = computed(() => store.activeId ?? "");
const bucket = computed(() => store.currentBucket ?? "");
const key = computed(() => props.entry?.key ?? "");

async function generate() {
  if (!props.entry || !connId.value || !bucket.value) return;
  loading.value = true;
  error.value = null;
  try {
    const r = unwrap(
      await commands.presignObject(
        connId.value,
        bucket.value,
        key.value,
        expiresSecs.value,
      ),
    );
    url.value = r.url;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

watch(
  () => [props.visible, props.entry?.key],
  () => {
    if (props.visible) {
      url.value = null;
      error.value = null;
      void generate();
    }
  },
  { immediate: true },
);

async function copyUrl() {
  if (!url.value) return;
  try {
    await navigator.clipboard.writeText(url.value);
    message.success("链接已复制");
  } catch {
    message.warning("复制失败，请手动选择链接复制");
  }
}

/**
 * 用系统默认浏览器打开链接。
 *
 * WebView 内 `window.open` 会被壳层拦截（无效果），必须走 opener 插件。
 */
async function openInBrowser() {
  if (!url.value) return;
  try {
    await openUrl(url.value);
  } catch (e) {
    message.warning(`打开浏览器失败：${e instanceof Error ? e.message : String(e)}`);
  }
}
</script>

<template>
  <NModal
    :show="visible"
    preset="card"
    :title="`分享 · ${props.entry?.name ?? ''}`"
    style="width: 620px"
    size="small"
    :bordered="false"
    @update:show="emit('update:visible', $event)"
  >
    <div class="share">
      <div class="row">
        <span class="label">有效期</span>
        <NSelect
          v-model:value="expiresSecs"
          :options="EXPIRY_OPTIONS"
          size="small"
          style="width: 140px"
          @update:value="generate()"
        />
      </div>

      <div v-if="loading" class="hint">正在生成链接…</div>

      <template v-else-if="url">
        <NInput :value="url" readonly size="small" class="url-input">
          <template #prefix>
            <NIcon size="13" color="#909399"><LinkOutline /></NIcon>
          </template>
        </NInput>
        <div class="hint">
          任何拿到该链接的人都可在有效期内下载此对象，请谨慎分发。
        </div>
      </template>

      <div v-else-if="error" class="hint err">{{ error }}</div>
    </div>

    <template #footer>
      <div class="footer">
        <NText depth="3" style="font-size: 12px">{{ key }}</NText>
        <div class="footer-actions">
          <NButton
            size="small"
            class="btn-plain"
            :loading="loading"
            @click="generate()"
          >
            <template #icon>
              <NIcon><RefreshOutline /></NIcon>
            </template>
            重新生成
          </NButton>
          <NButton
            size="small"
            class="btn-open"
            :disabled="!url"
            @click="openInBrowser()"
          >
            在浏览器中打开
          </NButton>
          <NButton size="small" type="primary" :disabled="!url" @click="copyUrl()">
            <template #icon>
              <NIcon><CopyOutline /></NIcon>
            </template>
            复制链接
          </NButton>
        </div>
      </div>
    </template>
  </NModal>
</template>

<style scoped>
.share {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.label {
  font-size: 12px;
  color: #606266;
}

.hint {
  font-size: 12px;
  color: #909399;
  line-height: 1.6;
}

.hint.err {
  color: #d03050;
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

.footer-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

/* 「在浏览器中打开」为次要按钮：默认白底描边，仅在悬停/按下时变浅灰，
   与 primary（复制链接）、quaternary（重新生成）区分开。 */
.footer-actions :deep(.btn-open) {
  --n-color: #ffffff;
  --n-color-hover: #f2f3f5;
  --n-color-pressed: #e8eaed;
  --n-color-focus: #ffffff;
  --n-border: 1px solid #dcdfe6;
  --n-border-hover: 1px solid #dcdfe6;
  --n-border-pressed: 1px solid #dcdfe6;
  --n-border-focus: 1px solid #dcdfe6;
  --n-text-color: #303236;
  --n-text-color-hover: #303236;
  --n-text-color-pressed: #303236;
  --n-text-color-focus: #303236;
}
</style>

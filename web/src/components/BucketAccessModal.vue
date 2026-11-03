<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  NAlert,
  NButton,
  NInput,
  NModal,
  NRadio,
  NRadioGroup,
  NSpace,
  NSpin,
  NText,
  useMessage,
} from "naive-ui";
import { commands } from "../bindings";
import type { BucketAccess, BucketPolicyInfo } from "../bindings";
import { useConnectionStore } from "../stores/connections";
import { errText } from "../utils/format";
import { unwrap } from "../utils/ipc";

const props = defineProps<{ visible: boolean; bucket: string }>();
const emit = defineEmits<{ (e: "update:visible", v: boolean): void }>();

const store = useConnectionStore();
const message = useMessage();

const loading = ref(false);
const saving = ref(false);
const info = ref<BucketPolicyInfo | null>(null);
const error = ref<string | null>(null);
const choice = ref<"public" | "private">("private");
/** 原策略 JSON 默认折叠，保存前可展开备份。 */
const showJson = ref(false);

/** 原策略 JSON（能美化就美化，解析失败则原样展示）。 */
const policyText = computed(() => {
  const raw = info.value?.policyJson;
  if (!raw) return "";
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
});

async function copyPolicy() {
  if (!policyText.value) return;
  try {
    await navigator.clipboard.writeText(policyText.value);
    message.success("已复制策略 JSON");
  } catch {
    message.warning("复制失败");
  }
}

const connId = computed(() => store.activeId ?? "");

const accessLabel = (a: BucketAccess | undefined) => {
  if (a === "public") return "公开";
  if (a === "custom") return "自定义策略";
  return "私有";
};

async function load() {
  loading.value = true;
  error.value = null;
  info.value = null;
  showJson.value = false;
  try {
    info.value = unwrap(
      await commands.getBucketAccess(connId.value, props.bucket),
    );
    choice.value = info.value.access === "public" ? "public" : "private";
  } catch (e) {
    error.value = errText(e);
  } finally {
    loading.value = false;
  }
}

watch(
  () => [props.visible, props.bucket],
  () => {
    if (props.visible) void load();
  },
  { immediate: true },
);

async function save() {
  if (!info.value) return;
  saving.value = true;
  try {
    const r = unwrap(
      await commands.setBucketPublic(
        connId.value,
        props.bucket,
        choice.value === "public",
      ),
    );
    info.value = r;
    message.success(
      choice.value === "public" ? "已设为公开（全员只读）" : "已设为私有",
    );
    emit("update:visible", false);
    await store.loadBucketAccess();
  } catch (e) {
    message.error(errText(e));
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <NModal
    :show="visible"
    preset="card"
    :title="`权限 · ${props.bucket}`"
    style="width: 460px"
    size="small"
    :bordered="false"
    @update:show="emit('update:visible', $event)"
  >
    <NSpin :show="loading">
      <NSpace vertical :size="12" style="min-height: 120px">
        <template v-if="error">
          <NAlert type="error" :bordered="false">{{ error }}</NAlert>
        </template>

        <template v-else-if="info">
          <NText depth="3" style="font-size: 12px">
            当前状态：{{ accessLabel(info.access) }}
          </NText>

          <NAlert
            v-if="info.access === 'custom'"
            type="warning"
            :bordered="false"
            style="font-size: 12px"
          >
            该桶存在非本工具生成的自定义策略，保存后会将其替换，原策略将丢失。
          </NAlert>

          <NRadioGroup v-model:value="choice">
            <NSpace vertical :size="8">
              <NRadio value="private">
                私有
                <NText depth="3" style="font-size: 12px">
                  · 仅凭 Access Key 访问（删除桶策略）
                </NText>
              </NRadio>
              <NRadio value="public">
                公开
                <NText depth="3" style="font-size: 12px">
                  · 任何人可读取对象（全员只读策略）
                </NText>
              </NRadio>
            </NSpace>
          </NRadioGroup>

          <!-- 保存会整份替换策略，先给一次备份机会 -->
          <template v-if="info.policyJson">
            <div class="policy-head">
              <NButton size="tiny" quaternary @click="showJson = !showJson">
                {{ showJson ? "收起原策略 JSON" : "查看原策略 JSON" }}
              </NButton>
              <NButton
                v-if="showJson"
                size="tiny"
                quaternary
                title="复制到剪贴板"
                @click="copyPolicy()"
              >
                复制
              </NButton>
            </div>
            <NInput
              v-if="showJson"
              :value="policyText"
              type="textarea"
              readonly
              :rows="9"
              size="small"
              class="policy-json"
            />
          </template>

          <NAlert
            v-if="choice === 'public'"
            type="warning"
            :bordered="false"
            style="font-size: 12px"
          >
            设为公开后，桶内所有对象无需凭据即可被下载，请确认无敏感数据。
          </NAlert>
        </template>
      </NSpace>
    </NSpin>

    <template #footer>
      <NSpace justify="end" :size="8">
        <NButton size="small" quaternary @click="emit('update:visible', false)">
          取消
        </NButton>
        <NButton
          size="small"
          type="primary"
          :loading="saving"
          :disabled="!info"
          @click="save()"
        >
          保存
        </NButton>
      </NSpace>
    </template>
  </NModal>
</template>

<style scoped>
.policy-head {
  display: flex;
  align-items: center;
  gap: 4px;
}

.policy-json :deep(textarea) {
  font-family: ui-monospace, SFMono-Regular, Consolas, "Liberation Mono", monospace;
  font-size: 12px;
  line-height: 1.5;
}
</style>

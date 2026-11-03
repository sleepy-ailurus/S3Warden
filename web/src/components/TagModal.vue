<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  NAlert,
  NButton,
  NIcon,
  NInput,
  NModal,
  NScrollbar,
  NSpace,
  NSpin,
  NText,
  useMessage,
} from "naive-ui";
import { AddOutline, CloseOutline, SaveOutline } from "@vicons/ionicons5";
import { commands } from "../bindings";
import type { ObjectEntry, ObjectTag } from "../bindings";
import { useConnectionStore } from "../stores/connections";
import { errText } from "../utils/format";
import { unwrap } from "../utils/ipc";

const props = defineProps<{ visible: boolean; entry: ObjectEntry | null }>();
const emit = defineEmits<{ (e: "update:visible", v: boolean): void }>();

const store = useConnectionStore();
const message = useMessage();

/** S3 对象标签上限。 */
const MAX_TAGS = 10;

const loading = ref(false);
const saving = ref(false);
const error = ref<string | null>(null);
const tags = ref<ObjectTag[]>([]);

const connId = computed(() => store.activeId ?? "");
const bucket = computed(() => store.currentBucket ?? "");
const key = computed(() => props.entry?.key ?? "");

const canAdd = computed(() => tags.value.length < MAX_TAGS);

async function load() {
  loading.value = true;
  error.value = null;
  tags.value = [];
  try {
    tags.value = unwrap(
      await commands.getObjectTags(connId.value, bucket.value, key.value),
    );
    if (tags.value.length === 0) addRow();
  } catch (e) {
    error.value = errText(e);
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

function addRow() {
  if (!canAdd.value) return;
  tags.value.push({ key: "", value: "" });
}

function removeRow(idx: number) {
  tags.value.splice(idx, 1);
  if (tags.value.length === 0) addRow();
}

/** 保存前过滤空行：标签名必填，值可为空。 */
function normalize(): ObjectTag[] {
  const seen = new Set<string>();
  const out: ObjectTag[] = [];
  for (const t of tags.value) {
    const k = t.key.trim();
    if (!k || seen.has(k)) continue;
    seen.add(k);
    out.push({ key: k, value: t.value.trim() });
  }
  return out;
}

async function save() {
  const payload = normalize();
  if (payload.length > MAX_TAGS) {
    message.warning(`标签最多 ${MAX_TAGS} 组`);
    return;
  }
  saving.value = true;
  try {
    tags.value = unwrap(
      await commands.putObjectTags(connId.value, bucket.value, key.value, payload),
    );
    if (tags.value.length === 0) addRow();
    message.success(payload.length ? `已保存 ${payload.length} 个标签` : "已清空标签");
    emit("update:visible", false);
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
    :title="`标签 · ${props.entry?.name ?? ''}`"
    style="width: 520px"
    size="small"
    :bordered="false"
    @update:show="emit('update:visible', $event)"
  >
    <NSpin :show="loading">
      <NSpace vertical :size="10" style="min-height: 160px">
        <NAlert v-if="error" type="error" :bordered="false">{{ error }}</NAlert>

        <template v-else>
          <NText depth="3" style="font-size: 12px">
            标签用于对单个对象做多维度分类与策略管控（如生命周期、权限、计费归组）。
            保存会整体替换该对象的标签集合，上限 {{ MAX_TAGS }} 组。
          </NText>

          <NScrollbar style="max-height: 260px">
            <div class="tag-rows">
              <div class="tag-head">
                <span>标签名</span>
                <span>值</span>
                <span class="op"></span>
              </div>
              <div v-for="(t, i) in tags" :key="i" class="tag-row">
                <NInput v-model:value="t.key" size="small" placeholder="例如 env" />
                <NInput v-model:value="t.value" size="small" placeholder="例如 prod" />
                <NButton size="tiny" quaternary @click="removeRow(i)">
                  <template #icon>
                    <NIcon><CloseOutline /></NIcon>
                  </template>
                </NButton>
              </div>
            </div>
          </NScrollbar>

          <div>
            <NButton size="tiny" :disabled="!canAdd" @click="addRow()">
              <template #icon>
                <NIcon><AddOutline /></NIcon>
              </template>
              添加标签
            </NButton>
            <NText v-if="!canAdd" depth="3" style="font-size: 12px; margin-left: 8px">
              已达 {{ MAX_TAGS }} 组上限
            </NText>
          </div>
        </template>
      </NSpace>
    </NSpin>

    <template #footer>
      <NSpace justify="end" :size="8">
        <NButton size="small" quaternary @click="emit('update:visible', false)">
          取消
        </NButton>
        <NButton size="small" type="primary" :loading="saving" @click="save()">
          <template #icon>
            <NIcon><SaveOutline /></NIcon>
          </template>
          保存
        </NButton>
      </NSpace>
    </template>
  </NModal>
</template>

<style scoped>
.tag-rows {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.tag-head {
  display: grid;
  grid-template-columns: 1fr 1fr 28px;
  gap: 8px;
  font-size: 11px;
  color: #909399;
  padding: 0 2px;
}

.tag-row {
  display: grid;
  grid-template-columns: 1fr 1fr 28px;
  gap: 8px;
  align-items: center;
}
</style>

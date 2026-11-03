<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  NAlert,
  NButton,
  NForm,
  NFormItem,
  NInput,
  NModal,
  NRadio,
  NRadioGroup,
  NSelect,
  NSpace,
  NText,
  useDialog,
  useMessage,
} from "naive-ui";
import type { SelectOption } from "naive-ui";
import { commands } from "../bindings";
import type { BackendPreset, ConnectionTestOk } from "../bindings";
import { useConnectionStore } from "../stores/connections";
import { backendLabel, errText, presetLabel } from "../utils/format";
import { unwrap } from "../utils/ipc";

const store = useConnectionStore();
const message = useMessage();
const dialog = useDialog();

const show = computed({
  get: () => store.modalVisible,
  set: (v: boolean) => {
    store.modalVisible = v;
  },
});
const editing = computed(() => store.editing);
const isEdit = computed(() => editing.value !== null);

// 表单字段用独立 ref：vue-tsc 对 reactive 对象成员的 v-model 检查存在摩擦，
// 且 ref 写法更贴合 Vue 3 惯用法。
const preset = ref<BackendPreset>("generic_s3");
const name = ref("");
const endpoint = ref("");
const region = ref("");
const accessKey = ref("");
const secretKey = ref("");
const pathStyle = ref(true);

/** 后端接入类型下拉项（数据来自 Rust 侧单一数据源）。 */
const presetOptions = computed<SelectOption[]>(() =>
  store.presets.map((p) => ({ label: presetLabel(p.preset), value: p.preset })),
);

/** 当前选中预设的元信息（默认寻址方式、示例端点、须知）。 */
const presetInfo = computed(
  () => store.presets.find((p) => p.preset === preset.value) ?? null,
);

const endpointPlaceholder = computed(
  () => presetInfo.value?.endpointHint ?? "https://s3.example.com",
);
const regionPlaceholder = computed(
  () => presetInfo.value?.regionHint ?? "us-east-1（留空使用默认）",
);

// 切换后端时套用该后端的默认寻址方式（用户随后可手动改）
watch(preset, (v) => {
  const info = store.presets.find((p) => p.preset === v);
  if (info) pathStyle.value = info.defaultPathStyle;
});

const testing = ref(false);
const testResult = ref<ConnectionTestOk | null>(null);
const saving = ref(false);

// ---- Secret Key 回填 ----
const secretLoaded = ref(false);
const secretLoading = ref(false);

/** 编辑已有连接时回填密钥明文；读取失败则保持空白（留空 = 不修改）。 */
async function loadSecret(id: string) {
  secretLoading.value = true;
  try {
    secretKey.value = unwrap(await commands.revealSecret(id));
    secretLoaded.value = true;
  } catch {
    secretKey.value = "";
    secretLoaded.value = false;
  } finally {
    secretLoading.value = false;
  }
}

watch(show, (v) => {
  if (!v) return;
  const p = store.editing;
  preset.value = p?.preset ?? "generic_s3";
  name.value = p?.name ?? "";
  endpoint.value = p?.endpoint ?? "";
  region.value = p?.region ?? "";
  accessKey.value = p?.accessKey ?? "";
  // 编辑时回填已保存的密钥（显式读取，仅此一处回传明文）
  secretKey.value = "";
  secretLoaded.value = false;
  if (p) void loadSecret(p.id);
  pathStyle.value = p?.pathStyle ?? true;
  testResult.value = null;
});

// ---- 端点自动补 scheme ----
/** 未写 scheme 时默认补 `http://`（用户写 https:// 则保留）。 */
function normalizeEndpoint() {
  const v = endpoint.value.trim();
  if (!v) return;
  if (!/^https?:\/\//i.test(v)) endpoint.value = `http://${v}`;
}

function validateForm(requireSecret: boolean): string | null {
  normalizeEndpoint();
  if (!name.value.trim()) return "请填写连接名称";
  if (!endpoint.value.trim()) return "请填写端点";
  if (!accessKey.value.trim()) return "请填写 Access Key";
  if (requireSecret && !secretKey.value) return "请填写 Secret Key";
  return null;
}

async function runTest() {
  // 编辑模式且未填新密钥 → 测试已保存的连接
  const useSaved = isEdit.value && !secretKey.value;
  const err = validateForm(!useSaved);
  if (err) {
    message.warning(err);
    return;
  }
  testing.value = true;
  testResult.value = null;
  try {
    if (useSaved && editing.value) {
      testResult.value = unwrap(
        await commands.testSavedConnection(editing.value.id),
      );
    } else {
      testResult.value = unwrap(
        await commands.testNewConnection({
          name: name.value.trim(),
          endpoint: endpoint.value.trim(),
          region: region.value.trim() || null,
          accessKey: accessKey.value.trim(),
          secretKey: secretKey.value,
          preset: preset.value,
          pathStyle: pathStyle.value,
        }),
      );
    }
  } catch (e) {
    message.error(errText(e));
  } finally {
    testing.value = false;
  }
}

async function save() {
  const err = validateForm(!isEdit.value);
  if (err) {
    message.warning(err);
    return;
  }
  saving.value = true;
  try {
    if (isEdit.value && editing.value) {
      unwrap(
        await commands.updateConnection(editing.value.id, {
          name: name.value.trim(),
          endpoint: endpoint.value.trim(),
          region: region.value.trim(),
          accessKey: accessKey.value.trim(),
          secretKey: secretKey.value || null,
          preset: preset.value,
          pathStyle: pathStyle.value,
        }),
      );
      message.success("连接已更新");
    } else {
      const created = unwrap(
        await commands.createConnection({
          name: name.value.trim(),
          endpoint: endpoint.value.trim(),
          region: region.value.trim() || null,
          accessKey: accessKey.value.trim(),
          secretKey: secretKey.value,
          preset: preset.value,
          pathStyle: pathStyle.value,
        }),
      );
      message.success(
        created.backend
          ? `连接已创建，识别为 ${backendLabel(created.backend)}`
          : "连接已创建",
      );
      store.select(created.id);
    }
    await store.refreshProfiles();
    show.value = false;
  } catch (e) {
    message.error(errText(e));
  } finally {
    saving.value = false;
  }
}

function removeConnection() {
  const p = editing.value;
  if (!p) return;
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
        show.value = false;
        message.success("连接已删除");
      } catch (e) {
        message.error(errText(e));
      }
    },
  });
}
</script>

<template>
  <NModal
    v-model:show="show"
    preset="card"
    :title="isEdit ? '编辑连接' : '新建连接'"
    style="width: 560px"
    :mask-closable="false"
  >
    <NForm label-placement="left" label-width="92" require-mark-placement="left">
      <NFormItem label="服务类型" required>
        <NSelect v-model:value="preset" :options="presetOptions" />
      </NFormItem>
      <NFormItem label="名称" required>
        <NInput v-model:value="name" placeholder="例如：本地 MinIO" />
      </NFormItem>
      <NFormItem label="端点" required>
        <NInput
          v-model:value="endpoint"
          :placeholder="endpointPlaceholder"
          @blur="normalizeEndpoint()"
        />
      </NFormItem>
      <NFormItem label="Region">
        <NInput v-model:value="region" :placeholder="regionPlaceholder" />
      </NFormItem>
      <NFormItem label="Access Key" required>
        <NInput v-model:value="accessKey" placeholder="Access Key" />
      </NFormItem>
      <NFormItem label="Secret Key" :required="!isEdit">
        <NInput
          v-model:value="secretKey"
          type="password"
          show-password-on="click"
          :placeholder="
            secretLoading
              ? '正在读取已保存的密钥…'
              : isEdit
                ? '留空则不修改'
                : 'Secret Key'
          "
          :loading="secretLoading"
        />
      </NFormItem>
      <NFormItem v-if="isEdit" label=" ">
        <NText depth="3" style="font-size: 12px">
          {{
            secretLoaded
              ? "已回填系统凭据存储中的密钥，点击输入框右侧的眼睛可查看；不改动则保持原值。"
              : "未读取到已保存密钥，留空保存则不修改原密钥。"
          }}
        </NText>
      </NFormItem>
      <NFormItem label="寻址方式">
        <NRadioGroup v-model:value="pathStyle">
          <NRadio :value="true">Path-Style（MinIO 推荐）</NRadio>
          <NRadio :value="false">Virtual-Host</NRadio>
        </NRadioGroup>
      </NFormItem>
    </NForm>

    <NSpace vertical :size="10">
      <NText v-if="presetInfo" depth="3" style="font-size: 12px">
        {{ presetInfo.notes }}
      </NText>
      <div>
        <NButton size="small" :loading="testing" @click="runTest">测试连接</NButton>
      </div>
      <NAlert v-if="testResult" type="success" :show-icon="true">
        连接成功 · 延迟 {{ testResult.latencyMs }}ms ·
        {{ backendLabel(testResult.backend) }} · {{ testResult.bucketCount }} 个桶
      </NAlert>
    </NSpace>

    <template #footer>
      <div class="modal-footer">
        <div>
          <NButton v-if="isEdit" size="small" type="error" quaternary @click="removeConnection">
            删除连接
          </NButton>
        </div>
        <NSpace>
          <NButton size="small" @click="show = false">取消</NButton>
          <NButton size="small" type="primary" :loading="saving" @click="save">保存</NButton>
        </NSpace>
      </div>
    </template>
  </NModal>
</template>

<style scoped>
.modal-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
</style>

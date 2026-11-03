<script setup lang="ts">
import { ref, watch } from "vue";
import { NButton, NInput, NModal, NSpace, NText } from "naive-ui";

const props = defineProps<{
  visible: boolean;
  title: string;
  label: string;
  /** 初始值（打开时回填）。 */
  initial: string;
  placeholder?: string;
  hint?: string;
  confirmText?: string;
  loading?: boolean;
}>();

const emit = defineEmits<{
  (e: "update:visible", v: boolean): void;
  (e: "submit", value: string): void;
}>();

const value = ref("");

watch(
  () => props.visible,
  (v) => {
    if (v) value.value = props.initial;
  },
  { immediate: true },
);

function submit() {
  const v = value.value.trim();
  if (!v) return;
  emit("submit", v);
}
</script>

<template>
  <NModal
    :show="visible"
    preset="card"
    :title="title"
    style="width: 420px"
    size="small"
    :bordered="false"
    @update:show="emit('update:visible', $event)"
  >
    <NSpace vertical :size="10">
      <NText depth="3" style="font-size: 12px">{{ label }}</NText>
      <NInput
        v-model:value="value"
        size="small"
        :placeholder="placeholder"
        @keydown.enter="submit()"
      />
      <NText v-if="hint" depth="3" style="font-size: 12px">{{ hint }}</NText>
    </NSpace>

    <template #footer>
      <NSpace justify="end" :size="8">
        <NButton size="small" quaternary @click="emit('update:visible', false)">
          取消
        </NButton>
        <NButton
          size="small"
          type="primary"
          :loading="loading"
          :disabled="!value.trim()"
          @click="submit()"
        >
          {{ confirmText ?? "保存" }}
        </NButton>
      </NSpace>
    </template>
  </NModal>
</template>

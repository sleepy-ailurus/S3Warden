<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { NIcon } from "naive-ui";
import type { ContextMenuItem } from "./context-menu";

const props = defineProps<{
  /** 触发位置（视口坐标）。 */
  x: number;
  y: number;
  items: ContextMenuItem[];
}>();

const emit = defineEmits<{ (e: "close"): void }>();

const el = ref<HTMLElement | null>(null);
const pos = ref({ left: props.x, top: props.y });

const style = computed(() => ({
  left: `${pos.value.left}px`,
  top: `${pos.value.top}px`,
}));

/** 贴边时向内翻转，保证菜单完整可见。 */
async function clamp() {
  await nextTick();
  const node = el.value;
  if (!node) return;
  const r = node.getBoundingClientRect();
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  let left = props.x;
  let top = props.y;
  if (left + r.width > vw - 8) left = Math.max(8, vw - r.width - 8);
  if (top + r.height > vh - 8) top = Math.max(8, top - r.height);
  pos.value = { left, top };
}

function pick(item: ContextMenuItem) {
  if (item.disabled || item.separator || !item.action) return;
  emit("close");
  item.action();
}

/** 点击别处 / 按 Esc / 滚动 / 窗口尺寸变化都关闭。 */
function onWindowMouseDown(e: MouseEvent) {
  if (!el.value?.contains(e.target as Node)) emit("close");
}
function onKeyDown(e: KeyboardEvent) {
  if (e.key === "Escape") emit("close");
}

onMounted(() => {
  void clamp();
  window.addEventListener("mousedown", onWindowMouseDown, true);
  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("resize", () => emit("close"));
  window.addEventListener("blur", () => emit("close"));
});

onBeforeUnmount(() => {
  window.removeEventListener("mousedown", onWindowMouseDown, true);
  window.removeEventListener("keydown", onKeyDown);
});
</script>

<template>
  <div ref="el" class="ctx-menu" :style="style" @contextmenu.prevent>
    <template v-for="(item, i) in items" :key="i">
      <div v-if="item.separator" class="ctx-sep" />
      <button
        v-else
        class="ctx-item"
        :class="{ danger: item.danger }"
        :disabled="item.disabled"
        type="button"
        @click="pick(item)"
      >
        <NIcon v-if="item.icon" size="15" class="ctx-icon">
          <component :is="item.icon" />
        </NIcon>
        <span class="ctx-label">{{ item.label }}</span>
        <span v-if="item.hint" class="ctx-hint">{{ item.hint }}</span>
      </button>
    </template>
  </div>
</template>

<style scoped>
.ctx-menu {
  position: fixed;
  z-index: 3000;
  min-width: 148px;
  padding: 4px;
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  box-shadow: 0 8px 24px rgba(15, 23, 42, 0.12);
}

.ctx-item {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  background: transparent;
  border: none;
  border-radius: 6px;
  font-size: 13px;
  color: #303236;
  cursor: pointer;
  text-align: left;
}

.ctx-item:hover:not(:disabled) {
  background: #f2f3f5;
}

.ctx-item:disabled {
  color: #c0c4cc;
  cursor: default;
}

.ctx-sep {
  height: 1px;
  margin: 4px 6px;
  background: #ebedf0;
}

.ctx-item.danger {
  color: #d03050;
}

.ctx-icon {
  flex-shrink: 0;
  color: #6b7280;
}

.ctx-item.danger .ctx-icon {
  color: #d03050;
}

.ctx-label {
  flex: 1;
  white-space: nowrap;
}

.ctx-hint {
  font-size: 11px;
  color: #c0c4cc;
}
</style>

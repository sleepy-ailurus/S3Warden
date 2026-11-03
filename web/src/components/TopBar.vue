<script setup lang="ts">
import { NButton, NIcon } from "naive-ui";
import { AddOutline } from "@vicons/ionicons5";
import { useConnectionStore } from "../stores/connections";

const store = useConnectionStore();
</script>

<template>
  <header class="topbar">
    <div class="brand">
      <svg class="logo" width="22" height="22" viewBox="0 0 24 24" aria-hidden="true">
        <defs>
          <linearGradient id="swGrad" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stop-color="#1e3a8a" />
            <stop offset="1" stop-color="#2563eb" />
          </linearGradient>
        </defs>
        <!-- 与应用图标一致：桶盖 + 上宽下窄的桶身 + 两条分层条纹 -->
        <rect x="4" y="3.6" width="16" height="2.8" rx="1.3" fill="url(#swGrad)" />
        <path
          d="M5.4 7.2h13.2l-1.4 12a1.3 1.3 0 0 1-1.3 1.2H8.1a1.3 1.3 0 0 1-1.3-1.2z"
          fill="url(#swGrad)"
        />
        <rect x="7.4" y="10.4" width="9.2" height="1.5" rx="0.75" fill="#ffffff" />
        <rect x="8" y="14.2" width="8" height="1.5" rx="0.75" fill="#ffffff" />
      </svg>
      <div>
        <div class="brand-name">S3Warden</div>
        <div class="brand-sub">S3 管理控制台</div>
      </div>
    </div>
    <div class="actions">
      <NButton size="small" type="primary" secondary @click="store.openCreate()">
        <template #icon>
          <NIcon><AddOutline /></NIcon>
        </template>
        新建连接
      </NButton>
    </div>
  </header>
</template>

<style scoped>
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 52px;
  padding: 0 16px;
  background: #ffffff;
  border-bottom: 1px solid #e5e7eb;
  flex-shrink: 0;
}

.brand {
  display: flex;
  align-items: center;
  gap: 10px;
}

.logo {
  flex-shrink: 0;
}

.brand-name {
  font-size: 15px;
  font-weight: 600;
  line-height: 1.2;
}

.brand-sub {
  font-size: 11px;
  color: #909399;
  line-height: 1.2;
}
</style>

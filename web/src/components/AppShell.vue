<script setup lang="ts">
import { onMounted } from "vue";
import { useMessage } from "naive-ui";
import TopBar from "./TopBar.vue";
import ConnectionSidebar from "./ConnectionSidebar.vue";
import ConnectionModal from "./ConnectionModal.vue";
import TransferPanel from "./TransferPanel.vue";
import BrowserView from "../views/BrowserView.vue";
import { useConnectionStore } from "../stores/connections";
import { useTransferStore } from "../stores/transfers";
import { errText } from "../utils/format";

const store = useConnectionStore();
const transfers = useTransferStore();
const message = useMessage();

onMounted(async () => {
  void transfers.init();
  try {
    await store.loadPresets();
    await store.refreshProfiles();
    const def = store.profiles.find((p) => p.isDefault) ?? store.profiles[0];
    if (def) store.select(def.id);
  } catch (e) {
    message.error(errText(e));
  }
});
</script>

<template>
  <div class="shell">
    <TopBar />
    <div class="shell-body">
      <ConnectionSidebar />
      <main class="shell-main">
        <BrowserView />
      </main>
    </div>
    <ConnectionModal />
    <TransferPanel />
  </div>
</template>

<style scoped>
.shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

.shell-body {
  display: flex;
  flex: 1;
  min-height: 0;
}

.shell-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
</style>

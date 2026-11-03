import { createApp } from "vue";
import { createPinia } from "pinia";
import naive from "naive-ui";
import App from "./App.vue";
import "./styles/main.css";

/**
 * 屏蔽 WebView 原生右键菜单（返回 / 刷新 / 另存为 / 打印 / 更多工具 / 检查）。
 *
 * 桌面应用里这些是浏览器遗留项，与业务无关且会暴露 DevTools 入口。
 * Tauri 2 无配置项可直接关闭，故在前端统一拦截（含输入框，全应用无右键菜单）；
 * 复制 / 剪切 / 粘贴等仍可用键盘快捷键（Ctrl+C / Ctrl+X / Ctrl+V / Ctrl+A）。
 */
document.addEventListener("contextmenu", (e) => {
  e.preventDefault();
});

createApp(App).use(createPinia()).use(naive).mount("#app");

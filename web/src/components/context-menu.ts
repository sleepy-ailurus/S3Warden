/** 业务右键菜单项（与 `ContextMenu.vue` 配套）。 */
export interface ContextMenuItem {
  /** 菜单项文字（分隔线时可省略）。 */
  label?: string;
  /** 图标组件（vicons 组件）。 */
  icon?: unknown;
  /** 右侧补充说明。 */
  hint?: string;
  /** 危险操作，红色文字。 */
  danger?: boolean;
  disabled?: boolean;
  /** 渲染为分隔线，不带文字与点击行为。 */
  separator?: boolean;
  action?: () => void;
}

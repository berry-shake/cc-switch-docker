/**
 * 是否是 cc-switch-server 的 Web 面板构建（`pnpm build:web`，见 vite.web.config.ts）。
 *
 * Web 面板只读：删除会话、在终端恢复、在文件管理器中显示这类改动本机或依赖桌面环境的操作要藏起来。
 * 桌面版构建里 Vite 把它替换成常量 false，相关分支会被摇树去掉。
 */
export const IS_WEB_BUILD = import.meta.env.VITE_CC_SWITCH_WEB === "1";

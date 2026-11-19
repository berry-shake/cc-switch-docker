/** Web 版没有原生窗口：返回一个所有方法都是空操作的对象 */
const noopWindow: Record<string, unknown> = new Proxy(
  {},
  {
    get: (_target, prop) => {
      if (prop === "then") return undefined;
      if (prop === "label") return "main";
      return async () => () => {};
    },
  },
);

export function getCurrentWindow() {
  return noopWindow as never;
}

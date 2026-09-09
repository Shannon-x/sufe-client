import { invoke as nativeInvoke, isTauri } from "@tauri-apps/api/core";
import {
  listen as nativeListen,
  type EventCallback,
  type UnlistenFn,
} from "@tauri-apps/api/event";
export const isDesktop = isTauri();
// Preview exists only in the development build, always with a visible banner.
// Never substitutes for a failed native call or sends demonstration payments.
export const isPreview =
  import.meta.env.DEV &&
  !isDesktop &&
  new URLSearchParams(location.search).get("preview") === "1";
export async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (isDesktop) return nativeInvoke<T>(command, args);
  if (isPreview)
    return (await import("./preview/fixtures")).previewInvoke(
      command,
      args,
    ) as Promise<T>;
  throw {
    kind: "desktop_required",
    message: "请在 Sufe 桌面客户端中使用此功能。当前是浏览器界面。",
  };
}
export async function listen<T>(
  event: string,
  callback: EventCallback<T>,
): Promise<UnlistenFn> {
  if (isDesktop) return nativeListen<T>(event, callback);
  const handler = (e: Event) =>
    callback({ event, id: 0, payload: (e as CustomEvent<T>).detail });
  window.addEventListener(event, handler);
  return () => window.removeEventListener(event, handler);
}

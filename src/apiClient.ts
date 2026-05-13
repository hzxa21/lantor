import { convertFileSrc, invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

const UI_REFRESH_EVENT = "lantor://refresh";
const WEB_TOKEN_STORAGE_KEY = "lantor.webToken";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export function isTauriRuntime() {
  return typeof window !== "undefined" && Boolean(window.__TAURI_INTERNALS__);
}

function webToken() {
  if (typeof window === "undefined") return "";
  const url = new URL(window.location.href);
  const tokenFromUrl = url.searchParams.get("token") || "";
  if (tokenFromUrl) {
    window.localStorage.setItem(WEB_TOKEN_STORAGE_KEY, tokenFromUrl);
    return tokenFromUrl;
  }
  return window.localStorage.getItem(WEB_TOKEN_STORAGE_KEY) || "";
}

function apiPath(command: string) {
  return `/api/${command}`;
}

export async function apiInvoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (isTauriRuntime()) {
    return tauriInvoke<T>(command, args);
  }

  const headers: Record<string, string> = {};
  const token = webToken();
  if (token) headers["x-lantor-token"] = token;

  const response = command === "bootstrap"
    ? await fetch(apiPath("bootstrap"), { headers })
    : await fetch(apiPath(command), {
      method: "POST",
      headers: {
        ...headers,
        "content-type": "application/json",
      },
      body: JSON.stringify(args),
    });

  const contentType = response.headers.get("content-type") || "";
  const payload = contentType.includes("application/json")
    ? await response.json()
    : await response.text();
  if (!response.ok) {
    const message = typeof payload === "object" && payload && "message" in payload
      ? String((payload as { message: unknown }).message)
      : String(payload || `${command} failed`);
    throw new Error(message);
  }
  return payload as T;
}

export async function subscribeBackendEvents(handler: (payload: string) => void): Promise<UnlistenFn> {
  if (isTauriRuntime()) {
    return tauriListen<string>(UI_REFRESH_EVENT, (event) => handler(event.payload));
  }

  const token = webToken();
  const url = token ? `/api/events?token=${encodeURIComponent(token)}` : "/api/events";
  const source = new EventSource(url);
  source.addEventListener("lantor", (event) => {
    handler((event as MessageEvent<string>).data);
  });
  source.onerror = () => {
    console.error("Lantor web event stream disconnected");
  };
  return () => source.close();
}

export function attachmentAssetUrl(storagePath: string, attachmentId: string) {
  if (isTauriRuntime()) {
    return convertFileSrc(storagePath);
  }
  const token = webToken();
  return token
    ? `/api/attachments/${attachmentId}?token=${encodeURIComponent(token)}`
    : `/api/attachments/${attachmentId}`;
}

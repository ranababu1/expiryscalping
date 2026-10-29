// Device token storage. In the desktop app the token lives in the OS
// credential store (Windows Credential Manager) through Tauri commands.
// In a plain browser (vite dev) it is kept in memory only.
import { invoke, isTauri } from "@tauri-apps/api/core";

let memory: string | null = null;

export const secureStorage = (): boolean => isTauri();

export async function loadToken(): Promise<string | null> {
  if (!isTauri()) return memory;
  try {
    return await invoke<string | null>("token_get");
  } catch {
    return null;
  }
}

export async function saveToken(token: string): Promise<void> {
  if (!isTauri()) {
    memory = token;
    return;
  }
  await invoke("token_set", { token });
}

export async function clearToken(): Promise<void> {
  memory = null;
  if (isTauri()) await invoke("token_clear");
}

const URL_KEY = "es.engine_url";
export const DEFAULT_URL = "ws://127.0.0.1:7400";

/** The engine URL is not a secret, so it lives in localStorage. */
export function loadUrl(): string {
  try {
    return localStorage.getItem(URL_KEY) ?? DEFAULT_URL;
  } catch {
    return DEFAULT_URL;
  }
}

export function saveUrl(url: string): void {
  try {
    localStorage.setItem(URL_KEY, url);
  } catch {
    // Storage unavailable: keep the default next time.
  }
}

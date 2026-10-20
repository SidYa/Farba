// Persistent UI settings in localStorage (WebView2 keeps it in the app's data folder).
// Every access is guarded: storage can be unavailable, and the app must work without it.

const PREFIX = "farba.";

export function loadSetting<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(PREFIX + key);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

export function saveSetting<T>(key: string, value: T): void {
  try {
    localStorage.setItem(PREFIX + key, JSON.stringify(value));
  } catch {
    // Storage full or unavailable: the setting simply is not remembered.
  }
}

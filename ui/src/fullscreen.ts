import { getCurrentWindow } from "@tauri-apps/api/window";

let inflight: Promise<boolean> | null = null;

export async function isAppFullscreen(): Promise<boolean> {
  return getCurrentWindow().isFullscreen();
}

export async function setAppFullscreen(on: boolean): Promise<boolean> {
  const win = getCurrentWindow();
  if (document.fullscreenElement) {
    await document.exitFullscreen().catch(() => undefined);
  }
  await win.setFullscreen(on);
  return win.isFullscreen();
}

export async function toggleAppFullscreen(): Promise<boolean> {
  if (inflight) return inflight;
  inflight = (async () => {
    const win = getCurrentWindow();
    if (document.fullscreenElement) {
      await document.exitFullscreen().catch(() => undefined);
    }
    const now = await win.isFullscreen();
    await win.setFullscreen(!now);
    return win.isFullscreen();
  })();
  try {
    return await inflight;
  } finally {
    inflight = null;
  }
}

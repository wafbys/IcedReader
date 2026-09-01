import { getCurrentWindow } from "@tauri-apps/api/window";

export async function isAppFullscreen(): Promise<boolean> {
  return getCurrentWindow().isFullscreen();
}

export async function setAppFullscreen(on: boolean): Promise<void> {
  await getCurrentWindow().setFullscreen(on);
}

export async function toggleAppFullscreen(): Promise<boolean> {
  const win = getCurrentWindow();
  const next = !(await win.isFullscreen());
  await win.setFullscreen(next);
  return next;
}

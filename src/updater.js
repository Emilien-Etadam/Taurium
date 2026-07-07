// Thin wrapper around the Tauri updater/process plugins.
// The updater compares the app version to the manifest published on GitHub
// Releases (see plugins.updater.endpoints in tauri.conf.json) and verifies the
// downloaded installer against the embedded public key before running it.
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

// Returns the Update object when a newer signed version is available, else null.
export async function checkForUpdate() {
  return await check();
}

// Download + install the update, reporting progress via onEvent, then relaunch.
export async function installAndRelaunch(update, onEvent) {
  await update.downloadAndInstall(onEvent);
  await relaunch();
}

import { invokeSafe, invokeSafeVoid } from './invokeSafe';
import type { FrontmostApplication } from 'asyar-sdk/contracts';

export async function getFrontmostApplication(): Promise<FrontmostApplication | null> {
  return invokeSafe<FrontmostApplication>('get_frontmost_application', undefined, { silent: true });
}

export async function appIsRunning(bundleId: string): Promise<boolean> {
  return invokeSafe<boolean>('app_is_running', { bundleId });
}

// `reveal_application` is `Result<(), AppError>` on the Rust side; failures
// reject with IpcError.

export async function revealApplication(path: string): Promise<true> {
  return invokeSafeVoid('reveal_application', { path });
}

export async function applicationIndexSubscribe(
  extensionId: string | null,
  eventTypes: string[],
): Promise<string> {
  return invokeSafe<string>('application_index_subscribe', { extensionId, eventTypes });
}

export async function applicationIndexUnsubscribe(
  extensionId: string | null,
  subscriptionId: string,
): Promise<boolean> {
  return invokeSafeVoid('application_index_unsubscribe', { extensionId, subscriptionId });
}

// `set_application_scan_paths` is `Result<(), AppError>` on the Rust side — use
// invokeSafeVoid's boolean signal, not invokeSafe's null.
export async function setApplicationScanPaths(paths: string[]): Promise<boolean> {
  return invokeSafeVoid('set_application_scan_paths', { paths });
}

export async function appUpdaterShouldShowWhatsNew(
  lastSeenVersion: string | undefined,
  currentVersion: string,
): Promise<boolean> {
  return invokeSafe<boolean>('app_updater_should_show_whats_new', {
    lastSeenVersion,
    currentVersion,
  });
}

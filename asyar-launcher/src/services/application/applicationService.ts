import { settingsService } from '../settings/settingsService.svelte';
import type { FrontmostApplication } from 'asyar-sdk/contracts';
import {
  getFrontmostApplication,
  appIsRunning,
  revealApplication,
} from '../../lib/ipc/applicationCommands';
import { syncApplicationIndex, listApplications } from '../../lib/ipc/commands';

/**
 * Host-side service that fulfils the query half of `ApplicationService`
 * (the `application:*` IPC namespace). It does NOT implement the SDK's
 * `IApplicationService` directly because the `on*` push subscriptions are
 * a client-side concern — those route through `appEventsService` on the
 * `appEvents:*` namespace, not through this service.
 */
export class ApplicationService {
  async getFrontmostApplication(): Promise<FrontmostApplication | null> {
    return getFrontmostApplication();
  }

  async syncApplicationIndex(
    extraPaths?: string[],
  ): Promise<{ added: number; removed: number; total: number }> {
    const paths = extraPaths ?? settingsService.currentSettings.search.additionalScanPaths ?? [];
    return syncApplicationIndex(paths);
  }

  async listApplications(extraPaths?: string[]): Promise<any[]> {
    const paths = extraPaths ?? settingsService.currentSettings.search.additionalScanPaths ?? [];
    return listApplications(paths);
  }

  /**
   * The ExtensionIpcRouter flattens SDK proxy payload `{ bundleId }` into a
   * positional `bundleId: string` (same mechanism used by the other query
   * methods on this service). The `application` namespace is NOT in
   * `INJECTS_EXTENSION_ID`, so the router does NOT prepend extensionId —
   * per-call permission enforcement is handled by the frontend gate. The
   * Rust command receives `extension_id: None` and falls through its
   * defense-in-depth check as a core-context call.
   */
  async isRunning(bundleId: string): Promise<boolean> {
    return appIsRunning(bundleId);
  }

  /**
   * Reveals the application in the file manager.
   * Tier 1 only — not exposed through the SDK.
   */
  async revealApplication(path: string): Promise<void> {
    await revealApplication(path);
  }
}

export const applicationService = new ApplicationService();

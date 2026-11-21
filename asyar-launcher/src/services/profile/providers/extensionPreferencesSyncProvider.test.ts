import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ExtensionPreferencesSyncProvider } from './extensionPreferencesSyncProvider';
import type { SyncProviderData } from '../types';

const mockExport = vi.hoisted(() => ({
  rows: [
    {
      extensionId: 'ext1',
      commandId: null,
      key: 'foo',
      value: '"bar"',
      isEncrypted: false,
      updatedAt: 1000,
    },
    {
      extensionId: 'ext2',
      commandId: 'cmd1',
      key: 'baz',
      value: '"qux"',
      isEncrypted: false,
      updatedAt: 2000,
    },
  ],
}));

vi.mock('../../../lib/ipc/extensionPreferencesCommands', () => ({
  extensionPreferencesExportAll: vi.fn().mockResolvedValue(mockExport),
  extensionPreferencesImportAll: vi
    .fn()
    .mockResolvedValue({ itemsAdded: 0, itemsUpdated: 2, itemsSkipped: 0 }),
}));

describe('ExtensionPreferencesSyncProvider', () => {
  let provider: ExtensionPreferencesSyncProvider;

  beforeEach(() => {
    vi.clearAllMocks();
    provider = new ExtensionPreferencesSyncProvider();
  });

  it('has correct metadata', () => {
    expect(provider.id).toBe('extension-preferences');
    expect(provider.syncTier).toBe('core');
    expect(provider.defaultEnabled).toBe(true);
    expect(provider.defaultConflictStrategy).toBe('replace');
    expect(provider.sensitiveFields).toEqual(['password']);
  });

  it('exportFull returns the IPC export payload', async () => {
    const result = await provider.exportFull();
    expect(result.providerId).toBe('extension-preferences');
    expect(result.version).toBe(1);
    expect(result.data).toEqual(mockExport);
  });

  it('preview counts conflicts and new items by composite key', async () => {
    const incoming: SyncProviderData = {
      providerId: 'extension-preferences',
      version: 1,
      exportedAt: Date.now(),
      data: {
        rows: [
          {
            extensionId: 'ext1',
            commandId: null,
            key: 'foo',
            value: '"updated"',
            isEncrypted: false,
            updatedAt: 5000,
          },
          {
            extensionId: 'ext1',
            commandId: null,
            key: 'new',
            value: '"x"',
            isEncrypted: false,
            updatedAt: 5000,
          },
        ],
      },
    };
    const preview = await provider.preview(incoming);
    expect(preview.localCount).toBe(2);
    expect(preview.incomingCount).toBe(2);
    expect(preview.conflicts).toBe(1);
    expect(preview.newItems).toBe(1);
  });

  it('applyImport replace — calls IPC import with strategy', async () => {
    const { extensionPreferencesImportAll } =
      await import('../../../lib/ipc/extensionPreferencesCommands');
    const incoming: SyncProviderData = {
      providerId: 'extension-preferences',
      version: 1,
      exportedAt: Date.now(),
      data: { rows: [] },
    };
    await provider.applyImport(incoming, 'replace');
    expect(extensionPreferencesImportAll).toHaveBeenCalledWith({ rows: [] }, 'replace');
  });

  it('applyImport skip — does not call IPC', async () => {
    const { extensionPreferencesImportAll } =
      await import('../../../lib/ipc/extensionPreferencesCommands');
    const incoming: SyncProviderData = {
      providerId: 'extension-preferences',
      version: 1,
      exportedAt: Date.now(),
      data: { rows: [] },
    };
    const result = await provider.applyImport(incoming, 'skip');
    expect(extensionPreferencesImportAll).not.toHaveBeenCalled();
    expect(result.itemsAdded).toBe(0);
  });

  it('getLocalSummary returns the row count', async () => {
    const summary = await provider.getLocalSummary();
    expect(summary.itemCount).toBe(2);
    expect(summary.label).toBe('2 preferences');
  });
});

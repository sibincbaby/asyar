import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../log/logService', () => ({
  logService: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('../search/SearchService', () => ({ searchService: { resetIndex: vi.fn() } }));
vi.mock('tauri-plugin-clipboard-x-api', () => ({
  writeText: vi.fn().mockResolvedValue(undefined),
}));

const mockSearchOrchestrator = vi.hoisted(() => ({ items: [] as any[] }));
vi.mock('../search/searchOrchestrator.svelte', () => ({
  searchOrchestrator: mockSearchOrchestrator,
}));

const mockSearchStores = vi.hoisted(() => ({ selectedIndex: -1 }));
vi.mock('../search/stores/search.svelte', () => ({ searchStores: mockSearchStores }));

vi.mock('../feedback/feedbackService.svelte', () => ({
  feedbackService: { showHUD: vi.fn(), confirmAlert: vi.fn() },
}));
vi.mock('../application/applicationService', () => ({
  applicationService: { revealApplication: vi.fn() },
}));

import { ActionService, setSelectedItemProvider } from './actionService.svelte';

describe('application location actions', () => {
  const desktopEntry = {
    objectId: 'app_foo',
    name: 'Foo',
    type: 'application' as const,
    score: 1,
    path: '/usr/share/applications/foo.desktop',
  };

  beforeEach(() => {
    mockSearchOrchestrator.items = [desktopEntry];
    mockSearchStores.selectedIndex = 0;
    setSelectedItemProvider(() => mockSearchOrchestrator.items[0]);
  });

  it('offers reveal and copy path', () => {
    const svc = new ActionService();
    for (const id of ['reveal_application', 'copy_application_path']) {
      expect(svc.getAllActions().find((a) => a.id === id)!.visible!(), id).toBe(true);
    }
  });

  it('registers no uninstall or run-as-administrator action', () => {
    const ids = new ActionService().getAllActions().map((a) => a.id);
    expect(ids).not.toContain('uninstall_application');
    expect(ids).not.toContain('run_application_as_admin');
  });
});

// @vitest-environment jsdom
import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}));

vi.mock('../../services/i18n', () => ({
  t: (key: string) => {
    if (key === 'actions.title') return 'Actions';
    if (key === 'common.show_more') return 'Show More';
    return key;
  },
}));

vi.mock('../../services/action/actionService.svelte', () => ({
  actionService: {
    filteredActions: [],
    activeViewPrimaryAction: null,
  },
}));

vi.mock('../../services/extension/viewManager.svelte', () => ({
  viewManager: {
    activeView: null,
    activeViewPrimaryActionLabel: null,
  },
}));

vi.mock('../../services/extension/extensionManager.svelte', () => ({
  default: {
    getManifestById: () => null,
  },
}));

vi.mock('../../services/feedback/feedbackService.svelte', () => ({
  feedbackService: {
    current: null,
  },
}));

import { viewManager } from '../../services/extension/viewManager.svelte';
import BottomActionBar from './BottomActionBar.svelte';

describe('BottomActionBar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('suppresses search selectedItem action when an extension view is active without activeViewPrimaryActionLabel', () => {
    (viewManager as any).activeView = 'create-extension/BuildProgressView';
    (viewManager as any).activeViewPrimaryActionLabel = null;

    const dummyItem: any = {
      id: 'cmd-1',
      name: 'Build Extension with AI',
      type: 'command',
    };

    render(BottomActionBar, {
      props: {
        selectedItem: dummyItem,
        isActionListOpen: false,
        onactionListToggled: () => {},
        onactionListClosed: () => {},
      },
    });

    expect(screen.queryByText('Run')).toBeNull();
  });
});

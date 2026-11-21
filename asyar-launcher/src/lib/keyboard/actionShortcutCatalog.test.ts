import { describe, it, expect } from 'vitest';
import { actionShortcutEntries } from './actionShortcutCatalog';

describe('actionShortcutEntries', () => {
  const actions = [
    { label: 'New Note', shortcut: 'Mod+N' },
    { label: 'Copy Path', shortcut: 'Mod+Shift+C' },
    { label: 'No shortcut' },
    { label: 'New Note', shortcut: 'Mod+N' },
  ];

  it('lists only actions that declare a shortcut, once each', () => {
    expect(actionShortcutEntries(actions).map((e) => e.label)).toEqual(['New Note', 'Copy Path']);
  });

  it('renders Ctrl/Shift keycaps', () => {
    expect(actionShortcutEntries(actions)[1].keys).toEqual(['Ctrl', 'Shift', 'C']);
  });
});

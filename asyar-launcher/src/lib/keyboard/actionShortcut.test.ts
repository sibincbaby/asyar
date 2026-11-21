// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import {
  validateActionShortcut,
  assertActionShortcut,
  eventToActionChord,
  resolveActionChord,
  RESERVED_ACTION_SHORTCUTS,
} from './actionShortcut';

const ev = (init: KeyboardEventInit & { code?: string }) => new KeyboardEvent('keydown', init);

describe('validateActionShortcut', () => {
  it.each([
    'Mod+N',
    'Mod+.',
    'Mod+Shift+C',
    'Mod+Alt+C',
    'Mod+Alt+Shift+F',
    'Mod+Shift+,',
    'Enter',
    'Alt+Enter',
    'Mod+Enter',
    'Mod+Alt+Enter',
    'Mod+Shift+Enter',
    'Control+Mod+Enter',
    'Control+X',
    'Control+Shift+X',
    'F5',
  ])('accepts canonical %s', (s) => {
    expect(validateActionShortcut(s)).toBeNull();
  });

  it.each([
    ['⌘N', 'glyph'],
    ['Cmd+N', 'alias'],
    ['Ctrl+N', 'alias'],
    ['Control+N', 'literal control'],
    ['Control+Enter', 'literal control on Enter without Mod'],
    ['Super+N', 'super'],
    ['Meta+N', 'meta'],
    ['mod+n', 'lowercase'],
    ['Mod+n', 'lowercase key'],
    ['Shift+Mod+N', 'non-canonical order'],
    ['Alt+Mod+N', 'non-canonical order'],
    ['Mod+Control+Enter', 'non-canonical order'],
    ['', 'empty'],
    ['Mod+', 'missing key'],
    ['Mod', 'modifier only'],
    ['Mod+Mod+N', 'duplicate modifier'],
    ['N', 'plain letter'],
    ['Shift+N', 'typing chord'],
    ['Shift+Enter', 'newline chord in textareas'],
    ['Alt+N', 'no Mod'],
    ['Space', 'plain key'],
    ['Mod+Space', 'unbindable key'],
    ['Mod+Alt+Shift+Space', 'unbindable key'],
    ['Escape', 'plain key'],
    ['Mod+Tab', 'unbindable key'],
    ['Mod+Escape', 'unbindable key'],
    ['Mod+ArrowUp', 'navigation key'],
    ['Mod+Backspace', 'text editing'],
    ['Mod+Delete', 'text editing'],
    ['Mod+N+M', 'two keys'],
    [' Mod+N', 'whitespace'],
  ])('rejects %s (%s)', (s) => {
    expect(validateActionShortcut(s)).not.toBeNull();
  });

  it.each([
    'Mod+K',
    'Mod+,',
    'Mod+P',
    'Mod+Q',
    'Mod+A',
    'Mod+C',
    'Mod+V',
    'Mod+X',
    'Mod+Z',
    'Mod+Shift+Z',
  ])('rejects reserved %s', (s) => {
    expect(RESERVED_ACTION_SHORTCUTS.has(s)).toBe(true);
    expect(validateActionShortcut(s)).toMatch(/reserved/i);
  });

  it('allows Mod+Y for root file Quick Look', () => {
    expect(validateActionShortcut('Mod+Y')).toBeNull();
  });

  it('assertActionShortcut throws with the offending value', () => {
    expect(() => assertActionShortcut('⌘N', 'notes:add')).toThrow(/notes:add.*⌘N/);
    expect(() => assertActionShortcut('Mod+N', 'notes:add')).not.toThrow();
  });
});

describe('eventToActionChord', () => {
  it('maps Ctrl to Mod', () => {
    expect(eventToActionChord(ev({ key: 'n', code: 'KeyN', ctrlKey: true }))).toBe('Mod+N');
  });

  it('never treats the Super key as Mod', () => {
    expect(eventToActionChord(ev({ key: 'n', code: 'KeyN', metaKey: true }))).toBeNull();
    expect(
      eventToActionChord(ev({ key: 'n', code: 'KeyN', metaKey: true, ctrlKey: true })),
    ).toBeNull();
  });

  it('maps Ctrl+X / Ctrl+Shift+X to Mod chords', () => {
    expect(eventToActionChord(ev({ key: 'x', code: 'KeyX', ctrlKey: true }))).toBe('Mod+X');
    expect(eventToActionChord(ev({ key: 'X', code: 'KeyX', ctrlKey: true, shiftKey: true }))).toBe(
      'Mod+Shift+X',
    );
  });

  it('maps Enter and Alt+Enter', () => {
    expect(eventToActionChord(ev({ key: 'Enter', code: 'Enter' }))).toBe('Enter');
    expect(eventToActionChord(ev({ key: 'Enter', code: 'Enter', altKey: true }))).toBe('Alt+Enter');
    expect(eventToActionChord(ev({ key: 'Enter', code: 'Enter', shiftKey: true }))).toBeNull();
  });

  it('orders modifiers canonically', () => {
    expect(
      eventToActionChord(
        ev({ key: 'C', code: 'KeyC', ctrlKey: true, shiftKey: true, altKey: true }),
      ),
    ).toBe('Mod+Alt+Shift+C');
  });

  it('uses the physical key so Shift/Alt do not change it', () => {
    expect(eventToActionChord(ev({ key: '<', code: 'Comma', ctrlKey: true, shiftKey: true }))).toBe(
      'Mod+Shift+,',
    );
  });

  it('falls back to event.key when code is absent (synthetic iframe events)', () => {
    expect(eventToActionChord(ev({ key: 'c', ctrlKey: true, shiftKey: true }))).toBe('Mod+Shift+C');
  });

  it('returns null for a bare modifier press', () => {
    expect(
      eventToActionChord(ev({ key: 'Control', code: 'ControlLeft', ctrlKey: true })),
    ).toBeNull();
  });

  it('allows bare F-keys', () => {
    expect(eventToActionChord(ev({ key: 'F5', code: 'F5' }))).toBe('F5');
  });

  it('returns null for plain keys other than Enter/F-keys', () => {
    expect(eventToActionChord(ev({ key: 'n', code: 'KeyN' }))).toBeNull();
    expect(eventToActionChord(ev({ key: ' ', code: 'Space' }))).toBeNull();
  });
});

describe('eventToActionChord: keyboard layouts', () => {
  it('follows the layout character for letters (Dvorak Ctrl+C is still copy)', () => {
    // On Dvorak the key that types "c" is the physical KeyI.
    expect(eventToActionChord(ev({ key: 'c', code: 'KeyI', ctrlKey: true }))).toBe('Mod+C');
  });

  it('falls back to the physical key when the layout character is not ASCII', () => {
    // Cyrillic layout: the KeyN key types "т".
    expect(eventToActionChord(ev({ key: 'т', code: 'KeyN', ctrlKey: true }))).toBe('Mod+N');
  });

  it('never treats AltGr as an action chord', () => {
    const e = ev({ key: 'ć', code: 'KeyC', ctrlKey: true, altKey: true });
    Object.defineProperty(e, 'getModifierState', { value: (k: string) => k === 'AltGraph' });
    expect(eventToActionChord(e)).toBeNull();
  });
});

describe('resolveActionChord', () => {
  it('resolves Mod to the physical modifier the iframe can see', () => {
    expect(resolveActionChord('Mod+Shift+C')).toBe('Control+Shift+C');
    expect(resolveActionChord('Mod+Alt+N')).toBe('Control+Alt+N');
  });

  it('leaves bare F-keys alone', () => {
    expect(resolveActionChord('F5')).toBe('F5');
  });
});

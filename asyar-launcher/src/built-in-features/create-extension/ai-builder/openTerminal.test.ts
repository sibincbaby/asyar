import { describe, it, expect } from 'vitest';
import { buildTerminalCommand } from './openTerminal';

const DIR = '/home/me/AsyarExtensions/com.user.notion';
const CMD = 'pnpm exec asyar publish';

describe('buildTerminalCommand', () => {
  it('launches a terminal emulator via /bin/sh', () => {
    const r = buildTerminalCommand(DIR, CMD);
    expect(r.program).toBe('/bin/sh');
    expect(r.args[0]).toBe('-c');
    expect(r.args[1]).toContain('gnome-terminal');
    expect(r.args[1]).toContain('x-terminal-emulator');
    expect(r.args[1]).toContain(DIR);
    expect(r.args[1]).toContain(CMD);
  });

  it('escapes a dir containing quotes (no shell breakout)', () => {
    const r = buildTerminalCommand(`/x'"; rm -rf ~ #`, CMD);
    expect(r.args[1]).toContain(`'\\''`); // POSIX single-quote escape present
  });
});

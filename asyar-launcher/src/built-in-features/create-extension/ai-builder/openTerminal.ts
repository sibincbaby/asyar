import { t } from '../../../services/i18n';
import { Command } from '@tauri-apps/plugin-shell';
import { openPath } from '@tauri-apps/plugin-opener';
import { feedbackService } from '../../../services/feedback/feedbackService.svelte';

export type TerminalCommand = { program: string; args: string[] };

/** POSIX single-quote escape: wraps `s` in single-quotes, handling embedded single-quotes. */
function shq(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/**
 * Builds a terminal launch command. Only `/bin/sh` from the Tauri shell plugin
 * allowlist is used as `program`.
 *
 * @param dir  - Absolute path to cd into before running `command`.
 * @param command - Shell command to run in the new terminal window.
 */
export function buildTerminalCommand(dir: string, command: string): TerminalCommand {
  const inner = `cd ${shq(dir)} && ${command}; exec bash`;
  const q = shq(inner);
  const script = `gnome-terminal -- bash -c ${q} || x-terminal-emulator -e bash -c ${q} || xterm -e bash -c ${q}`;
  return { program: '/bin/sh', args: ['-c', script] };
}

/**
 * Opens a terminal window at `dir` and runs `command` in it.
 *
 * Falls back to opening the directory in the OS file manager when the shell
 * invocation fails, and surfaces a manual-action diagnostic so the user knows
 * what to run.
 */
export async function openTerminalAt(dir: string, command: string): Promise<void> {
  const tc = buildTerminalCommand(dir, command);
  try {
    await Command.create(tc.program, tc.args).execute();
  } catch {
    await openPath(dir).catch(() => {});
    await feedbackService.report({
      source: 'frontend',
      kind: 'manual',
      severity: 'warning',
      retryable: false,
      context: { message: t('features.create_extension.ai.terminal_failed', { command, dir }) },
    });
  }
}

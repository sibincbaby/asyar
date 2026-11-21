import { t } from '../../../services/i18n';
import { Command } from '@tauri-apps/plugin-shell';
import { openPath } from '@tauri-apps/plugin-opener';
import { feedbackService } from '../../../services/feedback/feedbackService.svelte';

export async function openInEditor(path: string): Promise<void> {
  try {
    const cmd = Command.create('code', ['.'], { cwd: path });
    await cmd.execute();
  } catch {
    try {
      await openPath(path);
    } catch {
      await feedbackService.report({
        source: 'frontend',
        kind: 'manual',
        severity: 'info',
        retryable: false,
        context: { message: t('features.create_extension.ai.editor_failed', { path }) },
      });
    }
  }
}

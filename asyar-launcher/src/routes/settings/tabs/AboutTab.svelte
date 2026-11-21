<script lang="ts">
  import { SettingsCard, SettingsRow, Button } from '../../../components';
  import { getVersion } from '@tauri-apps/api/app';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import logoUrl from '../../../resources/images/Square142x142Logo.png';
  import { t } from '../../../services/i18n';

  let appVersion = $state('');

  $effect(() => {
    getVersion()
      .then((v) => {
        appVersion = v;
      })
      .catch(() => {
        appVersion = '0.1.0';
      });
  });
</script>

<div class="about-tab">
  <div id="about-app" class="anchor-group">
    <SettingsCard>
      <div class="app-header">
        <img src={logoUrl} alt="Asyar" class="app-logo" />
        <div class="app-name">Asyar</div>
        <div class="app-version">Version {appVersion}</div>
      </div>
    </SettingsCard>
  </div>

  <div id="about-credits" class="anchor-group">
    <div class="section-header">{t('settings.about.section_credits')}</div>
    <SettingsCard>
      <SettingsRow label={t('settings.about.created_by')}>
        {#snippet children()}
          <span class="info-text">Khoshbin Ali</span>
        {/snippet}
      </SettingsRow>

      <SettingsRow label={t('settings.about.built_with')}>
        {#snippet children()}
          <span class="info-text">Tauri, Rust, Svelte, TypeScript</span>
        {/snippet}
      </SettingsRow>
    </SettingsCard>
  </div>

  <div id="about-links" class="anchor-group">
    <div class="section-header">{t('settings.about.section_links')}</div>
    <SettingsCard>
      <SettingsRow
        label={t('settings.about.project_links')}
        description={t('settings.about.project_links_description')}
      >
        {#snippet children()}
          <div class="links-row">
            <Button onclick={() => openUrl('https://github.com/Xoshbin/asyar-launcher')}>
              GitHub
            </Button>
            <Button onclick={() => openUrl('https://discord.gg/vvYRXrs7Xa')}>Discord</Button>
            <Button onclick={() => openUrl('https://github.com/Xoshbin/asyar/blob/main/LICENSE')}>
              {t('settings.about.license')}
            </Button>
          </div>
        {/snippet}
      </SettingsRow>
    </SettingsCard>
  </div>
</div>

<style>
  .about-tab {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .anchor-group {
    scroll-margin-top: var(--space-6);
  }

  .app-header {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--space-8) var(--space-6) var(--space-6);
    position: relative;
  }

  .app-header::after {
    content: '';
    position: absolute;
    left: var(--space-5);
    right: var(--space-5);
    bottom: 0;
    height: 1px;
    background: var(--separator);
  }

  .app-logo {
    width: 72px;
    height: 72px;
    border-radius: var(--radius-xl);
  }

  .app-name {
    margin-top: var(--space-3);
    font-size: var(--font-size-lg);
    font-weight: 600;
    font-family: var(--font-ui);
    color: var(--text-primary);
  }

  .app-version {
    margin-top: var(--space-1);
    font-size: var(--font-size-sm);
    font-family: var(--font-ui);
    color: var(--text-secondary);
    user-select: none;
  }

  .info-text {
    font-size: var(--font-size-sm);
    font-family: var(--font-ui);
    color: var(--text-primary);
  }

  .links-row {
    display: flex;
    gap: var(--space-2);
  }
</style>

import { test, expect } from '@playwright/test';

test('should quick-start PI on NIM and use it as a code-writing horse', async ({ page }) => {
  await page.goto('/');
  await page.locator('.eyeloss-controls__view-btn').click();

  const missionTabs = page.getByRole('tablist', { name: 'Mission sections' });
  await missionTabs.getByRole('button', { name: 'Mission', exact: true }).click();
  const mission = page.locator('.eyeloss-mission-panel');
  await mission.getByLabel('Title').fill('PI horse mission');
  await mission.getByLabel('Brief').fill('Use PI on NIM as code-writing horse.');
  await mission.getByLabel('Validation command').fill('npm run test:e2e');
  await mission.getByRole('button', { name: 'Save', exact: true }).click();
  await mission.getByRole('button', { name: 'Plan', exact: true }).click();

  await missionTabs.getByRole('button', { name: 'Debug', exact: true }).click();
  const debugTabs = page.getByRole('tablist', { name: 'Debug sections' });
  await debugTabs.getByRole('button', { name: 'Profiles', exact: true }).click();

  const profiles = page.locator('.eyeloss-agent-profiles-panel');
  await profiles.getByRole('button', { name: 'PI on NIM', exact: true }).click();
  await expect(profiles.getByLabel('Label')).toHaveValue('PI on NIM');
  await expect(profiles.getByLabel('Provider')).toHaveValue('pi_cli');
  await expect(profiles.getByLabel('Command')).toHaveValue('pi');
  await expect(profiles.getByLabel('Model')).toHaveValue('openai/meta/llama3-8b-instruct');
  await expect(profiles).toContainText('NVIDIA_API_KEY');
  await expect(profiles).toContainText('OPENAI_BASE_URL');
  await expect(profiles).toContainText('PI on NIM via OpenAI-compatible NIM endpoint.');
  await profiles.getByRole('button', { name: 'Save profile', exact: true }).click();

  await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
  const runs = page.locator('.eyeloss-mission-runs-panel');
  await runs.getByLabel('Code worker profile').selectOption('pi-on-nim');
  await runs.getByRole('button', { name: 'Run code worker', exact: true }).click();

  await expect(runs).toContainText('selected: pi');
  await expect(runs).toContainText('model: openai/meta/llama3-8b-instruct');
  await expect(runs).toContainText('viewer profile: pi-on-nim');
  await expect(runs).toContainText('profile: pi-on-nim');
  await expect(runs).toContainText('Code worker completed objective-1.');
});

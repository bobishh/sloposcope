import { test, expect } from '@playwright/test';

test('should quick-start opencode on NIM and carry planner notes into profile-backed mission planning', async ({ page }) => {
  await page.goto('/');
  await page.locator('.eyeloss-controls__view-btn').click();

  const missionTabs = page.getByRole('tablist', { name: 'Mission sections' });
  await missionTabs.getByRole('button', { name: 'Debug', exact: true }).click();
  const debugTabs = page.getByRole('tablist', { name: 'Debug sections' });
  await debugTabs.getByRole('button', { name: 'Profiles', exact: true }).click();

  const profiles = page.locator('.eyeloss-agent-profiles-panel');

  // Given opencode-on-NIM quick start
  await profiles.getByRole('button', { name: 'OpenCode on NIM', exact: true }).click();
  await expect(profiles.getByLabel('Label')).toHaveValue('OpenCode on NIM');
  await expect(profiles.getByLabel('Provider')).toHaveValue('opencode_nim');
  await expect(profiles.getByLabel('Command')).toHaveValue('opencode');
  await expect(profiles.getByLabel('Model')).toHaveValue('openai/gpt-oss-20b');
  await expect(profiles).toContainText('NVIDIA_API_KEY');
  await expect(profiles).toContainText('Paste key. Run horse.');

  await profiles.getByLabel('Env JSON').fill(`{
  "NVIDIA_API_KEY": "demo-nim-key"
}`);
  await profiles.getByRole('button', { name: 'Save profile', exact: true }).click();

  // When planner face gets viewer-first notes and prompt override
  await missionTabs.getByRole('button', { name: 'Mission', exact: true }).click();
  const mission = page.locator('.eyeloss-mission-panel');
  await mission.getByLabel('Title').fill('Viewer-first planner face');
  await mission.getByLabel('Brief').fill('Tight planner view. No sludge.');
  await mission.getByLabel('Validation command').fill('npm run test:e2e');
  await mission.getByRole('textbox', { name: 'Planner notes' }).fill('Viewer-first. Show terse objective cards.');
  await mission.getByRole('textbox', { name: 'Planner prompt override' }).fill('Plan terse. Prefer user-visible checkpoints.');
  await mission.getByLabel('Planner profile').selectOption('opencode-on-nim');
  await mission.getByRole('button', { name: 'Save', exact: true }).click();
  await mission.getByRole('button', { name: 'Plan', exact: true }).click();

  // Then mission + runs expose planner context without sludge
  await expect(mission).toContainText('planner: OpenCode on NIM');
  await expect(mission).toContainText('Viewer-first. Show terse objective cards.');
  await expect(mission).toContainText('Plan terse. Prefer user-visible checkpoints.');
  await expect(mission).toContainText('mode: profile');
  await expect(mission).toContainText('Mission planned with OpenCode on NIM.');

  await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
  const runs = page.locator('.eyeloss-mission-runs-panel');
  await expect(runs).toContainText('planner profile: opencode-on-nim');
  await expect(runs).toContainText('planner notes: Viewer-first. Show terse objective cards.');
  await expect(runs).toContainText('prompt override: Plan terse. Prefer user-visible checkpoints.');
  await expect(runs).toContainText('Planned 1 objective(s) with opencode_nim.');

  // Failure state visible
  await missionTabs.getByRole('button', { name: 'Mission', exact: true }).click();
  await mission.getByLabel('Planner profile').selectOption('');
  await mission.getByRole('button', { name: 'Plan', exact: true }).click();
  await expect(mission).toContainText('Select planner profile for planner notes or override prompt.');
});

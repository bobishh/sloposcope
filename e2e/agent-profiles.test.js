import { test, expect } from '@playwright/test';

test('should save a code-worker profile, select it, and run with that profile', async ({ page }) => {
  await page.goto('/');
  await page.locator('.eyeloss-controls__view-btn').click();

  const missionTabs = page.getByRole('tablist', { name: 'Mission sections' });
  await missionTabs.getByRole('button', { name: 'Mission', exact: true }).click();
  await page.getByLabel('Title').fill('Profile wiring mission');
  await page.getByLabel('Brief').fill('Use saved CLI profile for code worker.');
  await page.getByLabel('Validation command').fill('npm run test:e2e');
  await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Save', exact: true }).click();
  await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Plan', exact: true }).click();

  await missionTabs.getByRole('button', { name: 'Debug', exact: true }).click();
  const debugTabs = page.getByRole('tablist', { name: 'Debug sections' });
  await debugTabs.getByRole('button', { name: 'Profiles', exact: true }).click();

  const profilePanel = page.locator('.eyeloss-agent-profiles-panel');
  await expect(profilePanel.getByRole('button', { name: 'Save profile', exact: true })).toBeDisabled();

  await profilePanel.getByLabel('Label').fill('Gemini horse');
  await profilePanel.getByLabel('Provider').fill('gemini_cli');
  await profilePanel.getByLabel('Command').fill('gemini');
  await profilePanel.getByLabel('Model').fill('gemini-2.5-pro');
  await profilePanel.getByLabel('Args').fill('--sandbox\n--yolo');
  await profilePanel.getByLabel('Capabilities').fill('can_edit\ncan_run_tools');
  await profilePanel.getByRole('button', { name: 'Save profile', exact: true }).click();

  await expect(profilePanel).toContainText('Gemini horse');
  await expect(profilePanel).toContainText('gemini_cli');

  await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
  const runs = page.locator('.eyeloss-mission-runs-panel');
  await runs.getByLabel('Code worker profile').selectOption('gemini-horse');
  await runs.getByRole('button', { name: 'Run code worker', exact: true }).click();

  await expect(runs).toContainText('viewer profile: gemini-horse');
  await expect(runs).toContainText('profile: gemini-horse');
  await expect(runs).toContainText('Code worker completed objective-1.');
});

test('should load a built-in horse template into the profile form', async ({ page }) => {
  await page.goto('/');
  await page.locator('.eyeloss-controls__view-btn').click();

  const missionTabs = page.getByRole('tablist', { name: 'Mission sections' });
  await missionTabs.getByRole('button', { name: 'Debug', exact: true }).click();
  const debugTabs = page.getByRole('tablist', { name: 'Debug sections' });
  await debugTabs.getByRole('button', { name: 'Profiles', exact: true }).click();

  const profilePanel = page.locator('.eyeloss-agent-profiles-panel');
  await profilePanel.getByRole('button', { name: 'Gemini', exact: true }).click();

  await expect(profilePanel.getByLabel('Label')).toHaveValue('Gemini horse');
  await expect(profilePanel.getByLabel('Provider')).toHaveValue('gemini_cli');
  await expect(profilePanel.getByLabel('Command')).toHaveValue('gemini');
  await expect(profilePanel.getByLabel('Model')).toHaveValue('gemini-2.5-pro');
  await expect(profilePanel.getByLabel('Transport')).toHaveValue('pty');
  await expect(profilePanel.getByLabel('Capabilities')).toHaveValue(/can_edit/);
  await expect(profilePanel.getByRole('button', { name: 'Save profile', exact: true })).toBeEnabled();
});

test('should load PI on NIM template into the profile form', async ({ page }) => {
  await page.goto('/');
  await page.locator('.eyeloss-controls__view-btn').click();

  const missionTabs = page.getByRole('tablist', { name: 'Mission sections' });
  await missionTabs.getByRole('button', { name: 'Debug', exact: true }).click();
  const debugTabs = page.getByRole('tablist', { name: 'Debug sections' });
  await debugTabs.getByRole('button', { name: 'Profiles', exact: true }).click();

  const profilePanel = page.locator('.eyeloss-agent-profiles-panel');
  await profilePanel.getByRole('button', { name: 'PI on NIM', exact: true }).click();

  await expect(profilePanel.getByLabel('Label')).toHaveValue('PI on NIM');
  await expect(profilePanel.getByLabel('Provider')).toHaveValue('pi_cli');
  await expect(profilePanel.getByLabel('Command')).toHaveValue('pi');
  await expect(profilePanel.getByLabel('Model')).toHaveValue('openai/meta/llama3-8b-instruct');
  await expect(profilePanel.getByLabel('Transport')).toHaveValue('pty');
  await expect(profilePanel).toContainText('NVIDIA_API_KEY');
  await expect(profilePanel).toContainText('OPENAI_BASE_URL');
  await expect(profilePanel).toContainText('PI on NIM via OpenAI-compatible NIM endpoint.');
  await expect(profilePanel.getByRole('button', { name: 'Save profile', exact: true })).toBeEnabled();
});

test('should show env token hints and keep env json visible after profile reload', async ({ page }) => {
  await page.goto('/');
  await page.locator('.eyeloss-controls__view-btn').click();

  const missionTabs = page.getByRole('tablist', { name: 'Mission sections' });
  await missionTabs.getByRole('button', { name: 'Debug', exact: true }).click();
  const debugTabs = page.getByRole('tablist', { name: 'Debug sections' });
  await debugTabs.getByRole('button', { name: 'Profiles', exact: true }).click();

  const profilePanel = page.locator('.eyeloss-agent-profiles-panel');

  // Given a Gemini template
  await profilePanel.getByRole('button', { name: 'Gemini', exact: true }).click();
  await expect(profilePanel).toContainText('GEMINI_API_KEY');
  await expect(profilePanel).toContainText('GOOGLE_API_KEY');

  // When env overrides are saved and profiles reload from mock backend
  await profilePanel.getByLabel('Env JSON').fill(`{
  "GEMINI_API_KEY": "demo-token",
  "GOOGLE_CLOUD_PROJECT": "sloposcope-test"
}`);
  await profilePanel.getByRole('button', { name: 'Save profile', exact: true }).click();
  await profilePanel.getByRole('button', { name: 'Reload', exact: true }).click();

  // Then saved profile still exposes env data without losing compact view
  const savedGemini = profilePanel.locator('.eyeloss-mission__item').filter({ hasText: 'Gemini horse' }).first();
  await expect(savedGemini).toContainText('env overrides: GEMINI_API_KEY, GOOGLE_CLOUD_PROJECT');
  await savedGemini.locator('summary').click();
  await expect(savedGemini).toContainText('"GOOGLE_CLOUD_PROJECT": "sloposcope-test"');
});

test('should load NIM template and use active NIM planner on mission plan', async ({ page }) => {
  await page.goto('/');
  await page.locator('.eyeloss-controls__view-btn').click();

  const missionTabs = page.getByRole('tablist', { name: 'Mission sections' });
  await missionTabs.getByRole('button', { name: 'Debug', exact: true }).click();
  const debugTabs = page.getByRole('tablist', { name: 'Debug sections' });
  await debugTabs.getByRole('button', { name: 'Profiles', exact: true }).click();

  const profilePanel = page.locator('.eyeloss-agent-profiles-panel');
  await profilePanel.getByRole('button', { name: 'NIM', exact: true }).click();
  await expect(profilePanel.getByLabel('Provider')).toHaveValue('nim_http');
  await expect(profilePanel.getByLabel('Model')).toHaveValue('meta/llama3-8b-instruct');
  await expect(profilePanel.getByLabel('Transport')).toHaveValue('http');
  await expect(profilePanel).toContainText('NVIDIA_API_KEY');
  await profilePanel.getByRole('button', { name: 'Save profile', exact: true }).click();

  await missionTabs.getByRole('button', { name: 'Mission', exact: true }).click();
  await page.getByLabel('Title').fill('NIM planner mission');
  await page.getByLabel('Brief').fill('[scope:src/] Plan with NIM\n[scope:src-tauri/] Review with NIM');
  await page.getByLabel('Validation command').fill('npm run test:e2e');
  await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Save', exact: true }).click();
  await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Plan', exact: true }).click();

  await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
  const runs = page.locator('.eyeloss-mission-runs-panel');
  await expect(runs).toContainText('Planned 2 objective(s) with nim_http.');
  await expect(runs).toContainText('objective-1');
  await expect(runs).toContainText('objective-2');
});

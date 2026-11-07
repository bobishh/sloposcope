import { test, expect } from '@playwright/test';

async function openAdvancedOperations(page) {
  await page.locator('.eyeloss-controls__view-btn').click();
  await page.getByLabel('Developer mode').check();
  return page.getByRole('tablist', { name: 'Advanced operation sections' });
}

test.describe('CodeGraph E2E', () => {
  test('should load the graph and display mock data', async ({ page }) => {
    await page.goto('/');

    // Verify mock data is loaded
    const repoPath = page.locator('.eyeloss-controls__repo');
    await expect(repoPath).toHaveText('/path/to/despair');

    // Wait for spinner to disappear
    await expect(page.locator('.eyeloss-spinner')).not.toBeVisible();

    // Check if the canvas is there
    const canvas = page.locator('canvas');
    await expect(canvas).toBeVisible();

    // Check the nav panel for the mock node (now shows filenames)
    const navPanel = page.locator('.eyeloss-nav-panel');
    await expect(navPanel).toContainText('MockModule');
  });

  test('Given the repository map, When zoom changes, Then shared anatomy grammar and static edges stay stable', async ({ page }) => {
    await page.goto('/');
    const canvas = page.locator('.eyeloss-graph-canvas');
    await expect(canvas).toHaveAttribute('data-phenotype-grammar', 'shared-callable-metrics');
    const callableCount = await canvas.getAttribute('data-callable-count');
    const staticLinkCount = await canvas.getAttribute('data-static-link-count');
    await expect(canvas).toHaveAttribute('data-detail-level', 'anatomy');
    for (let index = 0; index < 50; index += 1) await page.keyboard.press('Control+-');
    await expect(canvas).toHaveAttribute('data-detail-level', 'repository');
    await expect(canvas).toHaveAttribute('data-phenotype-grammar', 'shared-callable-metrics');
    await expect(canvas).toHaveAttribute('data-callable-count', callableCount);
    await expect(canvas).toHaveAttribute('data-static-link-count', staticLinkCount);
  });

  test('should allow changing the repository path', async ({ page }) => {
    await page.goto('/');

    // Click "Change" button (the mock returns /new/path/to/despair)
    const changeBtn = page.getByRole('button', { name: 'Change', exact: true });
    await changeBtn.click();

    // Verify path changed
    const repoPath = page.locator('.eyeloss-controls__repo');
    await expect(repoPath).toHaveText('/new/path/to/despair');
  });

  test('should toggle the nav panel', async ({ page }) => {
    await page.goto('/');
    
    const toggleBtn = page.locator('.eyeloss-nav-panel__toggle').first();
    await expect(page.locator('.eyeloss-nav-panel__search')).toBeVisible();
    
    await toggleBtn.click();
    await expect(page.locator('.eyeloss-nav-panel__search')).not.toBeVisible();
  });

  test('should display and interact with the timeline', async ({ page }) => {
    await page.goto('/');

    // Wait for data
    await expect(page.locator('.eyeloss-timeline')).toBeVisible();

    // Timeline window is dynamic; ensure we have visible points.
    const ticks = page.locator('.eyeloss-timeline__tick');
    expect(await ticks.count()).toBeGreaterThan(0);

    const targetTick = page.locator('.eyeloss-timeline__tick', {
      has: page.locator('.eyeloss-timeline__tick-id', { hasText: 'c0ffee11' }),
    });
    await expect(targetTick).toHaveCount(1);

    // Hover over a tick to update the fixed timeline description rail
    await targetTick.hover();
    const descRail = page.locator('.eyeloss-timeline__description-text');
    await expect(descRail).toBeVisible();
    await expect(descRail).toContainText('Refactor parser pipeline');

    const tickId = targetTick.locator('.eyeloss-timeline__tick-id');
    await expect(tickId).toHaveText('c0ffee11');

    // Click a tick to select it
    await targetTick.click();
    
    // Verify input changed
    const sinceInput = page.locator('#since-input');
    await expect(sinceInput).toHaveValue('c0ffee11');
  });

  test('should support multi-select with Shift key', async ({ page }) => {
    await page.goto('/');

    const firstTick = page.locator('.eyeloss-timeline__tick', {
      has: page.locator('.eyeloss-timeline__tick-id', { hasText: 'deadbeef' }),
    });
    const secondTick = page.locator('.eyeloss-timeline__tick', {
      has: page.locator('.eyeloss-timeline__tick-id', { hasText: 'c0ffee11' }),
    });
    await expect(firstTick).toHaveCount(1);
    await expect(secondTick).toHaveCount(1);

    // Select one commit, then shift-add another
    await firstTick.click();
    await page.keyboard.down('Shift');
    await secondTick.click();
    await page.keyboard.up('Shift');

    // Verify aggregate revset in input
    const sinceInput = page.locator('#since-input');
    await expect(sinceInput).toHaveValue('deadbeef | c0ffee11');
  });

  test('should switch revision repeatedly via timeline clicks', async ({ page }) => {
    await page.goto('/');

    const deadbeefTick = page.locator('.eyeloss-timeline__tick', {
      has: page.locator('.eyeloss-timeline__tick-id', { hasText: 'deadbeef' }),
    });
    const coffeeTick = page.locator('.eyeloss-timeline__tick', {
      has: page.locator('.eyeloss-timeline__tick-id', { hasText: 'c0ffee11' }),
    });
    const sinceInput = page.locator('#since-input');

    await expect(deadbeefTick).toHaveCount(1);
    await expect(coffeeTick).toHaveCount(1);

    await deadbeefTick.click();
    await expect(sinceInput).toHaveValue('deadbeef');

    await coffeeTick.click();
    await expect(sinceInput).toHaveValue('c0ffee11');

    await deadbeefTick.click();
    await expect(sinceInput).toHaveValue('deadbeef');
  });

  test('should open multiple windows and tile them with hotkey', async ({ page }) => {
    await page.goto('/');

    const moduleItem = page.locator('.eyeloss-nav-panel__item', { hasText: 'MockModule' });
    const helperItem = page.locator('.eyeloss-nav-panel__item', { hasText: 'MockHelper' });
    await moduleItem.click();
    await helperItem.click();

    const windows = page.locator('.window');
    await expect(windows).toHaveCount(4);

    const firstWindow = windows.first();
    const beforeStyle = await firstWindow.getAttribute('style');
    expect(beforeStyle || '').not.toContain('left: 16px;');

    await page.keyboard.down(process.platform === 'darwin' ? 'Meta' : 'Control');
    await page.keyboard.press('t');
    await page.keyboard.up(process.platform === 'darwin' ? 'Meta' : 'Control');

    const afterStyle = await firstWindow.getAttribute('style');
    expect(afterStyle || '').toContain('left: 16px;');
  });

  test('should drive the mission flow across tabs', async ({ page }) => {
    await page.goto('/');

    const missionTabs = await openAdvancedOperations(page);

    await expect(missionTabs.getByRole('button', { name: 'Mission', exact: true })).toBeVisible();
    await expect(missionTabs.getByRole('button', { name: 'Runs', exact: true })).toBeVisible();
    await expect(missionTabs.getByRole('button', { name: 'Debug', exact: true })).toBeVisible();

    await expect(page.getByText('Mission Brief')).toBeVisible();
    await page.getByLabel('Title').fill('M8A mission flow');
    await page.getByLabel('Brief').fill('Normalize mission state and keep form state across tabs.');
    await page.getByLabel('Validation command').fill('npm run test:e2e');
    await page.getByLabel('Issue key').fill('M8A-1');
    await page.getByLabel('Issue URL').fill('https://linear.app/example/M8A-1');

    await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Save', exact: true }).click();
    await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Plan', exact: true }).click();

    await expect(page.locator('.eyeloss-mission-panel')).toContainText('M8A mission flow');
    await expect(page.locator('.eyeloss-mission-panel')).toContainText('Normalize mission state and keep form state across tabs.');

    await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Mission Runs' })).toBeVisible();
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('objective-1');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('M8A mission flow');
    await page.locator('.eyeloss-mission-runs-panel').getByRole('button', { name: 'Run code worker', exact: true }).click();

    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('code-session-1');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('completed');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('verification-1');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('passed');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('Patch artifact');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('.sloposcope/worktrees/');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('Validation: npm run test:e2e');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('Code worker completed objective-1.');

    await missionTabs.getByRole('button', { name: 'Debug', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Enterprise Controls' })).toBeVisible();
    await expect(page.locator('.eyeloss-settings-tabs--debug')).toContainText('Issue');
    await expect(page.locator('.eyeloss-settings-tabs--debug')).toContainText('Config');
    await expect(page.locator('.eyeloss-settings-tabs--debug')).toContainText('Runs');

    await page.reload();
    await expect(page.locator('.eyeloss-spinner')).not.toBeVisible();
    const reloadedMissionTabs = await openAdvancedOperations(page);
    await expect(page.getByRole('heading', { name: 'Mission Brief' })).toBeVisible();
    await expect(page.locator('.eyeloss-mission-panel')).toContainText('M8A mission flow');
    await expect(page.locator('.eyeloss-mission-panel')).toContainText('Normalize mission state and keep form state across tabs.');
    await reloadedMissionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('code-session-1');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('verification-1');

    await missionTabs.getByRole('button', { name: 'Mission', exact: true }).click();
    await expect(page.getByLabel('Title')).toHaveValue('M8A mission flow');
    await expect(page.getByLabel('Brief')).toHaveValue('Normalize mission state and keep form state across tabs.');
    await expect(page.getByLabel('Validation command')).toHaveValue('npm run test:e2e');
    await expect(page.getByLabel('Issue key')).toHaveValue('M8A-1');
    await expect(page.getByLabel('Issue URL')).toHaveValue('https://linear.app/example/M8A-1');
  });

  test('should gate mission runs with viewer-first review actions', async ({ page }) => {
    await page.goto('/');

    const missionTabs = await openAdvancedOperations(page);

    await page.getByLabel('Title').fill('M9 viewer-first gating');
    await page.getByLabel('Brief').fill('Keep the viewer on the active session and mirror review state.');
    await page.getByLabel('Validation command').fill('npm run test:e2e');
    await page.getByLabel('Issue key').fill('M9-1');
    await page.getByLabel('Issue URL').fill('https://linear.app/example/M9-1');

    await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Save', exact: true }).click();
    await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Plan', exact: true }).click();

    await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();

    const panel = page.locator('.eyeloss-mission-runs-panel');
    await page.locator('.eyeloss-mission-runs-panel').getByRole('button', { name: 'Run code worker', exact: true }).click();

    await expect(panel).toContainText('viewer profile: local-code-worker');
    await expect(panel).toContainText('session: code-session-1');
    await expect(panel).toContainText('review: pending');
    await expect(panel).toContainText('gate: open');
    await expect(panel.getByRole('button', { name: 'Apply', exact: true })).toBeDisabled();

    await panel.getByRole('button', { name: 'Accept', exact: true }).click();
    await expect(panel).toContainText('review: accepted');
    await expect(panel).toContainText('gate: ready');
    await expect(panel.getByRole('button', { name: 'Apply', exact: true })).toBeEnabled();

    await panel.getByRole('button', { name: 'Gate', exact: true }).click();
    await expect(panel).toContainText('gate: ready');

    await panel.getByRole('button', { name: 'Apply', exact: true }).click();
    await expect(panel).toContainText('apply: applied');

    await page.reload();
    await expect(page.locator('.eyeloss-spinner')).not.toBeVisible();
    const reloadedMissionTabs = await openAdvancedOperations(page);
    await reloadedMissionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('viewer profile: local-code-worker');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('review: accepted');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('apply: applied');
    await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('gate: ready');
  });

  test('should plan scoped multi-objective missions and expose scheduler controls', async ({ page }) => {
    await page.goto('/');

    const missionTabs = await openAdvancedOperations(page);
    await missionTabs.getByRole('button', { name: 'Mission', exact: true }).click();

    await page.getByLabel('Title').fill('M10 scheduler mission');
    await page.getByLabel('Brief').fill([
      '[scope:src/] UI shell cleanup',
      '[scope:src/] UI second pass',
      '[scope:src-tauri/] Rust runtime sweep',
    ].join('\n'));
    await page.getByLabel('Validation command').fill('npm run test:e2e');
    await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Save', exact: true }).click();
    await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Plan', exact: true }).click();

    await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
    const runs = page.locator('.eyeloss-mission-runs-panel');
    await expect(runs).toContainText('scheduler');
    await expect(runs.locator('.eyeloss-mission__item-id')).toHaveCount(3);
    await expect(runs.getByRole('button', { name: 'Run next code worker', exact: true })).toBeVisible();
    await expect(runs.getByRole('button', { name: 'Sweep stale', exact: true })).toBeVisible();
  });

  test('should save and use an active agent profile from debug', async ({ page }) => {
    await page.goto('/');
    const missionTabs = await openAdvancedOperations(page);

    await page.getByLabel('Title').fill('M9 profile flow');
    await page.getByLabel('Brief').fill('Run code worker through saved active profile.');
    await page.getByLabel('Validation command').fill('npm run test:e2e');
    await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Save', exact: true }).click();
    await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Plan', exact: true }).click();

    await missionTabs.getByRole('button', { name: 'Debug', exact: true }).click();
    const debugTabs = page.getByRole('tablist', { name: 'Debug sections' });
    await debugTabs.getByRole('button', { name: 'Profiles', exact: true }).click();

    const profilePanel = page.locator('.eyeloss-agent-profiles-panel');
    await expect(profilePanel.getByRole('button', { name: 'Save profile', exact: true })).toBeDisabled();

    await profilePanel.getByLabel('Label').fill('Gemini Horse');
    await profilePanel.getByLabel('Provider').fill('gemini_cli');
    await profilePanel.getByLabel('Command').fill('/bin/sh');
    await profilePanel.getByLabel('Args').fill('-lc\nprintf "export const answer = 2;\\n" > app.js');
    await profilePanel.getByLabel('Capabilities').fill('can_edit, can_run_tools');
    await profilePanel.getByLabel('Transport').selectOption('pty');
    await profilePanel.getByLabel('Active profile').check();
    await profilePanel.getByRole('button', { name: 'Save profile', exact: true }).click();

    await expect(profilePanel).toContainText('Gemini Horse');
    await expect(profilePanel).toContainText('gemini_cli');

    await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
    const runs = page.locator('.eyeloss-mission-runs-panel');
    await runs.getByRole('button', { name: 'Run code worker', exact: true }).click();
    await expect(runs).toContainText('viewer profile: gemini-horse');
    await expect(runs).toContainText('profile: gemini-horse');
  });
});

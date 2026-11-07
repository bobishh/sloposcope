import { test, expect } from '@playwright/test';

async function openAdvancedOperations(page) {
  await page.locator('.eyeloss-controls__view-btn').click();
  await page.getByLabel('Developer mode').check();
  return page.getByRole('tablist', { name: 'Advanced operation sections' });
}

test('worker apply projects touched file onto live graph without reload', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.eyeloss-spinner')).not.toBeVisible();
  await expect(page.locator('.eyeloss-nav-panel')).toContainText('MockModule');
  await expect(page.locator('.eyeloss-nav-panel__toggle').first()).toContainText('Nodes (2)');

  const reloadToken = await page.evaluate(() => {
    window.__liveMapProjectionReloadToken = crypto.randomUUID();
    return window.__liveMapProjectionReloadToken;
  });

  const coordinator = page.locator('.eyeloss-coordinator-window');

  // Given a coordinator-planned worker task.
  await coordinator.getByLabel('Message coordinator').fill('Create a worker file placeholder on the live map.');
  await coordinator.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(coordinator).toContainText('Plan proposal');
  await coordinator.getByRole('button', { name: 'Approve plan', exact: true }).click();
  await expect(coordinator).toContainText('Ready to launch workers');

  // When worker output is launched, accepted, and applied.
  await coordinator.getByRole('button', { name: 'Launch workers', exact: true }).click();
  await expect(coordinator).toContainText('Workers launched');

  const missionTabs = await openAdvancedOperations(page);
  await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
  const runs = page.locator('.eyeloss-mission-runs-panel');
  await expect(runs).toContainText('code-session-1');
  await runs.getByRole('button', { name: 'Accept', exact: true }).click();
  await expect(runs).toContainText('gate: ready');
  await runs.getByRole('button', { name: 'Apply', exact: true }).click();
  await expect(runs).toContainText('apply: applied');

  // Then graph mutates in place with pending rebuild context.
  await page.getByRole('button', { name: 'Back to graph', exact: true }).click();
  await expect(page.locator('.eyeloss-nav-panel__toggle').first()).toContainText('Nodes (3)');
  const projectedNode = page.locator('.eyeloss-nav-panel__item', { hasText: 'mock_worker.js' });
  await expect(projectedNode).toContainText('pending graph rebuild');
  await expect(projectedNode).toContainText('code-session-1');
  await expect(projectedNode).toHaveAttribute('data-node-id', 'src/mock_worker.js');
  await expect(projectedNode).toHaveAttribute('data-worker-session-id', 'code-session-1');
  await expect(projectedNode).toHaveAttribute('data-verification-state', 'passed');
  await expect(projectedNode).toContainText('worker code-session-1');
  await expect(projectedNode).toContainText('verification passed');
  await expect(page.locator('g[data-robot-state="applied"]')).toBeVisible();

  const missionLifecycleTypes = await page.evaluate(() => {
    const states = JSON.parse(window.localStorage.getItem('sloposcope.mockMissionStateByRepo') || '{}');
    return Object.values(states).flatMap((state) => state.lifecycleEvents || []).map((event) => event.type);
  });
  expect(missionLifecycleTypes).toEqual(expect.arrayContaining(['review.accepted', 'patch.applied']));

  await expect.poll(() => page.evaluate(() => window.__liveMapProjectionReloadToken)).toBe(reloadToken);
});

test('worker verification failure projects failed state onto affected node', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.eyeloss-spinner')).not.toBeVisible();

  const coordinator = page.locator('.eyeloss-coordinator-window');
  await coordinator.getByLabel('Message coordinator').fill('Create a failing worker file placeholder. Validation: npm run fail');
  await coordinator.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(coordinator).toContainText('Plan proposal');
  await coordinator.getByRole('button', { name: 'Approve plan', exact: true }).click();
  await expect(coordinator).toContainText('Ready to launch workers');
  await coordinator.getByRole('button', { name: 'Launch workers', exact: true }).click();
  await expect(coordinator).toContainText('Workers launched');

  const missionTabs = await openAdvancedOperations(page);
  await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
  const runs = page.locator('.eyeloss-mission-runs-panel');
  await expect(runs).toContainText('failed');
  await runs.getByRole('button', { name: 'Accept', exact: true }).click();
  await runs.getByRole('button', { name: 'Apply', exact: true }).click();

  await page.getByRole('button', { name: 'Back to graph', exact: true }).click();
  const projectedNode = page.locator('[data-node-id="src/mock_worker.js"]');
  await expect(projectedNode).toHaveAttribute('data-worker-session-id', 'code-session-1');
  await expect(projectedNode).toHaveAttribute('data-verification-state', 'failed');
  await expect(projectedNode).toContainText('verification failed');
});

test('worker apply does not mutate historical graph view', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.eyeloss-spinner')).not.toBeVisible();
  await expect(page.locator('.eyeloss-nav-panel__toggle').first()).toContainText('Nodes (2)');

  const reloadToken = await page.evaluate(() => {
    window.__historicalProjectionReloadToken = crypto.randomUUID();
    return window.__historicalProjectionReloadToken;
  });

  await page.locator('#since-input').fill('b0ba0000');
  await page.locator('#since-input').press('Enter');
  await expect(page.locator('.eyeloss-controls__hint')).toContainText('approx historical view');
  await expect(page.locator('.eyeloss-nav-panel__toggle').first()).toContainText('Nodes (2)');

  const coordinator = page.locator('.eyeloss-coordinator-window');
  await coordinator.getByLabel('Message coordinator').fill('Create a worker file placeholder while historical graph is selected.');
  await coordinator.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(coordinator).toContainText('Plan proposal');
  await coordinator.getByRole('button', { name: 'Approve plan', exact: true }).click();
  await expect(coordinator).toContainText('Ready to launch workers');
  await coordinator.getByRole('button', { name: 'Launch workers', exact: true }).click();
  await expect(coordinator).toContainText('Workers launched');

  const missionTabs = await openAdvancedOperations(page);
  await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
  const runs = page.locator('.eyeloss-mission-runs-panel');
  await runs.getByRole('button', { name: 'Accept', exact: true }).click();
  await runs.getByRole('button', { name: 'Apply', exact: true }).click();

  await page.getByRole('button', { name: 'Back to graph', exact: true }).click();
  await expect(page.locator('.eyeloss-controls__hint')).toContainText('approx historical view');
  await expect(page.locator('.eyeloss-nav-panel__toggle').first()).toContainText('Nodes (2)');
  await expect(page.locator('[data-node-id="src/mock_worker.js"]')).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__historicalProjectionReloadToken)).toBe(reloadToken);
});

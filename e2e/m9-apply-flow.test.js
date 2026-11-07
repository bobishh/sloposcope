import { test, expect } from '@playwright/test';

async function openAdvancedOperations(page) {
  await page.locator('.eyeloss-controls__view-btn').click();
  await page.getByLabel('Developer mode').check();
  return page.getByRole('tablist', { name: 'Advanced operation sections' });
}

test('should gate and apply a worker patch from mission runs', async ({ page }) => {
  await page.goto('/');

  const missionTabs = await openAdvancedOperations(page);
  await missionTabs.getByRole('button', { name: 'Mission', exact: true }).click();
  await page.getByLabel('Title').fill('M9 apply flow');
  await page.getByLabel('Brief').fill('Apply code-worker patch into active map with gate.');
  await page.getByLabel('Validation command').fill('npm run test:e2e');
  await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Save', exact: true }).click();
  await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Plan', exact: true }).click();

  await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
  const runs = page.locator('.eyeloss-mission-runs-panel');
  await runs.getByRole('button', { name: 'Run code worker', exact: true }).click();
  await page.getByRole('button', { name: 'Back to graph', exact: true }).click();
  await expect(page.locator('g[data-robot-state="completed"]')).toBeVisible();
  await page.locator('.eyeloss-controls__view-btn').click();

  await expect(runs.getByRole('button', { name: 'Accept', exact: true })).toBeVisible();
  await runs.getByRole('button', { name: 'Accept', exact: true }).click();
  await expect(runs).toContainText('gate: ready');
  await page.getByRole('button', { name: 'Back to graph', exact: true }).click();
  await expect(page.locator('g[data-robot-state="gated"]')).toBeVisible();
  await page.locator('.eyeloss-controls__view-btn').click();
  await runs.getByRole('button', { name: 'Apply', exact: true }).click();
  await expect(runs).toContainText('applied');
  await page.getByRole('button', { name: 'Back to graph', exact: true }).click();
  await expect(page.locator('g[data-robot-state="applied"]')).toBeVisible();
});

test('should plan scheduler objectives, run next schedulable worker, and show stale sweep no-op', async ({ page }) => {
  await page.goto('/');

  const missionTabs = await openAdvancedOperations(page);

  // Given a multiline brief with scope tags
  await missionTabs.getByRole('button', { name: 'Mission', exact: true }).click();
  await page.getByLabel('Title').fill('M10A scheduler flow');
  await page.getByLabel('Brief').fill(`[scope:src/ui]
Map run summary into runs panel.
[scope:src/ui]
Show blocked card when scope clashes.
[scope:src/mock]
Expose stale sweep noop.`);
  await page.getByLabel('Validation command').fill('npm run test:e2e');
  await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Save', exact: true }).click();
  await page.locator('.eyeloss-mission-panel').getByRole('button', { name: 'Plan', exact: true }).click();

  // When planner expands brief into scheduler objectives
  await missionTabs.getByRole('button', { name: 'Runs', exact: true }).click();
  const runs = page.locator('.eyeloss-mission-runs-panel');
  await expect(runs).toContainText('objective-1');
  await expect(runs).toContainText('objective-2');
  await expect(runs).toContainText('objective-3');
  await expect(runs).toContainText('ready: 2');
  await expect(runs).toContainText('blocked: 1');
  await expect(runs).toContainText('active: 0');
  await expect(runs).toContainText('stale: 0');
  await expect(runs).toContainText('next objective: objective-1');
  await expect(runs).toContainText('scheduler: blocked');
  await expect(runs).toContainText('reason: waiting for scope src/ui');

  // Then run-next advances first schedulable objective
  await runs.getByRole('button', { name: 'Run next code worker', exact: true }).click();
  await expect(runs).toContainText('objective: objective-1');
  await expect(runs).toContainText('Code worker completed objective-1.');
  await expect(runs).toContainText('next objective: objective-2');

  // And stale sweep still shows visible no-op when nothing stale exists
  await runs.getByRole('button', { name: 'Sweep stale', exact: true }).click();
  await expect(runs).toContainText('No stale sessions to sweep.');
});

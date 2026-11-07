import { test, expect } from '@playwright/test';

test('should wake coordinator, plan a mission, launch workers, and report status', async ({ page }) => {
  await page.goto('/');

  const coordinator = page.locator('.eyeloss-coordinator-window');
  await expect(coordinator).toBeVisible();
  await expect(coordinator).toContainText('Coordinator');
  await expect(coordinator).toContainText('What problem');

  const input = coordinator.getByLabel('Message coordinator');
  await input.fill('Settings surface overloaded. Need coordinator-first shell.');
  await coordinator.getByRole('button', { name: 'Send', exact: true }).click();

  await expect(coordinator).toContainText('Plan proposal');
  await expect(coordinator).not.toContainText('Need validation command before plan');
  await expect(coordinator).not.toContainText('Example: npm run test:e2e');
  await expect(coordinator.getByRole('button', { name: 'Approve plan', exact: true })).toBeVisible();

  await coordinator.getByRole('button', { name: 'Approve plan', exact: true }).click();
  await expect(coordinator).toContainText('Ready to launch workers');

  await coordinator.getByRole('button', { name: 'Launch workers', exact: true }).click();
  await expect(coordinator).toContainText('Workers launched');

  await page.locator('.eyeloss-controls__view-btn').click();
  await page.getByLabel('Developer mode').check();
  await page.getByRole('tablist', { name: 'Advanced operation sections' }).getByRole('button', { name: 'Runs', exact: true }).click();
  await expect(page.locator('.eyeloss-mission-runs-panel')).toContainText('code-session-1');

  await page.getByRole('button', { name: 'Back to graph', exact: true }).click();
  const completedRobots = page.locator('g[data-robot-state="completed"]');
  expect(await completedRobots.count()).toBeGreaterThan(0);
  await expect(completedRobots.first()).toBeVisible();

  await coordinator.getByRole('button', { name: 'Ask report', exact: true }).click();
  await expect(coordinator).toContainText('Mission report');
  await expect(coordinator.getByRole('button', { name: 'Nudge latest worker', exact: true })).toBeHidden();
});

import { test, expect } from '@playwright/test';

test('should use coordinator as front door, switch threads, and nudge a live horse', async ({ page }) => {
  await page.goto('/');

  const coordinator = page.locator('.eyeloss-coordinator-window');
  await expect(coordinator).toBeVisible();
  await expect(page.locator('.eyeloss-controls__view-btn')).toHaveText('Settings');

  await page.locator('.eyeloss-controls__view-btn').click();
  await page.getByLabel('Developer mode').check();
  await page.getByRole('tablist', { name: 'Advanced operation sections' }).getByRole('button', { name: 'Debug', exact: true }).click();
  await page.getByRole('tablist', { name: 'Debug sections' }).getByRole('button', { name: 'Profiles', exact: true }).click();

  const profiles = page.locator('.eyeloss-agent-profiles-panel');
  await profiles.getByRole('button', { name: 'Live Local', exact: true }).click();
  await profiles.getByRole('button', { name: 'Save profile', exact: true }).click();
  await profiles.getByLabel('Selected profile').selectOption('live-local-horse');

  await page.getByRole('button', { name: 'Back to graph', exact: true }).click();

  const input = coordinator.getByLabel('Message coordinator');
  await input.fill('Search panel chaotic. Need coordinator-first shell and cleaner machine room.');
  await coordinator.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(coordinator).toContainText('Plan proposal');
  await expect(coordinator).not.toContainText('Need validation command before plan');

  await coordinator.getByRole('button', { name: 'Approve plan', exact: true }).click();
  await coordinator.getByRole('button', { name: 'Launch workers', exact: true }).click();

  await coordinator.getByRole('button', { name: 'Ask report', exact: true }).click();
  await expect(coordinator).toContainText('Live sessions: 1');

  await coordinator.getByRole('button', { name: 'Nudge latest worker', exact: true }).click();
  await expect(coordinator).toContainText('Nudge sent to');

  await coordinator.getByRole('button', { name: 'Ask report', exact: true }).click();
  await expect(coordinator).toContainText('Latest verification');

  await coordinator.getByRole('button', { name: 'New thread', exact: true }).click();
  await expect(coordinator).toContainText('No active mission locked');
  await expect(coordinator.getByRole('button', { name: 'Launch workers', exact: true })).toBeHidden();
  await expect(coordinator.getByRole('button', { name: 'Ask report', exact: true })).toBeHidden();
  await expect(coordinator.getByRole('button', { name: 'Nudge latest worker', exact: true })).toBeHidden();

  const threadSelect = coordinator.getByLabel('Mission threads');
  const previousThreadId = await threadSelect.locator('option').nth(1).getAttribute('value');
  await threadSelect.selectOption(previousThreadId);
  await expect(coordinator).toContainText('Plan approved');
});

import { test, expect } from '@playwright/test';

test('close docks Coordinator and Reviewer with state labels and restores windows', async ({ page }) => {
  await page.goto('/');

  const coordinator = page.locator('.eyeloss-coordinator-window');
  const reviewer = page.locator('.eyeloss-reviewer-window');
  await expect(coordinator).toBeVisible();
  await expect(reviewer).toBeVisible();
  await expect(coordinator).toContainText('Coordinator');
  await expect(coordinator).not.toContainText('Ruth');
  await expect(coordinator).not.toContainText('46');

  await coordinator.getByLabel('Message coordinator').fill('Dock labels need task state.');
  await coordinator.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(coordinator).toContainText('Plan proposal');
  await expect(coordinator).toContainText('Plan ready');

  await reviewer.getByRole('button', { name: 'Ask', exact: true }).click();
  await expect(reviewer).toContainText('Question required.');

  await coordinator.getByRole('button', { name: 'X', exact: true }).click();
  await reviewer.getByRole('button', { name: 'X', exact: true }).click();
  await expect(coordinator).toBeHidden();
  await expect(reviewer).toBeHidden();

  await expect(page.getByRole('button', { name: 'Coordinator', exact: true })).toBeHidden();
  await expect(page.getByRole('button', { name: 'Reviewer', exact: true })).toBeHidden();

  const coordinatorDock = page.getByRole('button', { name: /Coordinator\s+Plan ready/ });
  const reviewerDock = page.getByRole('button', { name: /Review\s+Working copy only/ });
  await expect(coordinatorDock).toBeVisible();
  await expect(reviewerDock).toBeVisible();

  await coordinatorDock.click();
  await expect(coordinator).toBeVisible();
  await expect(reviewer).toBeHidden();

  await reviewerDock.click();
  await expect(reviewer).toBeVisible();
});

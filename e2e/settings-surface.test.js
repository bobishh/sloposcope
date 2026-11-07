import { test, expect } from '@playwright/test';

test('settings shows durable app config without mission or debug operations', async ({ page }) => {
  await page.goto('/');

  // Given graph screen is visible.
  await expect(page.getByRole('button', { name: 'Settings', exact: true })).toBeVisible();

  // When user opens Settings.
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const settings = page.locator('.eyeloss-control-page');

  // Then Settings shows durable app configuration.
  await expect(settings.getByText('Settings', { exact: true })).toBeVisible();
  await expect(settings.getByRole('heading', { name: 'Application Config' })).toBeVisible();

  // And operational surfaces stay hidden from the Settings page.
  await expect(settings.getByRole('button', { name: 'Mission', exact: true })).toHaveCount(0);
  await expect(settings.getByRole('button', { name: 'Runs', exact: true })).toHaveCount(0);
  await expect(settings.getByRole('button', { name: 'Debug', exact: true })).toHaveCount(0);
  await expect(settings).not.toContainText('Enterprise Controls');
  await expect(settings).not.toContainText('Issue Context');
});

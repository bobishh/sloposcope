import { test, expect } from '@playwright/test';

test('Given the existing workspace, When panning and inspecting, Then scene diagnostics update without replacing windows or history', async ({ page }) => {
  await page.goto('/');
  const map = page.locator('.eyeloss-graph-canvas');
  await expect(map).toHaveAttribute('data-scene-runtime', 'spatial');
  await expect(map).toHaveAttribute('data-graph-seed', 'commit:c0ffee11');
  await expect(page.locator('.eyeloss-timeline')).toBeVisible();
  const closes = page.locator('.window .window-close');
  while (await closes.count()) await closes.first().click();
  await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();
  await expect(page.locator('.window--active').getByRole('region', { name: 'Bug comparison' })).toBeVisible();
  await page.locator('.window--active').getByRole('button', { name: 'Diff', exact: true }).click();
  await expect(page.locator('.window--active')).toContainText('Diff Changes');
  await page.locator('.window--active .window-close').click();
  const before = await map.getAttribute('data-camera-y');
  await map.hover({ position: { x: 100, y: 100 } });
  await page.mouse.wheel(0, 140);
  await expect(map).not.toHaveAttribute('data-camera-y', before);
});

test('Given pending anatomy loading, When the scene stays interactive, Then pending content and local navigation survive', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('sloposcope.mockAnatomyDelay', '400'));
  await page.goto('/');
  const map = page.locator('.eyeloss-graph-canvas');
  await expect(map).toHaveAttribute('data-scene-runtime', 'spatial');
  await expect(page.locator('.eyeloss-timeline')).toBeVisible();
  await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();
  const comparison = page.getByRole('region', { name: 'Bug comparison' });
  await expect(comparison).toHaveAttribute('data-status', 'pending');
  await expect(comparison).not.toHaveAttribute('data-status', 'pending');
});

test('Given unavailable history, When a repository opens, Then an error replaces the loader without falling back to all files', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('sloposcope.mockHistoryFailure', '1'));
  await page.goto('/');
  await expect(page.getByText('Cannot load history: Mock history unavailable', { exact: true })).toBeVisible();
  await expect(page.locator('.eyeloss-graph-canvas')).toHaveAttribute('data-file-count', '0');
  await expect(page.getByRole('button', { name: 'Atlas', exact: true })).toBeEnabled();
});

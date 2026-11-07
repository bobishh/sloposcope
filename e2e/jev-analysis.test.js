import { test, expect } from '@playwright/test';

async function openAnalysis(page) {
  await page.goto('/');
  const closes = page.locator('.window .window-close');
  while (await closes.count()) await closes.first().click();
  await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();
  await page.getByRole('button', { name: 'Analyze file with Jev', exact: true }).click();
  return page.getByRole('region', { name: 'Jev analysis' });
}

test('Given file-scoped preview, When cancelled, Then no request is sent and the graph remains open', async ({ page }) => {
  const panel = await openAnalysis(page);
  await expect(panel.getByRole('heading', { name: 'Send confirmation' })).toBeVisible();
  await expect(panel).toContainText('1 request');
  await expect(panel).toContainText('1 uncached source groups');
  await expect(panel).toContainText('No requests sent');
  await expect(panel.getByRole('textbox', { name: 'Jev API token' })).toHaveCount(0);
  await panel.getByRole('button', { name: 'Cancel preview' }).click();
  await expect(panel.getByRole('heading', { name: 'Send confirmation' })).toHaveCount(0);
  await expect(page.locator('.eyeloss-graph-canvas')).toBeVisible();
});

test('Given cached file interpretation, When requested again, Then local cache is reused without another send', async ({ page }) => {
  const panel = await openAnalysis(page);
  await panel.getByRole('button', { name: 'Confirm and send 1 request', exact: true }).click();
  await expect(panel).toContainText('Requests attempted: 1');
  await panel.getByRole('button', { name: 'Close analysis' }).click();
  await page.getByRole('button', { name: 'Analyze file with Jev', exact: true }).click();
  await expect(panel).toContainText('1 cached');
  await expect(panel.getByRole('region', { name: 'Jev traits' })).toContainText('transformation');
  await expect(panel.getByRole('button', { name: 'Confirm and send 1 request' })).toHaveCount(0);
});

test('Given source changes during a send, When result returns, Then old snapshot is not attached to current file', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('sloposcope.mockJevChangeDuringSend', '1'));
  const panel = await openAnalysis(page);
  await panel.getByRole('button', { name: 'Confirm and send 1 request', exact: true }).click();
  await expect(panel).toContainText('Requests attempted: 1');
  await expect(panel.getByRole('region', { name: 'Jev traits' })).toHaveCount(0);
  await expect(page.getByRole('region', { name: 'Interpreted anatomy' })).toHaveCount(0);
  await expect(panel.getByRole('button', { name: 'Preview file analysis' })).toBeEnabled();
});

test('Given a historical file different from the working copy, When analysis is requested, Then the wrong source cannot be sent', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('sloposcope.mockAnatomy', JSON.stringify({
    'mock.ex': { file: 'mock.ex', sourceId: 'historical-source', profileId: 'ex-v1', status: 'partial', callables: [], stateSlots: null },
  })));
  const panel = await openAnalysis(page);
  await expect(panel.getByRole('alert')).toContainText('Selected snapshot differs from the working copy');
  await expect(panel.getByRole('button', { name: 'Confirm and send 1 request' })).toHaveCount(0);
});

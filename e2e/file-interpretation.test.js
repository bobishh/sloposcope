import { test, expect } from '@playwright/test';

async function openFile(page) {
  await page.goto('/');
  const closes = page.locator('.window .window-close');
  while (await closes.count()) await closes.first().click();
  await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();
  return page.locator('.window').filter({ has: page.getByRole('region', { name: 'Bug comparison' }) });
}

test('Given local anatomy, When file analysis is requested, Then only its explicit confirmation enriches the existing bug', async ({ page }, testInfo) => {
  let sends = 0;
  await page.route('https://api.typesafe.ai/**', route => { sends++; return route.abort(); });
  const inspector = await openFile(page);
  await expect(inspector.getByRole('region', { name: 'Interpreted anatomy' })).toHaveCount(0);
  await inspector.getByRole('button', { name: 'Analyze file with Jev', exact: true }).click();
  const panel = inspector.getByRole('region', { name: 'Jev analysis' });
  await expect(panel).toContainText('mock.ex');
  await expect(panel).not.toContainText('mock_helper.ex');
  await expect(panel.getByRole('checkbox')).toHaveCount(0);
  await expect(inspector.getByRole('region', { name: 'Specimen diagram' }).last()).toBeVisible();
  await panel.getByRole('button', { name: 'Confirm and send 1 request', exact: true }).click();
  await expect(panel.getByRole('status')).toContainText('Sending');
  await expect(inspector.getByRole('region', { name: 'Interpreted anatomy' })).toContainText('mixing: 0.20');
  await expect(inspector.getByRole('region', { name: 'Interpreted anatomy' })).toContainText('hiding: 1.10');
  await page.screenshot({ path: testInfo.outputPath('file-interpretation-wide.png') });
  await page.setViewportSize({ width: 560, height: 850 });
  await page.screenshot({ path: testInfo.outputPath('file-interpretation-narrow.png') });
  await panel.getByRole('button', { name: 'Close analysis' }).click();
  await expect(inspector.getByRole('region', { name: 'Interpreted anatomy' })).toBeVisible();
  expect(sends).toBe(0);
});

test('Given failed analysis, When a file request fails, Then local anatomy survives without automatic retry', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('sloposcope.mockJevFailure', '1'));
  const inspector = await openFile(page);
  await inspector.getByRole('button', { name: 'Analyze file with Jev', exact: true }).click();
  const panel = inspector.getByRole('region', { name: 'Jev analysis' });
  await panel.getByRole('button', { name: 'Confirm and send 1 request', exact: true }).click();
  await expect(panel.getByRole('alert')).toContainText('Stopped after request failure');
  await expect(inspector.getByRole('region', { name: 'Interpreted anatomy' })).toHaveCount(0);
  await expect(inspector.getByRole('region', { name: 'Specimen diagram' }).last()).toBeVisible();
  await expect(panel.getByRole('button', { name: 'Confirm and send 1 request' })).toHaveCount(0);
});

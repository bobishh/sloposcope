import { test, expect } from '@playwright/test';

test.describe('Repository organism graph and bug specimen', () => {
  test('enriches the existing graph with path regions and opens evidence-backed specimens', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('.eyeloss-spinner')).not.toBeVisible();

    const graph = page.locator('canvas[aria-label="Repository organism graph"]');
    await expect(graph).toBeVisible();
    await expect(graph).toHaveAttribute('data-renderer', 'organism-regions');
    await expect(graph).toHaveAttribute('data-file-count', '2');
    await expect(graph).toHaveAttribute('data-path-region-count', '1');
    await expect(graph).toHaveAttribute('data-static-link-count', '1');
    await expect(graph).toHaveAttribute('data-phenotype-grammar', 'shared-callable-metrics');
    await expect(graph).toHaveAttribute('data-lod-mode', 'nonlinear-semantic');
    await expect(page.getByRole('button', { name: 'Ecosystem scene' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Graph scene' })).toHaveCount(0);

    const preopenedWindows = page.locator('.window .window-close');
    while (await preopenedWindows.count()) {
      await preopenedWindows.first().click();
    }

    await graph.hover();
    await page.keyboard.down('Control');
    for (let index = 0; index < 8; index += 1) await page.mouse.wheel(0, 100);
    await page.keyboard.up('Control');
    await expect(graph).toHaveAttribute('data-detail-level', 'repository');
    await page.keyboard.down('Control');
    for (let index = 0; index < 10; index += 1) await page.mouse.wheel(0, -100);
    await page.keyboard.up('Control');
    await expect(graph).toHaveAttribute('data-detail-level', 'anatomy');

    await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();

    const ready = page.locator('.bug-specimen[data-status="partial"]');
    await expect(ready).toBeVisible();
    await expect(ready.locator('canvas[aria-label="Bug specimen for mock.ex"]')).toBeVisible();
    await expect(ready).toContainText('hello');
    await expect(ready).toContainText('secret');
    await expect(ready).toContainText('public');
    await expect(ready).toContainText('private');
    await expect(ready).toContainText('arity unknown');
    await expect(page.getByRole('button', { name: 'Bug', exact: true }).last()).toHaveAttribute('aria-pressed', 'true');

    await page.getByRole('button', { name: 'Source', exact: true }).last().click();
    await expect(page.getByText('Full Source Code').last()).toBeVisible();
    await page.locator('.window--active .window-close').click();

    await page.getByRole('button', { name: 'MockHelper changed file' }).click();
    const afterEndpoint = page.getByRole('region', { name: 'After endpoint' });
    const unavailable = afterEndpoint.locator('[data-endpoint-state="unavailable"]');
    await expect(unavailable).toBeVisible();
    await expect(unavailable).toContainText('After unavailable');
    await expect(unavailable).toContainText('no Tree-sitter anatomy profile');
    await expect(unavailable).not.toContainText('0 callables');
    await expect(unavailable).not.toContainText('0 state slots');
    await page.locator('.window--active .window-close').click();
  });
});

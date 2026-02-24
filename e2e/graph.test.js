import { test, expect } from '@playwright/test';

test.describe('CodeGraph E2E', () => {
  test('should load the graph and display mock data', async ({ page }) => {
    await page.goto('/');

    // Verify mock data is loaded
    const repoPath = page.locator('.code-graph-controls__repo');
    await expect(repoPath).toHaveText('/path/to/despair');

    // Wait for spinner to disappear
    await expect(page.locator('.code-graph-spinner')).not.toBeVisible();

    // Check if the canvas is there
    const canvas = page.locator('canvas');
    await expect(canvas).toBeVisible();

    // Check the nav panel for the mock node (now shows filenames)
    const navPanel = page.locator('.code-graph-nav-panel');
    await expect(navPanel).toContainText('mock.ex');
  });

  test('should allow changing the repository path', async ({ page }) => {
    await page.goto('/');

    // Click "Change" button (the mock returns /new/path/to/despair)
    const changeBtn = page.locator('button', { hasText: 'Change' });
    await changeBtn.click();

    // Verify path changed
    const repoPath = page.locator('.code-graph-controls__repo');
    await expect(repoPath).toHaveText('/new/path/to/despair');
  });

  test('should toggle the nav panel', async ({ page }) => {
    await page.goto('/');
    
    const toggleBtn = page.locator('.code-graph-nav-panel__toggle');
    await expect(page.locator('.code-graph-nav-panel__search')).toBeVisible();
    
    await toggleBtn.click();
    await expect(page.locator('.code-graph-nav-panel__search')).not.toBeVisible();
  });

  test('should display and interact with the timeline', async ({ page }) => {
    await page.goto('/');

    // Wait for data
    await expect(page.locator('.code-graph-timeline')).toBeVisible();

    // Check if we have 3 points (mock data has 3 commits)
    const ticks = page.locator('.code-graph-timeline__tick');
    await expect(ticks).toHaveCount(3);

    // Hover over a tick to see meta
    await ticks.nth(1).hover();
    const meta = ticks.nth(1).locator('.code-graph-timeline__tick-meta');
    await expect(meta).toBeVisible();
    await expect(meta).toContainText('c0ffee11');

    // Click a tick to select it
    await ticks.nth(1).click();
    
    // Verify input changed
    const sinceInput = page.locator('#since-input');
    await expect(sinceInput).toHaveValue('c0ffee11');
  });

  test('should support multi-select with Shift key', async ({ page }) => {
    await page.goto('/');

    const ticks = page.locator('.code-graph-timeline__tick');
    
    // Select the last one (deadbeef)
    await ticks.nth(2).click();
    
    // Shift-click the second one (c0ffee11)
    await page.keyboard.down('Shift');
    await ticks.nth(1).click();
    await page.keyboard.up('Shift');

    // Verify aggregate revset in input
    const sinceInput = page.locator('#since-input');
    await expect(sinceInput).toHaveValue('deadbeef | c0ffee11');
  });
});

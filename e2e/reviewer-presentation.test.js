import { test, expect } from '@playwright/test';

test('reviewer shows task-facing review without generated internals', async ({ page }) => {
  // Given frontend trace logs enabled so backend command usage visible.
  const traceLines = [];
  page.on('console', (message) => {
    const text = message.text();
    if (text.includes('[FRONTEND][TRACE]') || text.includes('[MOCK] invoke')) {
      traceLines.push(text);
    }
  });
  await page.addInitScript(() => {
    window.localStorage.setItem('sloposcope-debug', '1');
  });

  // Given repo graph is open.
  await page.goto('/');

  const reviewer = page.locator('.eyeloss-reviewer-window');
  await expect(reviewer).toBeVisible();
  await expect(reviewer).toContainText('Code reviewer');
  await expect(reviewer).toContainText('Review workspace');
  await expect(reviewer).toContainText('Target mode');
  await expect(reviewer).toContainText('Working copy only');
  await expect(reviewer).not.toContainText('Generated Elm reviewer host');
  await expect(reviewer).not.toContainText('Generated reviewer waiting.');

  // When question is blank.
  await reviewer.getByRole('button', { name: 'Ask', exact: true }).click();

  // Then failure state is visible.
  await expect(reviewer).toContainText('Question required.');

  // When branch review is generated.
  const reviewBranchButton = reviewer.getByRole('button', { name: 'Review branch', exact: true });
  await reviewBranchButton.click();

  // Then pending state blocks duplicate submit while compile in progress.
  await expect(reviewBranchButton).toBeDisabled();

  // Then backend review context command called before compile command.
  await expect
    .poll(() => traceLines.some((line) => line.includes('get_reviewer_target_context')))
    .toBe(true);
  await expect
    .poll(() => traceLines.some((line) => line.includes('compile_reviewer_elm')))
    .toBe(true);
  const contextCallIndex = traceLines.findIndex((line) => line.includes('get_reviewer_target_context'));
  const compileCallIndex = traceLines.findIndex((line) => line.includes('compile_reviewer_elm'));
  expect(contextCallIndex).toBeGreaterThan(-1);
  expect(compileCallIndex).toBeGreaterThan(contextCallIndex);

  // Then review content mounts with task-facing labels.
  await expect(reviewer).toContainText('Review');
  await expect(reviewer).toContainText('Evidence');
  await expect(reviewer).toContainText('Decision');
  await expect(reviewer).not.toContainText('Branch review by generated Elm');
  await expect(reviewer).not.toContainText('Compiled Elm artifact');
  await expect(reviewer.locator('[data-surface-id="reviewer"] pre').first()).toContainText('defmodule MockModule');

  // Then selected target context remains visible after review compile.
  await expect(reviewer).toContainText('Target mode');
  await expect(reviewer).toContainText('Working copy only');
});

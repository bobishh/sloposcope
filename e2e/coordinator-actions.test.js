import { test, expect } from '@playwright/test';

test('should reveal coordinator actions only when actionable and keep thread resume labels clean', async ({ page }) => {
  await page.goto('/');

  const coordinator = page.locator('.eyeloss-coordinator-window');
  await expect(coordinator).toBeVisible();
  await expect(coordinator.getByRole('button', { name: 'Approve plan', exact: true })).toBeHidden();
  await expect(coordinator.getByRole('button', { name: 'Launch workers', exact: true })).toBeHidden();
  await expect(coordinator.getByRole('button', { name: 'Ask report', exact: true })).toBeHidden();
  await expect(coordinator.getByRole('button', { name: 'Nudge latest worker', exact: true })).toBeHidden();

  await coordinator.getByLabel('Message coordinator').fill('Coordinator action buttons should appear only when useful.');
  await coordinator.getByRole('button', { name: 'Send', exact: true }).click();

  await expect(coordinator).toContainText('Plan proposal');
  await expect(coordinator).toContainText('Active task');
  await expect(coordinator.getByRole('button', { name: 'Approve plan', exact: true })).toBeVisible();
  await expect(coordinator.getByRole('button', { name: 'Launch workers', exact: true })).toBeHidden();
  await expect(coordinator.getByRole('button', { name: 'Ask report', exact: true })).toBeHidden();
  await expect(coordinator.getByRole('button', { name: 'Nudge latest worker', exact: true })).toBeHidden();

  await coordinator.getByRole('button', { name: 'Approve plan', exact: true }).click();
  await expect(coordinator).toContainText('Ready to launch workers');
  await expect(coordinator.getByRole('button', { name: 'Approve plan', exact: true })).toBeHidden();
  await expect(coordinator.getByRole('button', { name: 'Launch workers', exact: true })).toBeVisible();
  await expect(coordinator.getByRole('button', { name: 'Ask report', exact: true })).toBeVisible();
  await expect(coordinator.getByRole('button', { name: 'Nudge latest worker', exact: true })).toBeHidden();

  await coordinator.getByRole('button', { name: 'Launch workers', exact: true }).click();
  await expect(coordinator).toContainText('Workers launched');
  await expect(coordinator.getByRole('button', { name: 'Ask report', exact: true })).toBeVisible();
  await expect(coordinator.getByRole('button', { name: 'Nudge latest worker', exact: true })).toBeHidden();

  await coordinator.getByRole('button', { name: 'Ask report', exact: true }).click();
  await expect(coordinator).toContainText('Mission report');
  await expect(coordinator.getByRole('button', { name: 'Ask report', exact: true })).toBeVisible();

  const lifecycleEvents = await page.evaluate(() => {
    const states = JSON.parse(window.localStorage.getItem('sloposcope.mockCoordinatorStateByRepo') || '{}');
    return Object.values(states).flatMap((state) => state.lifecycleEvents || []);
  });
  expect(lifecycleEvents.map((event) => event.type)).toEqual(expect.arrayContaining([
    'task.created',
    'plan.proposed',
    'plan.approved',
    'workers.launched',
    'coordinator.reported',
    'worker.started',
  ]));
  expect(lifecycleEvents.find((event) => event.type === 'worker.started')).toMatchObject({
    taskId: expect.any(String),
    threadId: expect.any(String),
    actor: expect.any(String),
    source: 'agent_session',
    payload: expect.objectContaining({
      objectiveId: expect.any(String),
      sessionId: expect.any(String),
    }),
  });

  const threadSelect = coordinator.getByLabel('Mission threads');
  const firstOptionText = await threadSelect.locator('option').first().innerText();
  expect(firstOptionText).not.toMatch(/\[[^\]]+\]/);
});

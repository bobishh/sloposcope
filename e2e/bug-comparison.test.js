import { test, expect } from '@playwright/test';

const comparison = {
  file: 'mock.ex',
  comparisonId: 'base..head',
  hasBaseline: true,
  before: {
    label: 'base', sourceId: 'before-hash', status: 'available',
    anatomy: {
      status: 'partial', file: 'mock.ex', language: 'ex', evidence: 'tree_sitter', profileId: 'ex-v1',
      callables: [
        { id: 'before:outer', matchKey: 'module:MockModule|def|outer', owner: 'MockModule', name: 'outer', kind: 'def', visibility: 'public', bodyLineSpan: 3, decisionCount: 1, startLine: 2, endLine: 4 },
        { id: 'before:gone', matchKey: 'module:OldModule|def|gone', owner: 'OldModule', name: 'gone', kind: 'def', visibility: 'private', bodyLineSpan: 2, decisionCount: 0, startLine: 6, endLine: 7 },
      ], stateSlots: null,
    },
  },
  after: {
    label: 'head', sourceId: 'after-hash', status: 'available',
    anatomy: {
      status: 'partial', file: 'mock.ex', language: 'ex', evidence: 'tree_sitter', profileId: 'ex-v1',
      callables: [
        { id: 'after:outer', matchKey: 'module:MockModule|def|outer', owner: 'MockModule', name: 'outer', kind: 'def', visibility: 'public', bodyLineSpan: 5, decisionCount: 1, startLine: 2, endLine: 6 },
        { id: 'after:new', matchKey: 'module:NewModule|def|new', owner: 'NewModule', name: 'new', kind: 'def', visibility: 'public', bodyLineSpan: 1, decisionCount: 0, startLine: 8, endLine: 8 },
      ], stateSlots: null,
    },
  },
};

test('Given a selected comparison, When opening a file, Then aligned bugs show measured Before and After while text views remain available', async ({ page }) => {
  await page.addInitScript((fixture) => {
    localStorage.setItem('sloposcope.mockAnatomyComparison', JSON.stringify({ 'mock.ex': fixture }));
  }, comparison);
  await page.goto('/');
  const closes = page.locator('.window .window-close');
  while (await closes.count()) await closes.first().click();
  await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();

  const dialog = page.locator('.window--active');
  await expect(dialog.getByRole('heading', { name: 'Before' })).toBeVisible();
  await expect(dialog.getByRole('heading', { name: 'After' })).toBeVisible();
  await expect(dialog).toContainText('base');
  await expect(dialog).toContainText('head');
  const beforePanel = dialog.getByRole('region', { name: 'Before endpoint' });
  const afterPanel = dialog.getByRole('region', { name: 'After endpoint' });
  const changedLimb = afterPanel.locator('[data-organ-kind="callable"][data-change-status="changed"]');
  await expect(changedLimb).toHaveCount(1);
  const beforeChangedLimb = beforePanel.locator('[data-slot-key="module:MockModule|def|outer"]');
  await expect(beforeChangedLimb).toHaveCount(1);
  await expect(changedLimb).toHaveAttribute('data-slot-key', 'module:MockModule|def|outer');
  expect(await changedLimb.getAttribute('data-layout-y')).toBe(await beforeChangedLimb.getAttribute('data-layout-y'));
  await expect(beforePanel.locator('[data-organ-kind="callable"]')).toHaveCount(2);
  await expect(afterPanel.locator('[data-organ-kind="callable"]')).toHaveCount(2);
  await changedLimb.click();
  const selectedOrgan = afterPanel.getByRole('region', { name: 'Selected organ' });
  await expect(selectedOrgan).toContainText('body lines: 3 → 5 (+2)');
  await expect(afterPanel.locator('[data-organ-kind="absence"]')).toContainText('Not present');
  await expect(beforePanel.locator('[data-organ-kind="absence"]')).toContainText('Not present');
  await expect(dialog.locator('.bug-comparison__changes')).toHaveCount(0);

  await dialog.getByRole('button', { name: 'Diff', exact: true }).click();
  await expect(dialog.getByText('Diff Changes')).toBeVisible();
  await dialog.getByRole('button', { name: 'Source', exact: true }).click();
  await expect(dialog.getByText('Full Source Code')).toBeVisible();
});

test('Given a slow comparison, When anatomy loads, Then both panels and their controls keep position', async ({ page }) => {
  await page.addInitScript((fixture) => {
    localStorage.setItem('sloposcope.mockAnatomyComparison', JSON.stringify({ 'mock.ex': fixture }));
    localStorage.setItem('sloposcope.mockAnatomyDelay', '500');
  }, comparison);
  await page.goto('/');
  const closes = page.locator('.window .window-close');
  while (await closes.count()) await closes.first().click();
  await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();

  const dialog = page.locator('.window--active');
  const comparisonView = dialog.getByRole('region', { name: 'Bug comparison' });
  await expect(comparisonView).toHaveAttribute('data-status', 'pending');
  const panels = dialog.locator('.bug-comparison__panel');
  await expect(panels).toHaveCount(2);
  const pendingGeometry = await panels.evaluateAll(items => items.map((item) => ({
    top: item.getBoundingClientRect().top,
    height: item.getBoundingClientRect().height,
  })));
  const zoomControls = dialog.locator('.bug-specimen__zoom');
  await expect(zoomControls).toHaveCount(2);
  const pendingControls = await zoomControls.evaluateAll(items => items.map((item) => item.getBoundingClientRect().top));

  await expect(comparisonView).toHaveAttribute('data-status', 'ready');
  const readyGeometry = await panels.evaluateAll(items => items.map((item) => ({
    top: item.getBoundingClientRect().top,
    height: item.getBoundingClientRect().height,
  })));
  const readyControls = await zoomControls.evaluateAll(items => items.map((item) => item.getBoundingClientRect().top));
  expect(readyGeometry).toEqual(pendingGeometry);
  expect(readyControls).toEqual(pendingControls);
});

test('Given aligned specimens, When the window is 1000px and 490px wide, Then paired limbs share rows and fit the panel layout', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1600, height: 1100 });
  await page.addInitScript((fixture) => {
    localStorage.setItem('sloposcope.mockAnatomyComparison', JSON.stringify({ 'mock.ex': fixture }));
  }, comparison);
  await page.goto('/');
  const closes = page.locator('.window .window-close');
  while (await closes.count()) await closes.first().click();
  await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();
  const dialog = page.locator('.window--active');
  const panels = dialog.locator('.bug-comparison__panel');
  await expect(panels).toHaveCount(2);
  const beforePanel = dialog.getByRole('region', { name: 'Before endpoint' });
  const afterPanel = dialog.getByRole('region', { name: 'After endpoint' });

  for (const width of [1000, 490]) {
    const box = await dialog.boundingBox();
    const handle = await dialog.locator('.window-resize-handle').boundingBox();
    await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
    await page.mouse.down();
    await page.mouse.move(handle.x + handle.width / 2 + width - box.width, handle.y + handle.height / 2, { steps: 8 });
    await page.mouse.up();
    await expect.poll(async () => Math.round((await dialog.boundingBox()).width)).toBe(width);
    const gridColumns = await dialog.locator('.bug-comparison__panels').evaluate(element => getComputedStyle(element).gridTemplateColumns.split(' ').length);
    expect(gridColumns).toBe(width <= 700 ? 1 : 2);
    for (const panel of [beforePanel, afterPanel]) {
      const stage = panel.getByRole('region', { name: 'Specimen diagram' });
      await expect.poll(() => stage.evaluate(element => {
        const panel = element.closest('.bug-comparison__panel');
        const style = getComputedStyle(panel);
        const available = panel.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
        return element.getBoundingClientRect().width <= available + 1;
      })).toBe(true);
    }
    const beforeLimb = beforePanel.locator('[data-slot-key="module:MockModule|def|outer"]');
    const afterLimb = afterPanel.locator('[data-slot-key="module:MockModule|def|outer"]');
    await expect(beforeLimb).toHaveCount(1);
    await expect(afterLimb).toHaveCount(1);
    expect(await beforeLimb.getAttribute('data-layout-y')).toBe(await afterLimb.getAttribute('data-layout-y'));
    const rightLimb = beforePanel.locator('[data-slot-key="module:OldModule|def|gone"]');
    const rightLabel = rightLimb.locator('.bug-specimen__organ-label');
    const stage = beforePanel.getByRole('region', { name: 'Specimen diagram' });
    const bounds = await Promise.all([rightLabel.boundingBox(), stage.boundingBox()]);
    expect(bounds[0].x).toBeGreaterThanOrEqual(bounds[1].x);
    expect(bounds[0].x + bounds[0].width).toBeLessThanOrEqual(bounds[1].x + bounds[1].width);
    await expect(rightLabel).toContainText('gone');
    await page.screenshot({ path: testInfo.outputPath(`comparison-width-${width}.png`) });
  }
});

test('Given an unavailable selected endpoint, When opening a file, Then it stays unavailable without working-copy limbs', async ({ page }) => {
  const unavailableComparison = {
    file: 'mock.ex',
    comparisonId: 'unreadable..head',
    hasBaseline: true,
    before: { label: 'base', status: 'absent', anatomy: null, reason: 'File did not exist at this endpoint' },
    after: { label: 'head', sourceId: 'head-hash', status: 'unavailable', anatomy: null, reason: 'Historical file read failed' },
  };
  await page.addInitScript((fixture) => {
    localStorage.setItem('sloposcope.mockAnatomyComparison', JSON.stringify({ 'mock.ex': fixture }));
  }, unavailableComparison);
  await page.goto('/');
  const graph = page.locator('.eyeloss-graph-canvas');
  await expect(graph).toHaveAttribute('data-callable-count', '0');
  await expect(graph).toHaveAttribute('data-callable-unknown-files', '2');
  const closes = page.locator('.window .window-close');
  while (await closes.count()) await closes.first().click();
  await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();

  const dialog = page.locator('.window--active');
  const comparisonView = dialog.getByRole('region', { name: 'Bug comparison' });
  await expect(comparisonView).toHaveAttribute('data-status', 'unavailable');
  const beforeEndpoint = dialog.getByRole('region', { name: 'Before endpoint' });
  const afterEndpoint = dialog.getByRole('region', { name: 'After endpoint' });
  await expect(beforeEndpoint.locator('.bug-comparison__state strong')).toHaveText('File did not exist');
  await expect(beforeEndpoint.locator('.bug-comparison__state span')).toContainText('File did not exist');
  await expect(afterEndpoint.locator('.bug-comparison__state span')).toHaveText('Historical file read failed');
  await expect(dialog.locator('.bug-specimen[data-status="partial"]')).toHaveCount(0);
  await expect(dialog.locator('.bug-comparison__panel')).toHaveCount(2);
});

test('Given 300 Before and 1000 After callables, When selecting a matched organ, Then both snapshots show their own file size and first Git appearance on shared anatomy', async ({ page }, testInfo) => {
  const makeCallable = (index, side) => ({
    id: `${side}:callable:${index}`,
    matchKey: `module:MockModule|function|work_${index}`,
    owner: 'MockModule', name: `work_${index}`, kind: 'function', visibility: 'public', arity: 2,
    bodyLineSpan: index === 299 && side === 'before' ? 300 : index === 299 ? 600 : index % 8 + 1,
    decisionCount: index === 299 && side === 'before' ? 3 : index === 299 ? 7 : index % 4,
    startLine: index + 1, endLine: index + 3,
  });
  const endpoint = (side, count, lineCount, firstSeenAt, ageAsOf, ageDays) => ({
    label: side,
    status: 'available',
    sourceId: `${side}-snapshot-hash`,
    anatomy: {
      status: 'partial', file: 'mock.ex', language: 'ex', evidence: 'tree_sitter',
      sourceId: `${side}-snapshot-hash`, profileId: 'ex-v1', lineCount, firstSeenAt, ageAsOf, ageDays, ageReason: null,
      callables: Array.from({ length: count }, (_, index) => makeCallable(index, side)),
      stateSlots: [],
    },
  });
  const fixture = {
    file: 'mock.ex', comparisonId: 'first..latest', hasBaseline: true,
    before: endpoint('before', 300, 2638, '2020-01-01T00:00:00Z', '2022-09-27T00:00:00Z', 1000),
    after: endpoint('after', 1000, 9145, '2020-01-01T00:00:00Z', '2026-10-27T00:00:00Z', 2491),
  };
  await page.addInitScript((value) => {
    localStorage.setItem('sloposcope.mockAnatomyComparison', JSON.stringify({ 'mock.ex': value }));
  }, fixture);
  await page.goto('/');
  const closes = page.locator('.window .window-close');
  while (await closes.count()) await closes.first().click();
  await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();

  const dialog = page.locator('.window--active');
  const beforePanel = dialog.getByRole('region', { name: 'Before endpoint' });
  const afterPanel = dialog.getByRole('region', { name: 'After endpoint' });
  const beforeSpecimen = beforePanel.locator('.bug-specimen[data-status="partial"]');
  const afterSpecimen = afterPanel.locator('.bug-specimen[data-status="partial"]');
  await expect(beforePanel.locator('[data-organ-kind="callable"]')).toHaveCount(300);
  await expect(afterPanel.locator('[data-organ-kind="callable"]')).toHaveCount(1000);
  await expect(beforeSpecimen).toContainText('2,638 physical lines');
  await expect(afterSpecimen).toContainText('9,145 physical lines');
  await expect(beforeSpecimen).toContainText('First Git appearance: 2020-01-01');
  await expect(afterSpecimen).toContainText('2,491 days');
  await dialog.screenshot({ path: testInfo.outputPath('before-after-age-wide.png') });

  const beforeOrgan = beforePanel.locator('[data-slot-key="module:MockModule|function|work_299"]');
  const afterOrgan = afterPanel.locator('[data-slot-key="module:MockModule|function|work_299"]');
  await expect(beforeOrgan).toHaveCount(1);
  await expect(afterOrgan).toHaveCount(1);
  expect(await beforeOrgan.getAttribute('data-layout-y')).toBe(await afterOrgan.getAttribute('data-layout-y'));
  await afterOrgan.click();
  const selectedOrgan = afterPanel.getByRole('region', { name: 'Selected organ' });
  await expect(selectedOrgan).toContainText('body lines: 300 → 600 (+300)');
  await expect(selectedOrgan).toContainText('decisions: 3 → 7 (+4)');
  expect(await selectedOrgan.evaluate(element => element.scrollHeight <= element.clientHeight)).toBe(true);
  await expect(beforeOrgan).toBeInViewport();
  const beforeViewport = await beforePanel.getByRole('region', { name: 'Specimen diagram' }).boundingBox();
  const afterViewport = await afterPanel.getByRole('region', { name: 'Specimen diagram' }).boundingBox();
  const beforeBox = await beforeOrgan.boundingBox();
  const afterBox = await afterOrgan.boundingBox();
  expect(Math.abs((beforeBox.y - beforeViewport.y) - (afterBox.y - afterViewport.y))).toBeLessThan(2);
  await dialog.screenshot({ path: testInfo.outputPath('before-after-selected.png') });
  const mapZoom = await page.locator('.eyeloss-graph-canvas').getAttribute('data-camera-zoom');
  const afterZoom = afterPanel.locator('.bug-specimen__zoom span');
  const initialZoom = await afterZoom.innerText();
  await afterOrgan.focus();
  await page.keyboard.press(`${process.platform === 'darwin' ? 'Meta' : 'Control'}+-`);
  await expect(afterZoom).not.toHaveText(initialZoom);
  await expect(beforePanel.locator('.bug-specimen__zoom span')).toHaveText(await afterZoom.innerText());
  await expect(page.locator('.eyeloss-graph-canvas')).toHaveAttribute('data-camera-zoom', mapZoom);
});

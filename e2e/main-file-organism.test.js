import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

test('Given the existing main application, When opening projects and inspecting files, Then the organism engine preserves tabs, history, source, and ordinary diff', async ({ page }, testInfo) => {
  await page.goto('/');
  await expect(page.locator('.eyeloss-timeline')).toBeVisible();
  await expect(page.locator('.eyeloss-project-tab--active')).toContainText('despair');
  const map = page.locator('.eyeloss-graph-canvas');
  await expect(map).toHaveAttribute('data-file-organism-engine', 'shared-world');

  const closes = page.locator('.window .window-close');
  while (await closes.count()) await closes.first().click();
  await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();
  const window = page.locator('.window--active');
  await expect(window.getByRole('region', { name: 'Bug comparison' })).toBeVisible();
  await window.getByRole('button', { name: 'Diff', exact: true }).click();
  await expect(window.getByText('Diff Changes')).toBeVisible();
  await window.getByRole('button', { name: 'Source', exact: true }).click();
  await expect(window.getByText('Full Source Code')).toBeVisible();
  await window.getByRole('button', { name: 'Bug', exact: true }).click();
  await page.screenshot({ path: testInfo.outputPath('main-organism-with-history.png') });

  await page.getByRole('button', { name: 'Open new project tab' }).click();
  await expect(page.locator('.eyeloss-project-tab:not(.eyeloss-project-tab--new)')).toHaveCount(2);
  await expect(page.locator('.eyeloss-controls__repo')).toHaveText('/new/path/to/despair');
  await page.locator('.eyeloss-project-tab:not(.eyeloss-project-tab--new)').first().click();
  await expect(page.locator('.eyeloss-controls__repo')).toHaveText('/path/to/despair');
  await expect(page.locator('.eyeloss-timeline')).toBeVisible();
});

test('Given the main map, When scrolling and using modified zoom, Then the established camera controls remain available', async ({ page }) => {
  await page.goto('/');
  const map = page.locator('.eyeloss-graph-canvas');
  await expect(map).toHaveAttribute('data-file-organism-engine', 'shared-world');
  const initial = await map.evaluate(canvas => ({ x: Number(canvas.dataset.cameraX), y: Number(canvas.dataset.cameraY), zoom: Number(canvas.dataset.cameraZoom) }));
  await map.dispatchEvent('wheel', { deltaX: 32, deltaY: 48, clientX: 900, clientY: 300, bubbles: true, cancelable: true });
  await expect.poll(() => map.evaluate(canvas => Number(canvas.dataset.cameraX))).not.toBe(initial.x);
  expect(Number(await map.getAttribute('data-camera-zoom'))).toBe(initial.zoom);
  await map.dispatchEvent('wheel', { deltaY: -50, ctrlKey: true, clientX: 900, clientY: 300, bubbles: true, cancelable: true });
  await expect.poll(() => map.evaluate(canvas => Number(canvas.dataset.cameraZoom))).toBeGreaterThan(initial.zoom);
  const closer = Number(await map.getAttribute('data-camera-zoom'));
  await page.locator('.eyeloss-controls__repo').click();
  await page.keyboard.press(`${process.platform === 'darwin' ? 'Meta' : 'Control'}+-`);
  await expect.poll(() => map.evaluate(canvas => Number(canvas.dataset.cameraZoom))).toBeLessThan(closer);
});

test('Given measured roles and first-appearance ages, When the production painter renders them, Then shell forms and colors remain distinguishable', async ({ page }, testInfo) => {
  const source = readFileSync(new URL('../src/lib/fileOrganism.ts', import.meta.url), 'utf8');
  const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  const interpretationSource = readFileSync(new URL('../src/lib/fileInterpretation.ts', import.meta.url), 'utf8');
  const interpretationCompiled = ts.transpileModule(interpretationSource, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/');
  await page.addScriptTag({ content: `window.FileInterpretation = {}; ((exports) => { ${interpretationCompiled} })(window.FileInterpretation); window.FileOrganismRenderer = {}; ((exports, require) => { ${compiled} })(window.FileOrganismRenderer, () => window.FileInterpretation);` });
  const colors = await page.evaluate(() => {
    const canvas = document.createElement('canvas');
    canvas.width = 1120;
    canvas.height = 760;
    canvas.style.cssText = 'position:fixed;inset:0;z-index:2147483647;width:1120px;height:760px';
    canvas.setAttribute('aria-label', 'Production file organism appearance samples');
    document.body.append(canvas);
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = '#1a1a2e';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    const samples = [
      { role: 'source', file: 'src/small.rs', lines: 3, count: 2, age: 10 },
      { role: 'source', file: 'src/module.rs', lines: 9145, count: 24, age: 2200 },
      { role: 'test', file: 'tests/module.test.ts', lines: 1000, count: 8, age: 90 },
      { role: 'config', file: 'vitest.config.ts', lines: 80, count: 2, age: 1200 },
      { role: 'docs', file: 'docs/guide.md', lines: 600, count: 0, age: 3000 },
      { role: 'data', file: 'fixtures/catalog.json', lines: 1200, count: 0, age: null },
    ];
    return samples.map((sample, index) => {
      const x = 18 + index % 3 * 366;
      const y = 18 + Math.floor(index / 3) * 370;
      const asOf = new Date('2026-11-04T00:00:00Z');
      const anatomy = {
        file: sample.file, language: sample.file.split('.').at(-1), status: 'partial',
        sourceId: `fixture:${sample.file}`, profileId: 'appearance-fixture', lineCount: sample.lines,
        ageDays: sample.age, ageAsOf: sample.age == null ? null : asOf.toISOString(),
        firstSeenAt: sample.age == null ? null : new Date(asOf.getTime() - sample.age * 86400000).toISOString(),
        ageReason: sample.age == null ? 'No Git history' : null,
        callables: Array.from({ length: sample.count }, (_, ordinal) => ({
          id: `${sample.file}:${ordinal}`, name: `work_${ordinal}`, kind: 'function',
          visibility: ['public', 'private', 'protected'][ordinal % 3], bodyLineSpan: ordinal * 4 + 1, decisionCount: ordinal % 5,
        })), stateSlots: null,
      };
      const geometry = FileOrganismRenderer.buildFileOrganismGeometry({ anatomy, lineCount: sample.lines, roleFact: sample.role });
      ctx.strokeStyle = '#2a2a4a';
      ctx.strokeRect(x, y, 350, 350);
      ctx.fillStyle = '#e0e0e0';
      ctx.font = 'bold 13px monospace';
      ctx.textAlign = 'left';
      ctx.fillText(sample.file, x + 12, y + 24);
      ctx.font = '11px monospace';
      ctx.fillText(`${sample.count} functions · ${sample.lines} lines`, x + 12, y + 44);
      ctx.fillText(sample.age == null ? 'First Git appearance unknown' : `${sample.age} days since first Git appearance`, x + 12, y + 61);
      const scale = Math.min(320 / geometry.bounds.width, 260 / geometry.bounds.height);
      const origin = {
        x: x + 175 - (geometry.bounds.minX + geometry.bounds.maxX) / 2 * scale,
        y: y + 205 - (geometry.bounds.minY + geometry.bounds.maxY) / 2 * scale,
      };
      FileOrganismRenderer.paintFileOrganism(ctx, geometry, { ...origin, scale });
      const pixel = ctx.getImageData(Math.round(origin.x), Math.round(origin.y + (geometry.body.y + geometry.body.height / 2) * scale), 1, 1).data;
      return [...pixel].join(',');
    });
  });
  expect(new Set(colors).size).toBeGreaterThanOrEqual(4);
  await page.locator('canvas[aria-label="Production file organism appearance samples"]').screenshot({ path: testInfo.outputPath('file-organism-forms-and-age.png') });
});

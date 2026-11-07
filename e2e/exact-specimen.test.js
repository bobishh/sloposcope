import { test, expect } from '@playwright/test';

const anatomy = (stateSlots = null) => ({
  status: 'partial', file: 'mock.ex', language: 'ex', evidence: 'tree_sitter',
  callables: Array.from({ length: 17 }, (_, i) => ({
    name: i === 16 ? 'assertRequiredMeshCapabilitiesWithFullName' : i === 15 ? 'function_0' : `function_${i}`,
    kind: 'def', visibility: ['public', 'private', 'protected', 'unknown'][i % 4],
    arity: i === 1 ? 2 : null, startLine: i === 1 ? 12 : null, endLine: i === 1 ? 18 : null,
    id: `callable-${i}`, matchKey: `MockModule::function_${i}`,
    bodyLineSpan: i === 1 ? 7 : null, decisionCount: i === 1 ? 2 : null,
  })),
  stateSlots,
});

async function openSpecimen(page, data) {
  await page.addInitScript(data => localStorage.setItem('sloposcope.mockAnatomy', JSON.stringify({ 'mock.ex': data })), data);
  await page.goto('/');
  const closes = page.locator('.window .window-close');
  while (await closes.count()) await closes.first().click();
  await page.getByRole('button', { name: 'MockModule 1-hop dependency' }).click();
  return page.locator('.bug-specimen[data-status="partial"]');
}

test('Given seventeen captures, When opening Bug, Then each function has one labelled limb and no detached list', async ({ page }, testInfo) => {
  const specimen = await openSpecimen(page, anatomy());
  const beforeEndpoint = page.getByRole('region', { name: 'Before endpoint' });
  await expect(beforeEndpoint).toContainText('No comparison selected');
  await expect(page.locator('.bug-specimen[data-status="partial"]')).toHaveCount(1);
  const stage = specimen.getByRole('region', { name: 'Specimen diagram' });
  await expect(stage.locator('[data-organ-kind="callable"]')).toHaveCount(17);
  await expect(stage.locator('[data-organ-kind="state"]')).toHaveCount(0);
  await expect(stage).toContainText('Stored state unknown');
  await expect(specimen.locator('ul')).toHaveCount(0);
  await expect(page.getByRole('button', { name: /Signatures/ })).toHaveCount(0);
  const privateLimb = stage.getByRole('button', { name: 'Function function_1', exact: true });
  await expect(privateLimb).toHaveAttribute('data-body-lines', '7');
  await expect(privateLimb).toHaveAttribute('data-decisions', '2');
  const rowYs = await stage.locator('[data-organ-kind="callable"]').evaluateAll(items => [...new Set(items.map(item => Number(item.dataset.layoutY)))]);
  expect(rowYs.length).toBeGreaterThan(1);
  await privateLimb.click();
  const selected = specimen.getByRole('region', { name: 'Selected organ' });
  await expect(selected).toContainText('function_1');
  await expect(selected).toContainText('private');
  await expect(selected).toContainText('2 params');
  await expect(selected).toContainText('7 body lines');
  await expect(selected).toContainText('2 decisions');
  await expect(selected).toContainText('12–18');
  const last = stage.getByRole('button', { name: 'Function assertRequiredMeshCapabilitiesWithFullName', exact: true });
  await last.scrollIntoViewIfNeeded();
  await expect(last).toBeVisible();
  await expect(last).toContainText('arity unknown');
  await last.click();
  await expect(selected).toContainText('assertRequiredMeshCapabilitiesWithFullName');
  await page.screenshot({ path: testInfo.outputPath('exact-limbs.png') });
  await expect(specimen).toHaveCSS('border-radius', '0px');
  await expect(stage).toHaveCSS('border-radius', '0px');
});

test('Given fifteen state slots, When zoom changes, Then all named spots and callable identities remain on picture', async ({ page }, testInfo) => {
  const slots = Array.from({ length: 15 }, (_, i) => ({ name: `state_${i}`, kind: 'field', visibility: 'private', startLine: i + 1, endLine: i + 1 }));
  const specimen = await openSpecimen(page, anatomy(slots));
  const stage = specimen.getByRole('region', { name: 'Specimen diagram' });
  await expect(stage.locator('[data-organ-kind="state"]')).toHaveCount(15);
  await expect(stage.locator('[data-organ-kind="callable"]')).toHaveCount(17);
  await specimen.getByRole('button', { name: 'Zoom specimen out', exact: true }).click();
  await expect(stage.locator('[data-organ-kind="state"]')).toHaveCount(15);
  const finalSlot = stage.getByRole('button', { name: 'State state_14', exact: true });
  await finalSlot.scrollIntoViewIfNeeded();
  await finalSlot.click();
  await expect(specimen.getByRole('region', { name: 'Selected organ' })).toContainText('state_14');
  await page.screenshot({ path: testInfo.outputPath('exact-state.png') });
});

test('Given delayed anatomy, When reading, Then pending view claims no organs', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('sloposcope.mockAnatomyDelay', '1200'));
  const specimenPromise = openSpecimen(page, anatomy());
  await expect(page.locator('.bug-specimen[data-status="pending"]')).toHaveCount(2);
  await expect(page.locator('[data-organ-kind]')).toHaveCount(0);
  const specimen = await specimenPromise;
  await expect(specimen.getByRole('region', { name: 'Specimen diagram' })).toBeVisible();
});

test('Given long capture names, When resizing the window, Then names and facts fit their own limbs', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1600, height: 1100 });
  const data = anatomy();
  data.callables[0].name = 'assertRequiredMeshCapabilitiesWithFullName'.repeat(4);
  const specimen = await openSpecimen(page, data);
  const dialog = page.locator('.window').filter({ has: specimen });
  const stage = specimen.getByRole('region', { name: 'Specimen diagram' });
  for (const width of [1000, 490]) {
    const box = await dialog.boundingBox();
    const handle = await dialog.locator('.window-resize-handle').boundingBox();
    await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
    await page.mouse.down();
    await page.mouse.move(handle.x + handle.width / 2 + width - box.width, handle.y + handle.height / 2, { steps: 8 });
    await page.mouse.up();
    await expect.poll(async () => Math.round((await dialog.boundingBox()).width)).toBe(width);
    await expect(stage.locator('[data-organ-kind="callable"]')).toHaveCount(17);
    await expect.poll(() => stage.locator('[data-organ-kind="callable"]').evaluateAll(organs => organs.flatMap(organ => {
      const label = organ.querySelector('.bug-specimen__organ-label');
      const box = label.getBoundingClientRect();
      return [...label.children].flatMap(child => {
        const childBox = child.getBoundingClientRect();
        return childBox.top >= box.top && childBox.bottom <= box.bottom ? [] : [{
          id: organ.dataset.organId, labelWidth: box.width, labelHeight: box.height,
          childWidth: childBox.width, font: getComputedStyle(child).font, childTop: childBox.top - box.top, childBottom: childBox.bottom - box.top,
        }];
      });
    }))).toEqual([]);
    const labelBoxes = await stage.locator('.bug-specimen__organ-label').evaluateAll(labels => labels.map(label => {
      const box = label.getBoundingClientRect();
      return { side: getComputedStyle(label).textAlign, left: box.left, right: box.right, top: box.top, bottom: box.bottom };
    }));
    for (let left = 0; left < labelBoxes.length; left += 1) {
      for (let right = left + 1; right < labelBoxes.length; right += 1) {
        if (labelBoxes[left].side !== labelBoxes[right].side) continue;
        expect(labelBoxes[left].bottom <= labelBoxes[right].top || labelBoxes[right].bottom <= labelBoxes[left].top).toBe(true);
      }
    }
    await stage.evaluate(el => { el.scrollTop = 0; el.scrollLeft = 0; });
    await page.screenshot({ path: testInfo.outputPath(`exact-width-${width}.png`) });
  }
});

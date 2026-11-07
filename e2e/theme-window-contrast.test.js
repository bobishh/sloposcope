import { test, expect } from '@playwright/test';

function luminanceChannel(value) {
  const normalized = value / 255;
  return normalized <= 0.03928
    ? normalized / 12.92
    : ((normalized + 0.055) / 1.055) ** 2.4;
}

function relativeLuminance([red, green, blue]) {
  return (0.2126 * luminanceChannel(red))
    + (0.7152 * luminanceChannel(green))
    + (0.0722 * luminanceChannel(blue));
}

function contrastRatio(foreground, background) {
  const lighter = Math.max(relativeLuminance(foreground), relativeLuminance(background));
  const darker = Math.min(relativeLuminance(foreground), relativeLuminance(background));
  return (lighter + 0.05) / (darker + 0.05);
}

function parseRgb(value) {
  const match = String(value).match(/rgba?\(([^)]+)\)/);
  if (!match) throw new Error(`Unsupported color: ${value}`);
  return match[1].split(',').slice(0, 3).map((part) => Number.parseFloat(part.trim()));
}

async function textContrast(locator) {
  return locator.evaluate((element) => {
    function parseCssRgb(value) {
      const match = String(value).match(/rgba?\(([^)]+)\)/);
      if (!match) return null;
      const parts = match[1].split(',').map((part) => Number.parseFloat(part.trim()));
      return {
        rgb: parts.slice(0, 3),
        alpha: parts.length > 3 ? parts[3] : 1,
      };
    }

    function paintedBackground(start) {
      let current = start;
      while (current && current.nodeType === Node.ELEMENT_NODE) {
        const color = parseCssRgb(getComputedStyle(current).backgroundColor);
        if (color && color.alpha > 0) return color.rgb;
        current = current.parentElement;
      }
      return parseCssRgb(getComputedStyle(document.documentElement).backgroundColor).rgb;
    }

    return {
      color: getComputedStyle(element).color,
      backgroundColor: `rgb(${paintedBackground(element).join(', ')})`,
      text: element.textContent.trim(),
    };
  }).then(({ color, backgroundColor, text }) => ({
    text,
    color,
    backgroundColor,
    ratio: contrastRatio(parseRgb(color), parseRgb(backgroundColor)),
  }));
}

test('light theme keeps Coordinator and Reviewer normal text contrast at WCAG AA', async ({ page }) => {
  // Given light theme is selected before boot.
  await page.addInitScript(() => {
    window.localStorage.setItem('codegraph-theme', 'victorian');
  });

  await page.goto('/');

  // When Coordinator and Reviewer windows render.
  const samples = [
    page.locator('.eyeloss-coordinator-shell__hero-copy p'),
    page.locator('.eyeloss-coordinator-shell__message pre').first(),
    page.locator('.eyeloss-reviewer__target label').first(),
    page.locator('.eyeloss-reviewer__target-summary'),
  ];

  // Then normal text contrast meets WCAG AA.
  for (const sample of samples) {
    await expect(sample).toBeVisible();
    const contrast = await textContrast(sample);
    expect(
      contrast.ratio,
      `${contrast.text} color ${contrast.color} on ${contrast.backgroundColor}`,
    ).toBeGreaterThanOrEqual(4.5);
  }
});

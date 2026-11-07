import { describe, expect, it } from 'vitest';
import { stepScene, resolveSceneCollisions, createFrameScheduler } from './sceneRuntime';

const node = (id: string, x: number, y: number, width = 40, height = 80) => ({ id, x, y, vx: 0, vy: 0, mass: 1, pinned: false, cluster_key: 'src', cluster_target_x: 0, cluster_target_y: 0, cluster_boundary_radius: 10000, organismGeometry: { bounds: { minX: -width / 2, maxX: width / 2, minY: -20, maxY: height - 20, width, height } } });

describe('renderer-independent scene', () => {
  it('Given 1000 distributed nodes, When stepping, Then repulsion avoids all-pairs work and remains finite', () => {
    const nodes = Array.from({ length: 1000 }, (_, i) => node(`${i}`, (i % 32) * 130, Math.floor(i / 32) * 130));
    const result = stepScene(nodes, [], 0);
    expect(result.treeVisits).toBeLessThan(1000 * 999 / 4);
    expect(result.collisionTests).toBeLessThan(1000 * 20);
    expect(nodes.every(n => [n.x, n.y, n.vx, n.vy].every(Number.isFinite))).toBe(true);
  });
  it('Given coincident nodes, When stepping, Then tree construction terminates and pinned positions stay fixed', () => {
    const nodes = Array.from({ length: 300 }, (_, i) => node(`${i}`, 0, 0));
    nodes[0].pinned = true;
    stepScene(nodes, [], 0);
    expect(nodes[0].x).toBe(0); expect(nodes[0].y).toBe(0);
    expect(nodes.every(n => [n.x, n.y].every(Number.isFinite))).toBe(true);
  });
  it('Given a tall file and an overlapping small file, When collisions resolve, Then full off-center bounds participate', () => {
    const tall = node('1000-functions', 0, 0, 130, 15000);
    const small = node('small', 10, 12000);
    tall.pinned = true;
    for (let i = 0; i < 35; i++) resolveSceneCollisions([tall, small]);
    expect(Math.abs(small.x)).toBeGreaterThan(84);
    expect(tall.x).toBe(0);
  });
  it('Given no motion, When invalidated repeatedly, Then one frame runs and scheduling sleeps until the next change', () => {
    let callback: (() => void) | null = null;
    let requests = 0; let renders = 0;
    const scheduler = createFrameScheduler({ request: fn => { callback = fn; return ++requests; }, cancel: () => { callback = null; }, frame: () => { renders++; return false; } });
    scheduler.invalidate(); scheduler.invalidate();
    expect(requests).toBe(1);
    callback!();
    expect(renders).toBe(1); expect(requests).toBe(1);
    scheduler.invalidate(); expect(requests).toBe(2);
    scheduler.dispose(); expect(callback).toBeNull();
  });
});

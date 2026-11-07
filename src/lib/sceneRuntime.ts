export type SceneBounds = { minX: number; minY: number; maxX: number; maxY: number; width: number; height: number };
export type SceneNode = {
  id: string; x: number; y: number; vx: number; vy: number; mass?: number; pinned?: boolean;
  cluster_key?: string; cluster_target_x?: number; cluster_target_y?: number; cluster_boundary_radius?: number;
  organismGeometry: { bounds: SceneBounds };
};
export type SceneEdge = { source: string; target: string };
type Tree = { x: number; y: number; half: number; meanX: number; meanY: number; count: number; cluster: string | null; children: Tree[]; points: number[] };
const clusterKey = (node: SceneNode) => node.cluster_key || node.id.split('/').slice(0, -1).join('/') || 'root';

function buildTree(nodes: SceneNode[], points: number[], x: number, y: number, half: number, depth = 0): Tree {
  let sumX = 0; let sumY = 0;
  let cluster: string | null = clusterKey(nodes[points[0]]);
  for (const index of points) { sumX += nodes[index].x; sumY += nodes[index].y; if (clusterKey(nodes[index]) !== cluster) cluster = null; }
  const tree: Tree = { x, y, half, meanX: sumX / points.length, meanY: sumY / points.length, count: points.length, cluster, children: [], points: [] };
  if (points.length <= 4 || depth >= 20 || half < .01) { tree.points = points; return tree; }
  const buckets: number[][] = [[], [], [], []];
  for (const index of points) buckets[Number(nodes[index].x >= x) + 2 * Number(nodes[index].y >= y)].push(index);
  for (let i = 0; i < 4; i++) if (buckets[i].length) tree.children.push(buildTree(nodes, buckets[i], x + (i % 2 ? .5 : -.5) * half, y + (i >= 2 ? .5 : -.5) * half, half / 2, depth + 1));
  return tree;
}

function applyRepulsion(nodes: SceneNode[]): number {
  if (!nodes.length) return 0;
  let minX = Infinity; let maxX = -Infinity; let minY = Infinity; let maxY = -Infinity;
  for (const n of nodes) { minX = Math.min(minX, n.x); maxX = Math.max(maxX, n.x); minY = Math.min(minY, n.y); maxY = Math.max(maxY, n.y); }
  const root = buildTree(nodes, nodes.map((_, i) => i), (minX + maxX) / 2, (minY + maxY) / 2, Math.max(1, maxX - minX, maxY - minY) / 2 + .01);
  let visits = 0;
  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i];
    if (node.pinned) continue;
    const stack = [root];
    const force = (x: number, y: number, count: number, cluster: string | null, otherIndex = -1) => {
      let dx = x - node.x; let dy = y - node.y;
      if (dx * dx + dy * dy < 1) {
        // Deterministic separation for coincident points; bounded tree leaves prevent infinite subdivision.
        const angle = ((Math.min(i, otherIndex) * 97 + Math.max(i, otherIndex) * 31) % 360) * Math.PI / 180;
        const sign = i < otherIndex ? 1 : -1;
        dx = Math.cos(angle) * sign; dy = Math.sin(angle) * sign;
      }
      const distanceSquared = dx * dx + dy * dy;
      const distance = Math.sqrt(distanceSquared);
      const strength = cluster === clusterKey(node) ? 30000 : 45000;
      const magnitude = Math.min(18, strength / distanceSquared) * count / (node.mass || 1);
      node.vx -= dx / distance * magnitude; node.vy -= dy / distance * magnitude;
    };
    while (stack.length) {
      const tree = stack.pop()!; visits++;
      if (tree.points.length) {
        for (const index of tree.points) if (index !== i) force(nodes[index].x, nodes[index].y, 1, clusterKey(nodes[index]), index);
        continue;
      }
      const dx = tree.meanX - node.x; const dy = tree.meanY - node.y;
      const contains = Math.abs(node.x - tree.x) <= tree.half && Math.abs(node.y - tree.y) <= tree.half;
      if (!contains && (tree.half * 2) ** 2 < .49 * (dx * dx + dy * dy)) force(tree.meanX, tree.meanY, tree.count, tree.cluster);
      else for (const child of tree.children) stack.push(child);
    }
  }
  return visits;
}

/** Broad phase uses full asymmetric anatomy bounds, including tall files and labels. */
export function resolveSceneCollisions(nodes: SceneNode[]): number {
  const boxes = nodes.map(node => ({ node, minX: node.x + node.organismGeometry.bounds.minX - 8,
    maxX: node.x + node.organismGeometry.bounds.maxX + 8, minY: node.y + node.organismGeometry.bounds.minY - 8,
    maxY: node.y + node.organismGeometry.bounds.maxY + 8 })).sort((a, b) => a.minX - b.minX);
  let active: typeof boxes = []; let tests = 0;
  for (const box of boxes) {
    active = active.filter(other => other.maxX > box.minX);
    for (const other of active) {
      if (other.maxY <= box.minY || other.minY >= box.maxY || (other.node.pinned && box.node.pinned)) continue;
      tests++;
      const a = other.node; const b = box.node;
      const boundsA = a.organismGeometry.bounds; const boundsB = b.organismGeometry.bounds;
      const dx = b.x + (boundsB.minX + boundsB.maxX) / 2 - a.x - (boundsA.minX + boundsA.maxX) / 2;
      const dy = b.y + (boundsB.minY + boundsB.maxY) / 2 - a.y - (boundsA.minY + boundsA.maxY) / 2;
      const overlapX = (boundsA.width + boundsB.width) / 2 + 16 - Math.abs(dx);
      const overlapY = (boundsA.height + boundsB.height) / 2 + 16 - Math.abs(dy);
      if (overlapX <= 0 || overlapY <= 0) continue;
      const ratioA = a.pinned ? 0 : b.pinned ? 1 : .5;
      const ratioB = b.pinned ? 0 : a.pinned ? 1 : .5;
      if (overlapX < overlapY) { const push = (dx >= 0 ? 1 : -1) * overlapX * .3; a.x -= push * ratioA; b.x += push * ratioB; }
      else { const push = (dy >= 0 ? 1 : -1) * overlapY * .3; a.y -= push * ratioA; b.y += push * ratioB; }
    }
    active.push(box);
  }
  return tests;
}

/** One outstanding frame; returning false sleeps until explicit invalidation. */
export function createFrameScheduler(options: { request: (callback: () => void) => number; cancel: (id: number) => void; frame: () => boolean }) {
  let pending: number | null = null; let disposed = false;
  const run = () => { pending = null; if (!disposed && options.frame()) invalidate(); };
  function invalidate() { if (!disposed && pending === null) pending = options.request(run); }
  return { invalidate, dispose() { disposed = true; if (pending !== null) options.cancel(pending); pending = null; } };
}

// Physics integration follows below; it has no framework or rendering dependency.
export function stepScene(nodes: SceneNode[], edges: SceneEdge[], iteration: number) {
    const nodeMap: Record<string, SceneNode> = {};
    for (const node of nodes) nodeMap[node.id] = node;
    const springStrength = 0.003;
    const springLength = 320;
    const centerGravity = 0.0008;
    const clusterGravity = 0.015;
    const damping = 0.82;
    const maxVelocity = 20;
    const maxClusterRepulsionForce = 8;
    const worldRadius = Math.max(900, 420 + Math.sqrt(nodes.length) * 200);
    const hardWorldRadius = worldRadius * 1.8;

    const clusterSums: Record<string, { x: number; y: number; count: number }> = {};
    for (const node of nodes) {
      const key = clusterKey(node);
      if (!clusterSums[key]) clusterSums[key] = { x: 0, y: 0, count: 0 };
      clusterSums[key].x += node.x;
      clusterSums[key].y += node.y;
      clusterSums[key].count++;
    }
    const clusterCenters: Record<string, { x: number; y: number }> = {};
    for (const key in clusterSums) {
      const s = clusterSums[key];
      clusterCenters[key] = { x: s.x / s.count, y: s.y / s.count };
    }

    const cKeys = Object.keys(clusterCenters);
    const interClusterRepulsion = 55000;
    const clusterForces: Record<string, { x: number; y: number }> = {};
    for (const key of cKeys) clusterForces[key] = { x: 0, y: 0 };

    for (let i = 0; i < cKeys.length; i++) {
      for (let j = i + 1; j < cKeys.length; j++) {
        const ca = clusterCenters[cKeys[i]];
        const cb = clusterCenters[cKeys[j]];
        let dx = cb.x - ca.x;
        let dy = cb.y - ca.y;
        let dist = Math.sqrt(dx * dx + dy * dy);
        if (dist < 10) dist = 10;

        const force = Math.min(maxClusterRepulsionForce, interClusterRepulsion / (dist * dist));
        const fx = (dx / dist) * force;
        const fy = (dy / dist) * force;

        clusterForces[cKeys[i]].x -= fx;
        clusterForces[cKeys[i]].y -= fy;
        clusterForces[cKeys[j]].x += fx;
        clusterForces[cKeys[j]].y += fy;
      }
    }

    const treeVisits = applyRepulsion(nodes);
    const collisionTests = resolveSceneCollisions(nodes);

    for (const edge of edges) {
      const source = nodeMap[edge.source];
      const target = nodeMap[edge.target];
      if (!source || !target) continue;

      let dx = target.x - source.x;
      let dy = target.y - source.y;
      let dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < 1) dist = 1;

      const sameCluster = clusterKey(source) === clusterKey(target);
      const sl = sameCluster ? springLength * 0.6 : springLength;
      const ss = sameCluster ? springStrength * 1.5 : springStrength * 0.3;

      const displacement = dist - sl;
      const force = ss * displacement;
      const fx = (dx / dist) * force;
      const fy = (dy / dist) * force;
      const invMassSource = 1 / (source.mass || 1);
      const invMassTarget = 1 / (target.mass || 1);

      if (!source.pinned) { source.vx += fx * invMassSource; source.vy += fy * invMassSource; }
      if (!target.pinned) { target.vx -= fx * invMassTarget; target.vy -= fy * invMassTarget; }
    }

    for (const node of nodes) {
      if (node.pinned) continue;

      const key = clusterKey(node);
      const cc = clusterCenters[key];
      const cf = clusterForces[key];
      const invMass = 1 / (node.mass || 1);

      node.vx -= (node.x - cc.x) * clusterGravity * invMass;
      node.vy -= (node.y - cc.y) * clusterGravity * invMass;

      const targetX = Number(node.cluster_target_x || 0);
      const targetY = Number(node.cluster_target_y || 0);
      node.vx += (targetX - cc.x) * 0.0045 * invMass;
      node.vy += (targetY - cc.y) * 0.0045 * invMass;

      const folderDx = node.x - targetX;
      const folderDy = node.y - targetY;
      const folderDistance = Math.sqrt(folderDx * folderDx + folderDy * folderDy);
      const folderBoundary = Number(node.cluster_boundary_radius || 115);
      if (folderDistance > folderBoundary && folderDistance > 0) {
        const overflow = folderDistance - folderBoundary;
        const boundaryForce = overflow * 0.0035;
        node.vx -= (folderDx / folderDistance) * boundaryForce * invMass;
        node.vy -= (folderDy / folderDistance) * boundaryForce * invMass;
      }

      node.vx += cf.x * invMass;
      node.vy += cf.y * invMass;

      node.vx -= node.x * centerGravity * invMass;
      node.vy -= node.y * centerGravity * invMass;

      // Soft boundary: nudge far-away nodes back toward center to prevent long-run drift.
      const radialDist = Math.sqrt(node.x * node.x + node.y * node.y);
      if (radialDist > worldRadius && radialDist > 0) {
        const overflow = radialDist - worldRadius;
        const pull = overflow * 0.0022;
        node.vx -= (node.x / radialDist) * pull;
        node.vy -= (node.y / radialDist) * pull;
      }

      node.vx *= damping;
      node.vy *= damping;

      const speed = Math.sqrt(node.vx * node.vx + node.vy * node.vy);
      const heavyMaxVelocity = Math.max(4.5, maxVelocity / Math.sqrt(node.mass || 1));
      if (speed > heavyMaxVelocity) {
        node.vx = (node.vx / speed) * heavyMaxVelocity;
        node.vy = (node.vy / speed) * heavyMaxVelocity;
      }

      node.x += node.vx;
      node.y += node.vy;

      // Hard boundary as a final safety net.
      const postDist = Math.sqrt(node.x * node.x + node.y * node.y);
      if (postDist > hardWorldRadius && postDist > 0) {
        const scale = hardWorldRadius / postDist;
        node.x *= scale;
        node.y *= scale;
        node.vx *= 0.2;
        node.vy *= 0.2;
      }
    }

    let totalEnergy = 0;
    for (const node of nodes) if (!node.pinned) totalEnergy += node.vx * node.vx + node.vy * node.vy;
    return { active: iteration < 350 || totalEnergy / Math.max(1, nodes.length) >= .01, treeVisits, collisionTests };
}

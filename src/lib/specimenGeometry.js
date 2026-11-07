import {
  buildFileOrganismGeometry,
} from './fileOrganism.ts';
import { classifyPathRole } from './ecosystemModel.js';

function inputFor(anatomy, roleFact) {
  return {
    anatomy,
    // File size belongs to this endpoint. Older snapshots and fixtures remain explicitly unknown.
    lineCount: Number.isFinite(anatomy?.lineCount) && anatomy.lineCount >= 0 ? anatomy.lineCount : null,
    roleFact: roleFact || classifyPathRole({ file: anatomy?.file || '' }).role,
  };
}

function buildEndpoint(anatomy, slotOrder, slotKeys, absenceSlotKeys, rowPitch, roleFact, assessment) {
  return buildFileOrganismGeometry({ ...inputFor(anatomy, roleFact), assessment }, {
    slotOrder,
    callableSlotKeys: slotKeys,
    absenceSlotKeys,
    rowPitch,
  });
}

/**
 * Build both comparison endpoints through the same file-organism engine and shared row pitch.
 * The full slot union plus endpoint-specific absence keys keeps row count and Y coordinates stable.
 * World layout depends on endpoint facts only, never on viewport width.
 */
export function buildSharedSpecimenGeometries(beforeAnatomy, afterAnatomy, _availableWidth, options = {}) {
  const slotOrder = options.slotOrder || [];
  const beforeSlotKeys = options.beforeSlotKeys || [];
  const afterSlotKeys = options.afterSlotKeys || [];
  const beforeAbsentSlotKeys = options.beforeAbsentSlotKeys || [];
  const afterAbsentSlotKeys = options.afterAbsentSlotKeys || [];
  const beforeRole = options.beforeRoleFact || options.roleFact;
  const afterRole = options.afterRoleFact || options.roleFact;
  const beforeInitial = buildEndpoint(beforeAnatomy, slotOrder, beforeSlotKeys, beforeAbsentSlotKeys, 0, beforeRole);
  const afterInitial = buildEndpoint(afterAnatomy, slotOrder, afterSlotKeys, afterAbsentSlotKeys, 0, afterRole);
  const rowPitch = Math.max(beforeInitial.rowPitch, afterInitial.rowPitch);

  return {
    rowPitch,
    before: buildEndpoint(beforeAnatomy, slotOrder, beforeSlotKeys, beforeAbsentSlotKeys, rowPitch, beforeRole, options.assessment),
    after: buildEndpoint(afterAnatomy, slotOrder, afterSlotKeys, afterAbsentSlotKeys, rowPitch, afterRole, options.assessment),
  };
}

export function buildSingleSpecimenGeometry(anatomy, options = {}) {
  return buildEndpoint(
    anatomy,
    options.slotOrder || [],
    options.slotKeys || [],
    options.absenceSlotKeys || [],
    options.rowPitch || 0,
    options.roleFact,
    options.assessment,
  );
}

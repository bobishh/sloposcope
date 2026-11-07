import { projectFileInterpretation, type FileInterpretation } from './fileInterpretation';

export type OrganismPoint = Readonly<{ x: number; y: number }>;
export type OrganismBounds = Readonly<{
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  width: number;
  height: number;
}>;

export type FileOrganismInput = Readonly<{
  anatomy: unknown;
  lineCount: number | null;
  roleFact?: string | null;
  assessment?: unknown;
}>;

export type FileOrganismOptions = Readonly<{
  slotOrder?: readonly string[];
  callableSlotKeys?: readonly string[];
  absenceSlotKeys?: readonly string[];
  rowPitch?: number;
}>;

export type FileOrganismPaintOptions = Readonly<{
  x?: number;
  y?: number;
  scale?: number;
  selectedCallableId?: string | null;
  selectedStateId?: string | null;
  showCallableLabels?: boolean;
  showSelectedCallableLabel?: boolean;
  showStateLabels?: boolean;
  bodyFill?: string;
  selectionColor?: string;
  labelColor?: string;
  labelFontSize?: number;
  detail?: 'full' | 'screen';
}>;

export type FileOrganismLimb = Readonly<{
  id: string;
  matchKey: string | null;
  slotKey: string;
  slotIndex: number;
  dataLayoutY: number;
  row: number;
  side: -1 | 1;
  name: string | null;
  displayName: string;
  kind: string;
  visibility: string | null;
  arity: number | null;
  startLine: number | null;
  endLine: number | null;
  bodyLineSpan: number | null;
  decisionCount: number | null;
  length: number;
  thickness: number;
  terminalSize: number;
  terminal: 'round' | 'square' | 'unknown';
  start: OrganismPoint;
  end: OrganismPoint;
  hitRegion: OrganismBounds;
  labelRegion: OrganismBounds;
  rawCallable: Readonly<Record<string, unknown>>;
}>;

export type FileOrganismGeometry = Readonly<{
  file: string;
  language: string | null;
  sourceId: string | null;
  profileId: string | null;
  status: string;
  lineCount: number | null;
  callableCoverage: 'known' | 'unknown';
  stateCoverage: 'known' | 'unknown';
  firstSeenAt: string | null;
  ageAsOf: string | null;
  ageDays: number | null;
  ageReason: string | null;
  roleFact: string;
  interpretation: FileInterpretation | null;
  head: Readonly<{ width: number; height: number; y: number; bounds: OrganismBounds }>;
  body: Readonly<{
    x: number;
    y: number;
    width: number;
    height: number;
    shape: 'oval' | 'segmented-oval' | 'angular' | 'leaf' | 'round' | 'unknown';
    cornerRadius: number;
  }>;
  appearance: Readonly<{
    bodyColor: string;
    shellColor: string;
    shellBandCount: number;
    ageKnown: boolean;
  }>;
  limbs: readonly FileOrganismLimb[];
  stateSlots: readonly Readonly<{
    id: string;
    index: number;
    name: string;
    kind: string;
    visibility: string | null;
    startLine: number | null;
    endLine: number | null;
    x: number;
    y: number;
    hitRadius: number;
    hitRegion: OrganismBounds;
    labelRegion: OrganismBounds;
    rawState: Readonly<Record<string, unknown>>;
  }>[];
  stateNote: string | null;
  absenceMarkers: readonly Readonly<{
    slotKey: string;
    slotIndex: number;
    row: number;
    side: -1 | 1;
    x: number;
    y: number;
    labelRegion: OrganismBounds;
  }>[];
  rowCount: number;
  rowPitch: number;
  filenameRegion: OrganismBounds;
  bounds: OrganismBounds;
}>;

type RecordValue = Record<string, unknown>;

const BODY_BASE_WIDTH = 60;
const BODY_WIDTH_PER_LINE_STEP = 22;
const BODY_BASE_HEIGHT = 52;
const BODY_VERTICAL_PADDING = 30;
const MIN_ROW_PITCH = 30;
const ROW_GAP = 8;
const MIN_HIT_RADIUS = 9;
const BODY_STROKE = 2;
const UNKNOWN_FILE_WIDTH = 92;
const UNKNOWN_LENGTH = 28;
const ZERO_LENGTH = 12;
const LENGTH_PER_LOG_STEP = 10;
const UNKNOWN_THICKNESS = 3;
const ZERO_THICKNESS = 2;
const THICKNESS_PER_LOG_STEP = 0.9;
const TERMINAL_RADIUS_MIN = 3;
const LABEL_HEIGHT = 18;
const LABEL_CHAR_WIDTH = 7;

const AGE_COLORS = Object.freeze({
  youngest: '#ffbd75',
  recent: '#f1d26a',
  established: '#75d3b2',
  historic: '#86b9ef',
  unknown: '#b8b7e7',
});

const ROLE_COLORS: Readonly<Record<string, string>> = Object.freeze({
  source: '#568ca8',
  test: '#72a878',
  config: '#d09a4a',
  docs: '#a183c2',
  data: '#43a79e',
  unknown: '#778da8',
});

const VISIBILITY_COLORS: Readonly<Record<string, string>> = Object.freeze({
  public: '#ffd166',
  private: '#ff7ac8',
  protected: '#c77dff',
  unknown: '#b9d8ed',
});

const isRecord = (value: unknown): value is RecordValue => Boolean(value) && typeof value === 'object' && !Array.isArray(value);
const finiteNonnegative = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value) && value >= 0;
const optionalString = (value: unknown): string | null => typeof value === 'string' && value.length ? value : null;
const optionalNumber = (value: unknown): number | null => finiteNonnegative(value) ? value : null;
const logMetric = (value: number): number => Math.log1p(value);

function anatomyRecord(value: unknown): RecordValue | null {
  return isRecord(value) ? value : null;
}

function readCallables(anatomy: RecordValue | null): readonly RecordValue[] | null {
  if (!anatomy || String(anatomy.status || '').toLowerCase() === 'unavailable') return null;
  if (!Array.isArray(anatomy.callables)) return null;
  return anatomy.callables.filter(isRecord);
}

function normalizedRole(value: string | null | undefined): string {
  const role = String(value || 'unknown').toLowerCase();
  return role in ROLE_COLORS ? role : 'unknown';
}

function ageAppearance(ageDays: number | null): { shellColor: string; shellBandCount: number; ageKnown: boolean } {
  if (ageDays === null) return { shellColor: AGE_COLORS.unknown, shellBandCount: 0, ageKnown: false };
  if (ageDays < 30) return { shellColor: AGE_COLORS.youngest, shellBandCount: 1, ageKnown: true };
  if (ageDays < 365) return { shellColor: AGE_COLORS.recent, shellBandCount: 2, ageKnown: true };
  if (ageDays < 1825) return { shellColor: AGE_COLORS.established, shellBandCount: 3, ageKnown: true };
  return { shellColor: AGE_COLORS.historic, shellBandCount: 4, ageKnown: true };
}

function blendAgeColor(roleColor: string, ageColor: string, ageKnown: boolean): string {
  if (!ageKnown) return roleColor;
  const blend = (baseChannel: number, ageChannel: number) => Math.round(baseChannel * 0.4 + ageChannel * 0.6);
  const channels = [1, 3, 5].map(offset => blend(
    Number.parseInt(roleColor.slice(offset, offset + 2), 16),
    Number.parseInt(ageColor.slice(offset, offset + 2), 16),
  ));
  return `#${channels.map(channel => channel.toString(16).padStart(2, '0')).join('')}`;
}

function bodyShape(role: string, callableCount: number): FileOrganismGeometry['body']['shape'] {
  if (role === 'source') return callableCount >= 6 ? 'segmented-oval' : 'oval';
  if (role === 'test') return 'segmented-oval';
  if (role === 'config') return 'angular';
  if (role === 'docs') return 'leaf';
  if (role === 'data') return 'round';
  return 'unknown';
}

function makeBounds(minX: number, minY: number, maxX: number, maxY: number): OrganismBounds {
  return Object.freeze({ minX, minY, maxX, maxY, width: maxX - minX, height: maxY - minY });
}

function includeBounds(extents: { minX: number; minY: number; maxX: number; maxY: number }, bounds: OrganismBounds): void {
  extents.minX = Math.min(extents.minX, bounds.minX);
  extents.minY = Math.min(extents.minY, bounds.minY);
  extents.maxX = Math.max(extents.maxX, bounds.maxX);
  extents.maxY = Math.max(extents.maxY, bounds.maxY);
}

function freezeGeometry(geometry: FileOrganismGeometry): FileOrganismGeometry {
  for (const limb of geometry.limbs) {
    Object.freeze(limb.start);
    Object.freeze(limb.end);
    Object.freeze(limb.hitRegion);
    Object.freeze(limb.labelRegion);
    Object.freeze(limb);
  }
  for (const marker of geometry.absenceMarkers) Object.freeze(marker);
  Object.freeze(geometry.body);
  Object.freeze(geometry.appearance);
  Object.freeze(geometry.limbs);
  Object.freeze(geometry.absenceMarkers);
  Object.freeze(geometry.filenameRegion);
  Object.freeze(geometry.bounds);
  return Object.freeze(geometry);
}

function callableSize(callable: RecordValue): number | null {
  return optionalNumber(callable.bodyLineSpan);
}

function callableBranching(callable: RecordValue): number | null {
  return optionalNumber(callable.decisionCount);
}

function limbThickness(decisionCount: number | null): number {
  return decisionCount === null ? UNKNOWN_THICKNESS : ZERO_THICKNESS + THICKNESS_PER_LOG_STEP * logMetric(decisionCount);
}

function limbLength(bodyLineSpan: number | null): number {
  return bodyLineSpan === null ? UNKNOWN_LENGTH : ZERO_LENGTH + LENGTH_PER_LOG_STEP * logMetric(bodyLineSpan);
}

function terminalRadius(thickness: number): number {
  return Math.max(TERMINAL_RADIUS_MIN, thickness * 1.5) + thickness / 2;
}

/** Convert one endpoint's existing FileAnatomy and endpoint-local physical line count into immutable shared world geometry. */
export function buildFileOrganismGeometry(input: FileOrganismInput, options: FileOrganismOptions = {}): FileOrganismGeometry {
  const anatomy = anatomyRecord(input?.anatomy);
  const callables = readCallables(anatomy);
  const lineCount = finiteNonnegative(input?.lineCount) ? input.lineCount : null;
  const roleFact = normalizedRole(input?.roleFact);
  const rawCallables = callables || [];
  const assignedSlotKeys = rawCallables.map((callable, index) => options.callableSlotKeys?.[index]
    || optionalString(callable.matchKey)
    || optionalString(callable.id)
    || `callable:${index}`);
  const defaultOrder = [...assignedSlotKeys, ...(options.absenceSlotKeys || []).filter(key => !assignedSlotKeys.includes(key))];
  const slotOrder = options.slotOrder?.length ? [...options.slotOrder] : defaultOrder;
  for (const key of assignedSlotKeys) if (!slotOrder.includes(key)) slotOrder.push(key);
  for (const key of options.absenceSlotKeys || []) if (!slotOrder.includes(key)) slotOrder.push(key);
  const slotIndexByKey = new Map<string, number>();
  slotOrder.forEach((key, index) => { if (!slotIndexByKey.has(key)) slotIndexByKey.set(key, index); });
  const slotIndexes = assignedSlotKeys.map((key, index) => slotIndexByKey.get(key) ?? index);
  const rowCount = Math.max(Math.ceil(slotOrder.length / 2), 0, ...slotIndexes.map(slotIndex => Math.floor(slotIndex / 2) + 1), ...(options.absenceSlotKeys || []).map(key => {
    const slotIndex = slotIndexByKey.get(key);
    return slotIndex === undefined ? 0 : Math.floor(slotIndex / 2) + 1;
  }));
  const thicknesses = rawCallables.map(callable => limbThickness(callableBranching(callable)));
  const requiredRowPitch = Math.max(MIN_ROW_PITCH, ...thicknesses.map(thickness => 2 * Math.max(MIN_HIT_RADIUS, terminalRadius(thickness)) + ROW_GAP));
  const requestedRowPitch = finiteNonnegative(options.rowPitch) ? options.rowPitch : 0;
  const rowPitch = Math.max(requiredRowPitch, requestedRowPitch);
  const bodyWidth = lineCount === null ? UNKNOWN_FILE_WIDTH : BODY_BASE_WIDTH + BODY_WIDTH_PER_LINE_STEP * Math.log2(lineCount + 1);
  const rawStateSlots = anatomy && Array.isArray(anatomy.stateSlots) ? anatomy.stateSlots.filter(isRecord) : null;
  const stateColumnCount = Math.max(1, Math.min(5, Math.floor((bodyWidth - 16) / 18)));
  const stateRowCount = rawStateSlots ? Math.ceil(rawStateSlots.length / stateColumnCount) : 0;
  const stateAreaHeight = stateRowCount ? stateRowCount * 15 + 12 : 0;
  const bodyHeight = Math.max(BODY_BASE_HEIGHT, BODY_VERTICAL_PADDING + rowCount * rowPitch + stateAreaHeight);
  const body = Object.freeze({
    x: -bodyWidth / 2,
    y: 0,
    width: bodyWidth,
    height: bodyHeight,
    shape: bodyShape(roleFact, rawCallables.length),
    cornerRadius: Math.min(bodyWidth / 2, bodyHeight / 2),
  });
  const headWidth = Math.min(90, bodyWidth * 0.58);
  const head = Object.freeze({ width: headWidth, height: 44, y: -44,
    bounds: makeBounds(-headWidth / 2 - 30, -102, headWidth / 2 + 30, 2) });

  // Top anchoring keeps shared comparison slots at the same local y across different endpoint row counts.
  const bodyTop = 0;
  const rowY = (row: number) => bodyTop + BODY_VERTICAL_PADDING / 2 + rowPitch * (row + 0.5);
  const limbs: FileOrganismLimb[] = rawCallables.map((callable, index) => {
    const slotIndex = slotIndexes[index];
    const side: -1 | 1 = slotIndex % 2 === 0 ? -1 : 1;
    const row = Math.floor(slotIndex / 2);
    const y = rowY(row);
    const bodyLineSpan = callableSize(callable);
    const decisionCount = callableBranching(callable);
    const length = limbLength(bodyLineSpan);
    const thickness = limbThickness(decisionCount);
    const horizontalContour = sideExtentAtY(body, y);
    const start = Object.freeze({ x: side * horizontalContour, y });
    const end = Object.freeze({ x: start.x + side * length, y });
    const terminalSize = Math.max(3, thickness * 1.5);
    const hitRadius = Math.max(MIN_HIT_RADIUS, terminalRadius(thickness));
    const hitRegion = makeBounds(
      Math.min(start.x, end.x) - hitRadius,
      y - hitRadius,
      Math.max(start.x, end.x) + hitRadius,
      y + hitRadius,
    );
    const displayName = optionalString(callable.displayName) || optionalString(callable.name) || `callable ${index + 1}`;
    const labelWidth = Math.max(28, displayName.length * LABEL_CHAR_WIDTH + 12);
    const labelRegion = side < 0
      ? makeBounds(end.x - 8 - labelWidth, y - LABEL_HEIGHT / 2, end.x - 8, y + LABEL_HEIGHT / 2)
      : makeBounds(end.x + 8, y - LABEL_HEIGHT / 2, end.x + 8 + labelWidth, y + LABEL_HEIGHT / 2);
    return Object.freeze({
      id: optionalString(callable.id) || `${String(anatomy?.file || '')}#callable:${index}`,
      matchKey: optionalString(callable.matchKey),
      slotKey: assignedSlotKeys[index],
      slotIndex,
      dataLayoutY: y,
      row,
      side,
      name: optionalString(callable.name),
      displayName,
      kind: optionalString(callable.kind) || 'unknown',
      visibility: optionalString(callable.visibility),
      arity: optionalNumber(callable.arity),
      startLine: optionalNumber(callable.startLine),
      endLine: optionalNumber(callable.endLine),
      bodyLineSpan,
      decisionCount,
      length,
      thickness,
      terminalSize,
      terminal: callable.visibility === 'public' ? 'round' : callable.visibility === 'private' || callable.visibility === 'protected' ? 'square' : 'unknown',
      start,
      end,
      hitRegion,
      labelRegion,
      rawCallable: Object.freeze({ ...callable }),
    });
  });

  const absenceMarkers = (options.absenceSlotKeys || []).flatMap(slotKey => {
    const slotIndex = slotIndexByKey.get(slotKey);
    if (slotIndex === undefined || assignedSlotKeys.includes(slotKey)) return [];
    const row = Math.floor(slotIndex / 2);
    const side: -1 | 1 = slotIndex % 2 === 0 ? -1 : 1;
    const y = rowY(row);
    const contourX = sideExtentAtY(body, y);
    const x = side * contourX;
    const labelWidth = 'Not present'.length * LABEL_CHAR_WIDTH + 12;
    const labelRegion = side < 0
      ? makeBounds(x - 8 - labelWidth, y - LABEL_HEIGHT / 2, x - 8, y + LABEL_HEIGHT / 2)
      : makeBounds(x + 8, y - LABEL_HEIGHT / 2, x + 8 + labelWidth, y + LABEL_HEIGHT / 2);
    return [Object.freeze({ slotKey, slotIndex, row, side, x, y, labelRegion })];
  });

  const stateSlots = (rawStateSlots || []).map((state, index) => {
    const column = index % stateColumnCount;
    const row = Math.floor(index / stateColumnCount);
    const innerWidth = Math.max(0, bodyWidth - 20);
    const x = -innerWidth / 2 + 10 + column * (innerWidth / Math.max(1, stateColumnCount));
    const y = bodyTop + BODY_VERTICAL_PADDING / 2 + rowCount * rowPitch + 8 + row * 15;
    const name = optionalString(state.name) || `state ${index + 1}`;
    const hitRadius = 6;
    return Object.freeze({
      id: `state:${index}:${name}`,
      index,
      name,
      kind: optionalString(state.kind) || 'unknown',
      visibility: optionalString(state.visibility),
      startLine: optionalNumber(state.startLine),
      endLine: optionalNumber(state.endLine),
      x,
      y,
      hitRadius,
      hitRegion: makeBounds(x - hitRadius, y - hitRadius, x + hitRadius, y + hitRadius),
      labelRegion: makeBounds(x + hitRadius + 2, y - 7, x + hitRadius + 2 + name.length * LABEL_CHAR_WIDTH, y + 7),
      rawState: Object.freeze({ ...state }),
    });
  });

  const file = optionalString(anatomy?.file) || '';
  const baseName = file.split(/[\\/]/).at(-1) || file;
  const labelWidth = Math.max(bodyWidth, baseName.length * LABEL_CHAR_WIDTH + 12);
  const filenameRegion = makeBounds(-labelWidth / 2, body.height / 2 + 8, labelWidth / 2, body.height / 2 + 26);
  const extents = { minX: body.x - BODY_STROKE, minY: body.y - BODY_STROKE, maxX: -body.x + BODY_STROKE, maxY: -body.y + BODY_STROKE };
  includeBounds(extents, head.bounds);
  for (const limb of limbs) {
    const halfStroke = Math.max(BODY_STROKE, limb.thickness) / 2;
    includeBounds(extents, makeBounds(Math.min(limb.start.x, limb.end.x) - halfStroke, limb.start.y - halfStroke, Math.max(limb.start.x, limb.end.x) + halfStroke, limb.start.y + halfStroke));
    includeBounds(extents, limb.hitRegion);
    includeBounds(extents, limb.labelRegion);
  }
  for (const marker of absenceMarkers) {
    includeBounds(extents, makeBounds(marker.x - MIN_HIT_RADIUS, marker.y - MIN_HIT_RADIUS, marker.x + MIN_HIT_RADIUS, marker.y + MIN_HIT_RADIUS));
    includeBounds(extents, marker.labelRegion);
  }
  for (const state of stateSlots) {
    includeBounds(extents, state.hitRegion);
    includeBounds(extents, state.labelRegion);
  }
  includeBounds(extents, filenameRegion);
  const ageDays = optionalNumber(anatomy?.ageDays);
  const age = ageAppearance(ageDays);
  const geometry: FileOrganismGeometry = {
    file,
    language: optionalString(anatomy?.language),
    sourceId: optionalString(anatomy?.sourceId),
    profileId: optionalString(anatomy?.profileId),
    status: String(anatomy?.status || 'unavailable').toLowerCase(),
    lineCount,
    callableCoverage: callables === null ? 'unknown' : 'known',
    stateCoverage: rawStateSlots === null ? 'unknown' : 'known',
    firstSeenAt: optionalString(anatomy?.firstSeenAt),
    ageAsOf: optionalString(anatomy?.ageAsOf),
    ageDays,
    ageReason: optionalString(anatomy?.ageReason),
    roleFact,
    interpretation: projectFileInterpretation(anatomy, input.assessment),
    head,
    body,
    appearance: Object.freeze({ ...age, bodyColor: blendAgeColor(ROLE_COLORS[roleFact], age.shellColor, age.ageKnown) }),
    limbs: Object.freeze(limbs),
    stateSlots: Object.freeze(stateSlots),
    stateNote: rawStateSlots === null ? 'Stored state unknown' : rawStateSlots.length ? null : 'No stored state slots',
    absenceMarkers: Object.freeze(absenceMarkers),
    rowCount,
    rowPitch,
    filenameRegion,
    bounds: makeBounds(extents.minX, extents.minY, extents.maxX, extents.maxY),
  };
  return freezeGeometry(geometry);
}

function appendBodyContour(path: Path2D, geometry: FileOrganismGeometry, x: number, y: number, scale: number): void {
  const left = (geometry.body.x * scale) + x;
  const top = (geometry.body.y * scale) + y;
  const width = geometry.body.width * scale;
  const height = geometry.body.height * scale;
  if (geometry.body.shape === 'angular') {
    const cutX = Math.min(width * 0.18, 18 * scale);
    const cutY = Math.min(height * 0.12, 18 * scale);
    path.moveTo(left + cutX, top);
    path.lineTo(left + width - cutX, top);
    path.lineTo(left + width, top + cutY);
    path.lineTo(left + width, top + height - cutY);
    path.lineTo(left + width - cutX, top + height);
    path.lineTo(left + cutX, top + height);
    path.lineTo(left, top + height - cutY);
    path.lineTo(left, top + cutY);
    path.closePath();
    return;
  }
  if (geometry.body.shape === 'leaf') {
    path.moveTo(left + width / 2, top);
    path.quadraticCurveTo(left + width * 1.5, top + height / 2, left + width / 2, top + height);
    path.quadraticCurveTo(left - width * 0.5, top + height / 2, left + width / 2, top);
    path.closePath();
    return;
  }
  if (geometry.body.shape === 'round') {
    const radius = Math.min(width, height) / 2;
    path.moveTo(left + radius, top);
    path.lineTo(left + width - radius, top);
    path.quadraticCurveTo(left + width, top, left + width, top + radius);
    path.lineTo(left + width, top + height - radius);
    path.quadraticCurveTo(left + width, top + height, left + width - radius, top + height);
    path.lineTo(left + radius, top + height);
    path.quadraticCurveTo(left, top + height, left, top + height - radius);
    path.lineTo(left, top + radius);
    path.quadraticCurveTo(left, top, left + radius, top);
    path.closePath();
    return;
  }
  path.ellipse(left + width / 2, top + height / 2, width / 2, height / 2, 0, 0, Math.PI * 2);
  path.closePath();
}

/** Create the actual body contour in projected coordinates for heat and selection overlays. */
export function createFileOrganismBodyPath(geometry: FileOrganismGeometry, transform: { x?: number; y?: number; scale?: number } = {}): Path2D {
  const x = Number.isFinite(transform.x) ? Number(transform.x) : 0;
  const y = Number.isFinite(transform.y) ? Number(transform.y) : 0;
  const scale = Number.isFinite(transform.scale) && Number(transform.scale) > 0 ? Number(transform.scale) : 1;
  const path = new Path2D();
  appendBodyContour(path, geometry, x, y, scale);
  return path;
}

function paintBodyPattern(ctx: CanvasRenderingContext2D, geometry: FileOrganismGeometry): void {
  if (geometry.body.shape !== 'segmented-oval') return;
  const { x, y, width, height } = geometry.body;
  ctx.save();
  ctx.beginPath();
  ctx.ellipse(x + width / 2, y + height / 2, width / 2, height / 2, 0, 0, Math.PI * 2);
  ctx.clip();
  ctx.strokeStyle = 'rgba(255, 255, 255, .28)';
  ctx.lineWidth = 1.2;
  for (let band = 1; band <= 3; band += 1) {
    const bandY = y + (height / 4) * band;
    ctx.beginPath();
    ctx.moveTo(x, bandY);
    ctx.lineTo(x + width, bandY);
    ctx.stroke();
  }
  ctx.restore();
}

function sideExtentAtY(body: FileOrganismGeometry['body'], y: number): number {
  const { x: left, y: top, width, height, shape } = body;
  const right = left + width;
  const bottom = top + height;
  const centerX = left + width / 2;
  const localY = y;
  if (shape === 'angular') {
    const cutY = Math.min(height * 0.12, 18);
    const cutX = Math.min(width * 0.18, 18);
    if (localY < top + cutY) return width / 2 - cutX * (1 - (localY - top) / cutY);
    if (localY > bottom - cutY) return width / 2 - cutX * (1 - (bottom - localY) / cutY);
    return width / 2;
  }
  if (shape === 'leaf') {
    const progress = Math.max(0, Math.min(1, (localY - top) / height));
    let low = 0;
    let high = 1;
    for (let step = 0; step < 24; step += 1) {
      const t = (low + high) / 2;
      const curveY = top * (1 - t) * (1 - t) + 2 * (1 - t) * t * (top + height / 2) + bottom * t * t;
      if ((curveY - top) / height < progress) low = t; else high = t;
    }
    const t = (low + high) / 2;
    const controlX = centerX + width;
    const curveX = centerX * (1 - t) * (1 - t) + 2 * (1 - t) * t * controlX + centerX * t * t;
    return curveX - centerX;
  }
  if (shape === 'round') {
    const radius = Math.min(width, height) / 2;
    const cornerCenterY = localY < top + radius ? top + radius : bottom - radius;
    if (localY >= top + radius && localY <= bottom - radius) return width / 2;
    return width / 2 - radius + Math.sqrt(Math.max(0, radius * radius - Math.pow(localY - cornerCenterY, 2)));
  }
  return width / 2 * Math.sqrt(Math.max(0, 1 - Math.pow((localY - top - height / 2) / (height / 2), 2)));
}

function limbColor(limb: FileOrganismLimb): string {
  return VISIBILITY_COLORS[limb.visibility || 'unknown'] || VISIBILITY_COLORS.unknown;
}

const limbPaintPlans = new WeakMap<FileOrganismGeometry, Map<number, readonly FileOrganismLimb[]>>();

/** Coalesce only subpixel rows, preserving each side/visibility and length/thickness extrema. */
export function getLimbPaintPlan(geometry: FileOrganismGeometry, scale: number): readonly FileOrganismLimb[] {
  const rowsPerBin = 2 ** Math.max(0, Math.ceil(Math.log2(1.25 / (geometry.rowPitch * Math.max(.000001, scale)))));
  if (rowsPerBin <= 1) return geometry.limbs;
  let cache = limbPaintPlans.get(geometry);
  if (!cache) { cache = new Map(); limbPaintPlans.set(geometry, cache); }
  const previous = cache.get(rowsPerBin);
  if (previous) return previous;
  const bins = new Map<string, FileOrganismLimb>();
  for (const limb of geometry.limbs) {
    const key = `${Math.floor(limb.row / rowsPerBin)}:${limb.side}:${limb.visibility}`;
    const other = bins.get(key);
    if (!other) { bins.set(key, limb); continue; }
    const longest = limb.length > other.length ? limb : other;
    bins.set(key, Object.freeze({ ...longest, thickness: Math.max(limb.thickness, other.thickness), terminalSize: Math.max(limb.terminalSize, other.terminalSize) }));
  }
  const plan = Object.freeze([...bins.values()]);
  if (cache.size >= 16) cache.delete(cache.keys().next().value!);
  cache.set(rowsPerBin, plan);
  return plan;
}

function paintHeadAndInterpretation(ctx: CanvasRenderingContext2D, geometry: FileOrganismGeometry): void {
  const { width, height, y } = geometry.head;
  const half = width / 2;
  ctx.save();
  ctx.fillStyle = geometry.appearance.bodyColor;
  ctx.strokeStyle = geometry.appearance.shellColor;
  ctx.lineWidth = 2;
  // A shield-shaped head, narrow neck and mandibles keep the silhouette recognizable at map scale.
  ctx.beginPath();
  ctx.moveTo(-half * .55, y);
  ctx.quadraticCurveTo(-half * 1.1, y + height * .3, -half * .8, y + height * .65);
  ctx.lineTo(-half * .32, 0);
  ctx.lineTo(half * .32, 0);
  ctx.lineTo(half * .8, y + height * .65);
  ctx.quadraticCurveTo(half * 1.1, y + height * .3, half * .55, y);
  ctx.closePath();
  ctx.fill(); ctx.stroke();
  for (const side of [-1, 1]) {
    ctx.beginPath();
    ctx.moveTo(side * half * .65, y + 12);
    ctx.lineTo(side * (half + 12), y - 18);
    ctx.lineTo(side * (half + 24), y - 52);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(side * half * .25, y + 2);
    ctx.lineTo(side * half * .34, y - 10);
    ctx.lineTo(side * half * .1, y - 6);
    ctx.stroke();
  }
  const traits = geometry.interpretation;
  if (traits) {
    // A continuous suture expresses a probability, never a discrete responsibility count.
    ctx.strokeStyle = '#f5cc83';
    ctx.lineWidth = 1 + traits.responsibilityMix * 3;
    ctx.setLineDash([3, 3]);
    ctx.beginPath();
    ctx.moveTo(0, y + 5);
    ctx.lineTo(-half * .25 * traits.responsibilityMix, y + height * .5);
    ctx.lineTo(0, -4);
    ctx.stroke();
    ctx.setLineDash([]);
    const inset = 4 + traits.informationHiding * 5;
    const shellHeight = Math.min(80, geometry.body.height * .75);
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(geometry.body.x + inset, 12);
    ctx.quadraticCurveTo(geometry.body.x + inset, shellHeight, 0, shellHeight);
    ctx.quadraticCurveTo(-geometry.body.x - inset, shellHeight, -geometry.body.x - inset, 12);
    ctx.stroke();
  }
  ctx.restore();
}

/** Paint exactly the immutable geometry; transform and selection never change metric dimensions. */
export function paintFileOrganism(ctx: CanvasRenderingContext2D, geometry: FileOrganismGeometry, options: FileOrganismPaintOptions = {}): Path2D {
  const x = Number.isFinite(options.x) ? Number(options.x) : 0;
  const y = Number.isFinite(options.y) ? Number(options.y) : 0;
  const scale = Number.isFinite(options.scale) && Number(options.scale) > 0 ? Number(options.scale) : 1;
  const selectionColor = options.selectionColor || '#fff0a6';
  const bodyPath = createFileOrganismBodyPath(geometry, { x, y, scale });
  ctx.save();
  ctx.translate(x, y);
  ctx.scale(scale, scale);
  ctx.fillStyle = options.bodyFill || geometry.appearance.bodyColor;
  ctx.strokeStyle = geometry.appearance.shellColor;
  ctx.lineWidth = 3;
  if (!geometry.appearance.ageKnown) ctx.setLineDash([5, 3]);
  ctx.beginPath();
  appendBodyContour(ctx as unknown as Path2D, geometry, 0, 0, 1);
  ctx.fill();
  ctx.stroke();
  ctx.setLineDash([]);
  paintBodyPattern(ctx, geometry);
  paintHeadAndInterpretation(ctx, geometry);

  if (geometry.appearance.shellBandCount > 0) {
    ctx.save();
    ctx.strokeStyle = geometry.appearance.shellColor;
    ctx.globalAlpha *= 0.9;
    ctx.lineWidth = 1.6;
    const top = geometry.body.y;
    for (let band = 1; band <= geometry.appearance.shellBandCount; band += 1) {
      const bandY = top + geometry.body.height * band / (geometry.appearance.shellBandCount + 1);
      const halfWidth = sideExtentAtY(geometry.body, bandY);
      ctx.beginPath();
      ctx.moveTo(-halfWidth, bandY);
      ctx.lineTo(halfWidth, bandY);
      ctx.stroke();
    }
    ctx.restore();
  }

  for (const limb of options.detail === 'screen' ? getLimbPaintPlan(geometry, scale) : geometry.limbs) {
    const color = limbColor(limb);
    ctx.beginPath();
    ctx.lineCap = 'round';
    ctx.lineWidth = limb.thickness;
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    ctx.setLineDash(limb.bodyLineSpan === null || limb.decisionCount === null ? [4, 3] : []);
    ctx.moveTo(limb.start.x, limb.start.y);
    ctx.lineTo(limb.end.x, limb.end.y);
    ctx.stroke();
    ctx.setLineDash([]);
    if (limb.terminal === 'square') {
      ctx.fillRect(limb.end.x - limb.terminalSize, limb.end.y - limb.terminalSize, limb.terminalSize * 2, limb.terminalSize * 2);
    } else {
      ctx.beginPath();
      ctx.arc(limb.end.x, limb.end.y, limb.terminalSize, 0, Math.PI * 2);
      if (limb.terminal === 'round') ctx.fill(); else ctx.stroke();
    }
    if (options.selectedCallableId === limb.id) {
      ctx.beginPath();
      ctx.lineWidth = 2;
      ctx.strokeStyle = selectionColor;
      const selectionRadius = Math.min(limb.terminalSize + 3, Math.max(limb.terminalSize, geometry.rowPitch / 2 - 2));
      ctx.arc(limb.end.x, limb.end.y, selectionRadius, 0, Math.PI * 2);
      ctx.stroke();
    }
    if (options.showCallableLabels || (options.showSelectedCallableLabel && options.selectedCallableId === limb.id)) {
      ctx.fillStyle = options.labelColor || '#f4f6fb';
      ctx.font = `${options.labelFontSize || 12}px monospace`;
      ctx.textAlign = limb.side < 0 ? 'right' : 'left';
      ctx.textBaseline = 'middle';
      ctx.fillText(limb.displayName, limb.end.x + limb.side * 8, limb.end.y);
    }
  }
  for (const marker of geometry.absenceMarkers) {
    ctx.beginPath();
    ctx.lineWidth = 2;
    ctx.strokeStyle = '#e7e9f1';
    ctx.setLineDash([3, 3]);
    ctx.arc(marker.x, marker.y, MIN_HIT_RADIUS, 0, Math.PI * 2);
    ctx.stroke();
    ctx.setLineDash([]);
  }
  for (const state of geometry.stateSlots) {
    ctx.beginPath();
    ctx.fillStyle = geometry.appearance.shellColor;
    ctx.strokeStyle = limbColor({ visibility: state.visibility } as FileOrganismLimb);
    ctx.lineWidth = 1.5;
    ctx.arc(state.x, state.y, state.hitRadius * 0.65, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
    if (options.selectedStateId === state.id) {
      ctx.beginPath();
      ctx.strokeStyle = options.selectionColor || '#fff0a6';
      ctx.lineWidth = 2;
      ctx.arc(state.x, state.y, state.hitRadius + 2, 0, Math.PI * 2);
      ctx.stroke();
    }
    if (options.showStateLabels) {
      ctx.fillStyle = options.labelColor || '#f4f6fb';
      ctx.font = `${options.labelFontSize || 12}px monospace`;
      ctx.textAlign = 'left';
      ctx.textBaseline = 'middle';
      ctx.fillText(state.name, state.x + state.hitRadius + 2, state.y);
    }
  }
  ctx.restore();
  return bodyPath;
}

/** Hit test full limb strokes/terminal regions and return the exact callable record. */
export function findFileOrganismLimbAt(geometry: FileOrganismGeometry, point: OrganismPoint): FileOrganismLimb | null {
  for (let index = geometry.limbs.length - 1; index >= 0; index -= 1) {
    const limb = geometry.limbs[index];
    const dx = limb.end.x - limb.start.x;
    const dy = limb.end.y - limb.start.y;
    const lengthSquared = dx * dx + dy * dy;
    const t = lengthSquared === 0 ? 0 : Math.max(0, Math.min(1, ((point.x - limb.start.x) * dx + (point.y - limb.start.y) * dy) / lengthSquared));
    const closest = { x: limb.start.x + t * dx, y: limb.start.y + t * dy };
    const radius = Math.max(MIN_HIT_RADIUS, limb.thickness / 2 + 2, terminalRadius(limb.thickness));
    if (Math.hypot(point.x - closest.x, point.y - closest.y) <= radius) return limb;
  }
  return null;
}

/** Hit test returns exact callable identity for map selection. */
export function hitTestFileOrganism(geometry: FileOrganismGeometry, point: OrganismPoint): string | null {
  return findFileOrganismLimbAt(geometry, point)?.id ?? null;
}

/** Hit test named state spots without treating them as callable limbs. */
export function hitTestFileOrganismState(geometry: FileOrganismGeometry, point: OrganismPoint): FileOrganismGeometry['stateSlots'][number] | null {
  for (let index = geometry.stateSlots.length - 1; index >= 0; index -= 1) {
    const state = geometry.stateSlots[index];
    if (Math.hypot(point.x - state.x, point.y - state.y) <= state.hitRadius) return state;
  }
  return null;
}


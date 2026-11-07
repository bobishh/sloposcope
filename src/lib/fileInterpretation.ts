export type FileInterpretation = Readonly<{
  assessmentId: string;
  model: string;
  role: string;
  responsibilityMix: number;
  informationHiding: number;
}>;

type Value = Record<string, unknown>;
const record = (value: unknown): Value | null => value != null && typeof value === 'object' ? value as Value : null;
const bounded = (value: unknown, max: number): value is number => typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= max;
const path = (value: unknown) => typeof value === 'string' ? value.replaceAll('\\', '/').replace(/^\.\//, '') : '';

/** Project estimates only when provenance identifies the exact organism source. */
export function projectFileInterpretation(anatomy: unknown, assessment: unknown): FileInterpretation | null {
  const source = record(anatomy);
  const evidence = record(assessment);
  const answers = record(evidence?.answers);
  const mix = record(answers?.multiple)?.noul;
  const hiding = record(answers?.hiding)?.score;
  const role = record(answers?.role)?.choice;
  if (!source?.sourceId || source.sourceId !== evidence?.source_id
    || !Array.isArray(evidence?.paths) || !evidence.paths.some(value => path(value) === path(source.file))
    || evidence.rubric !== 'file-traits-v1' || !evidence.id || !evidence.model
    || record(answers?.context)?.choice !== 'sufficient'
    || !bounded(mix, 1) || !bounded(hiding, 2)
    || typeof role !== 'string' || !['transformation', 'orchestration', 'boundary', 'representation', 'unknown'].includes(role)) return null;
  return Object.freeze({ assessmentId: String(evidence.id), model: String(evidence.model), role, responsibilityMix: mix, informationHiding: hiding });
}

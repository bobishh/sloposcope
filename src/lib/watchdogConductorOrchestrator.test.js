import { describe, expect, test } from 'vitest';

import {
  buildMissionInputFromForm,
  deriveMissionFormValues,
  getAgentProfileProviderHint,
} from './watchdogConductorOrchestrator.js';

describe('watchdogConductorOrchestrator mission helpers', () => {
  test('buildMissionInputFromForm keeps planner fields trimmed', () => {
    expect(buildMissionInputFromForm({
      title: '  Mission  ',
      brief: '  Viewer-first  ',
      validationCommand: '  npm run test:e2e  ',
      issueKey: '  LIN-7  ',
      issueUrl: '  https://linear.app/test  ',
      plannerProfileId: '  opencode-on-nim  ',
      plannerNotes: '  Tight cards only.  ',
      plannerPromptOverride: '  Plan terse.  ',
    })).toEqual({
      title: 'Mission',
      brief: 'Viewer-first',
      validationCommand: 'npm run test:e2e',
      issueKey: 'LIN-7',
      issueUrl: 'https://linear.app/test',
      plannerProfileId: 'opencode-on-nim',
      plannerNotes: 'Tight cards only.',
      plannerPromptOverride: 'Plan terse.',
    });
  });

  test('deriveMissionFormValues exposes planner fields', () => {
    const state = {
      title: 'Mission',
      brief: 'Brief',
      validationCommand: 'npm run test:e2e',
      issueKey: 'LIN-8',
      issueUrl: 'https://linear.app/test',
      plannerProfileId: 'opencode-on-nim',
      plannerNotes: 'Viewer-first.',
      plannerPromptOverride: 'Plan terse.',
    };
    const derived = deriveMissionFormValues(state, () => ({}));

    expect(derived.form.plannerProfileId).toBe('opencode-on-nim');
    expect(derived.form.plannerNotes).toBe('Viewer-first.');
    expect(derived.form.plannerPromptOverride).toBe('Plan terse.');
  });

  test('getAgentProfileProviderHint recognizes opencode on NIM', () => {
    const hint = getAgentProfileProviderHint('opencode_nim');

    expect(hint.title).toContain('OpenCode on NIM');
    expect(hint.summary).toContain('Paste key. Run horse.');
    expect(hint.envVars).toContain('NVIDIA_API_KEY');
  });
});

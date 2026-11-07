import { expect, it } from 'vitest';
import { createProjectSession, initializeProjectSelection } from './projectSessionModel';

it('Given a new repository and history, When initial selection resolves, Then the newest committed change is selected once', () => {
  const project = createProjectSession('/repo', 20);
  project.changes = [{ id: '@', description: '(working copy)' }, { id: 'latest', description: 'Latest commit' }, { id: 'older', description: 'Older commit' }];
  initializeProjectSelection(project);
  expect(project.since).toBe('latest');
  expect(project.initialSelectionPending).toBe(false);
  project.since = '@';
  initializeProjectSelection(project);
  expect(project.since).toBe('@');
});
it('Given explicit selection or empty history, When initialized, Then user intent and unborn repositories are preserved', () => {
  const project = createProjectSession('/repo', 20);
  project.since = 'selected'; project.changes = [{ id: 'latest', description: 'Commit' }];
  initializeProjectSelection(project); expect(project.since).toBe('selected');
  const empty = createProjectSession('/new', 20);
  initializeProjectSelection(empty); expect(empty.since).toBe('@');
});

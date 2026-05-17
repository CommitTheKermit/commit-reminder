import { describe, expect, it } from 'vitest';
import { changedFileCount, changedLineCount, isCooldownElapsed, shouldNotify } from './recommendation';
import type { RepositoryAnalysis } from '../types';

const base: RepositoryAnalysis = {
  repo: { name: 'demo', path: '/tmp/demo' },
  statusItems: [],
  changedFiles: 1,
  untrackedFiles: 2,
  additions: 10,
  deletions: 5,
  lastCommitIso: null,
  minutesSinceLastCommit: null,
  recommendation: { shouldRemind: false, severity: 'low', reasons: [] },
};

describe('recommendation helpers', () => {
  it('counts changed files and lines', () => {
    expect(changedFileCount(base)).toBe(3);
    expect(changedLineCount(base)).toBe(15);
  });

  it('notifies when either rules or AI recommend it', () => {
    expect(shouldNotify(base)).toBe(false);
    expect(shouldNotify({ ...base, recommendation: { shouldRemind: true, severity: 'medium', reasons: [] } })).toBe(true);
    expect(shouldNotify(base, { shouldRemind: true, confidence: 'high', summary: '기능 단위입니다.', commitMessageCandidates: [] })).toBe(true);
  });

  it('honors cooldown windows', () => {
    expect(isCooldownElapsed(undefined, 45, 1000)).toBe(true);
    expect(isCooldownElapsed(1000, 45, 1000 + 44 * 60_000)).toBe(false);
    expect(isCooldownElapsed(1000, 45, 1000 + 45 * 60_000)).toBe(true);
  });
});

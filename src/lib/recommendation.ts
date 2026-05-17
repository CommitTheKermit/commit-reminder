import type { AiJudgement, RepositoryAnalysis } from '../types';

export function changedLineCount(analysis: RepositoryAnalysis): number {
  return analysis.additions + analysis.deletions;
}

export function changedFileCount(analysis: RepositoryAnalysis): number {
  return analysis.changedFiles + analysis.untrackedFiles;
}

export function hasAnyChange(analysis: RepositoryAnalysis): boolean {
  return changedLineCount(analysis) > 0 || changedFileCount(analysis) > 0 || analysis.statusItems.length > 0;
}

export function shouldRunAi(analysis: RepositoryAnalysis): boolean {
  return hasAnyChange(analysis) && (analysis.recommendation.shouldRemind || changedLineCount(analysis) >= 20 || changedFileCount(analysis) >= 2);
}

export function shouldNotify(analysis: RepositoryAnalysis, ai?: AiJudgement): boolean {
  return analysis.recommendation.shouldRemind || Boolean(ai?.shouldRemind);
}

export function notificationBody(analysis: RepositoryAnalysis, ai?: AiJudgement): string {
  const lines = changedLineCount(analysis);
  const files = changedFileCount(analysis);
  if (ai?.summary) {
    const message = ai.commitMessageCandidates?.[0] ? ` 후보: ${ai.commitMessageCandidates[0]}` : '';
    return `${files} files, ${lines} lines. ${ai.summary}${message}`;
  }
  const reason = analysis.recommendation.reasons[0] ?? '변경사항이 커밋할 만한 크기입니다.';
  return `${files} files, +${analysis.additions}/-${analysis.deletions}. ${reason}`;
}

export function isCooldownElapsed(lastAlertAt: number | undefined, cooldownMinutes: number, now = Date.now()): boolean {
  if (!lastAlertAt) return true;
  return now - lastAlertAt >= cooldownMinutes * 60_000;
}

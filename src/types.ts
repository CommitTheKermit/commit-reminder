export interface ReminderRules {
  enabled: boolean;
  excludeUntrackedFiles: boolean;
  lineThreshold: number;
  fileThreshold: number;
  elapsedMinutesThreshold: number;
  cooldownMinutes: number;
  excludedPathPatterns: string[];
}

export interface AiConfig {
  enabled: boolean;
  provider: string;
  model: string;
  maxDiffChars: number;
}

export interface AppConfig {
  rootFolders: string[];
  excludedRepos: string[];
  scanIntervalSeconds: number;
  rules: ReminderRules;
  ai: AiConfig;
}

export interface RepositoryInfo {
  name: string;
  path: string;
}

export interface RuleRecommendation {
  shouldRemind: boolean;
  severity: string;
  reasons: string[];
}

export interface RepositoryAnalysis {
  repo: RepositoryInfo;
  statusItems: string[];
  changedFiles: number;
  untrackedFiles: number;
  additions: number;
  deletions: number;
  lastCommitIso?: string | null;
  minutesSinceLastCommit?: number | null;
  recommendation: RuleRecommendation;
}

export interface AiJudgement {
  shouldRemind: boolean;
  confidence: 'low' | 'medium' | 'high' | string;
  summary: string;
  commitMessageCandidates: string[];
  splitSuggestion?: string | null;
}

export interface ApiKeyStatus {
  provider: string;
  configured: boolean;
}

export interface RepoViewModel extends RepositoryAnalysis {
  aiJudgement?: AiJudgement;
  aiError?: string;
}

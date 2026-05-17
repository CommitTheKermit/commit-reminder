import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { isPermissionGranted, requestPermission } from '@tauri-apps/plugin-notification';
import type { AiJudgement, ApiKeyStatus, AppConfig, RepositoryAnalysis, RepoViewModel } from './types';
import { changedFileCount, changedLineCount, hasAnyChange, isCooldownElapsed, notificationBody, shouldNotify, shouldRunAi } from './lib/recommendation';
import './styles.css';

const DEFAULT_CONFIG: AppConfig = {
  rootFolders: [],
  excludedRepos: [],
  scanIntervalSeconds: 180,
  rules: {
    enabled: true,
    excludeUntrackedFiles: true,
    lineThreshold: 200,
    fileThreshold: 5,
    elapsedMinutesThreshold: 90,
    cooldownMinutes: 45,
    excludedPathPatterns: ['node_modules/', 'vendor/', 'dist/', 'build/', 'target/', '.dart_tool/', 'Pods/'],
  },
  ai: {
    enabled: true,
    provider: 'gemini',
    model: 'gemini-2.5-flash',
    maxDiffChars: 40_000,
  },
};

function App() {
  const [config, setConfig] = useState<AppConfig>(DEFAULT_CONFIG);
  const [repos, setRepos] = useState<RepoViewModel[]>([]);
  const [apiKeyStatus, setApiKeyStatus] = useState<ApiKeyStatus>({ provider: 'gemini', configured: false });
  const [apiKeyDraft, setApiKeyDraft] = useState('');
  const [message, setMessage] = useState('초기화 중...');
  const [notificationPermission, setNotificationPermission] = useState<NotificationPermission>('default');
  const [scanning, setScanning] = useState(false);
  const [autoScan, setAutoScan] = useState(true);
  const [lastScanAt, setLastScanAt] = useState<Date | null>(null);
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    void bootstrap();
  }, []);

  useEffect(() => {
    if (timerRef.current) window.clearInterval(timerRef.current);
    if (!autoScan) return;
    timerRef.current = window.setInterval(() => {
      void scanNow({ notify: true, requestNotificationPermission: false });
    }, Math.max(config.scanIntervalSeconds, 30) * 1000);
    return () => {
      if (timerRef.current) window.clearInterval(timerRef.current);
    };
  }, [autoScan, config]);

  const dirtyRepoCount = useMemo(() => repos.filter(hasAnyChange).length, [repos]);
  const recommendedCount = useMemo(() => repos.filter((repo) => shouldNotify(repo, repo.aiJudgement)).length, [repos]);

  useEffect(() => {
    void invoke('set_tray_commit_status', { recommendedCount, dirtyCount: dirtyRepoCount }).catch((error) => {
      console.warn('Failed to update tray commit status', error);
    });
  }, [dirtyRepoCount, recommendedCount]);

  async function bootstrap() {
    try {
      const loaded = await invoke<AppConfig>('get_config');
      const next = { ...DEFAULT_CONFIG, ...loaded, rules: { ...DEFAULT_CONFIG.rules, ...loaded.rules }, ai: { ...DEFAULT_CONFIG.ai, ...loaded.ai } };
      if (next.rootFolders.length === 0) {
        const suggested = await invoke<string | null>('suggest_default_root');
        if (suggested) next.rootFolders = [suggested];
      }
      setConfig(next);
      await refreshApiKeyStatus(next.ai.provider);
      await refreshNotificationPermission();
      setMessage('준비되었습니다.');
      setTimeout(() => void scanNow({ notify: false, overrideConfig: next }), 200);
    } catch (error) {
      setMessage(`초기화 실패: ${String(error)}`);
    }
  }

  async function refreshApiKeyStatus(provider = config.ai.provider) {
    const status = await invoke<ApiKeyStatus>('get_api_key_status', { provider });
    setApiKeyStatus(status);
  }

  async function saveConfig(next = config) {
    await invoke('save_config', { config: next });
    setConfig(next);
    setMessage('설정을 저장했습니다.');
  }

  async function addRootFolder() {
    const selected = await open({ directory: true, multiple: false, title: '감시할 상위 폴더 선택' });
    if (typeof selected !== 'string') return;
    const rootFolders = Array.from(new Set([...config.rootFolders, selected]));
    await saveConfig({ ...config, rootFolders });
  }

  async function removeRootFolder(path: string) {
    await saveConfig({ ...config, rootFolders: config.rootFolders.filter((root) => root !== path) });
  }

  async function saveApiKey() {
    if (!apiKeyDraft.trim()) {
      setMessage('API key를 입력해주세요.');
      return;
    }
    await invoke('set_api_key', { provider: config.ai.provider, apiKey: apiKeyDraft });
    setApiKeyDraft('');
    await refreshApiKeyStatus();
    setMessage('API key를 OS 키체인에 저장했습니다.');
  }

  async function refreshNotificationPermission() {
    try {
      const granted = await isPermissionGranted();
      const permission = granted ? 'granted' : window.Notification?.permission ?? 'default';
      setNotificationPermission(permission);
      return permission;
    } catch {
      setNotificationPermission('default');
      return 'default';
    }
  }

  async function ensureNotificationPermission(promptUser: boolean) {
    let granted = await isPermissionGranted();
    if (!granted) {
      if (!promptUser) {
        const permission = window.Notification?.permission ?? 'default';
        setNotificationPermission(permission);
        return false;
      }
      const permission = await requestPermission();
      setNotificationPermission(permission);
      granted = permission === 'granted';
    } else {
      setNotificationPermission('granted');
    }
    return granted;
  }

  async function sendTestNotification() {
    try {
      await ensureNotificationPermission(true);
      await invoke('send_native_notification', {
        title: 'Commit Reminder 테스트',
        body: '이 알림이 보이면 커밋 추천 알림도 표시됩니다.',
      });
      setMessage('테스트 알림을 보냈습니다.');
    } catch (error) {
      setMessage(`테스트 알림 실패: ${String(error)}`);
    }
  }

  function getLastAlertMap(): Record<string, number> {
    try {
      return JSON.parse(localStorage.getItem('commit-reminder:last-alerts') ?? '{}');
    } catch {
      return {};
    }
  }

  function setLastAlert(repoPath: string) {
    const alerts = getLastAlertMap();
    alerts[repoPath] = Date.now();
    localStorage.setItem('commit-reminder:last-alerts', JSON.stringify(alerts));
  }

  async function scanNow(options: { notify: boolean; overrideConfig?: AppConfig; requestNotificationPermission?: boolean } = { notify: true }) {
    const activeConfig = options.overrideConfig ?? config;
    if (activeConfig.rootFolders.length === 0) {
      setMessage('감시할 폴더를 먼저 추가해주세요.');
      return;
    }

    if (options.notify && options.requestNotificationPermission !== false) {
      await ensureNotificationPermission(true);
    }

    setScanning(true);
    setMessage('변경사항을 스캔하는 중...');
    try {
      const analyses = await invoke<RepositoryAnalysis[]>('analyze_repositories', { config: activeConfig });
      const viewModels: RepoViewModel[] = analyses;
      const canUseAi = activeConfig.ai.enabled && apiKeyStatus.configured;
      const aiTargets = canUseAi ? viewModels.filter(shouldRunAi).slice(0, 3) : [];

      for (const target of aiTargets) {
        try {
          target.aiJudgement = await invoke<AiJudgement>('ai_judge_repository', { repoPath: target.repo.path, config: activeConfig });
        } catch (error) {
          target.aiError = String(error);
        }
      }

      setRepos([...viewModels]);
      setLastScanAt(new Date());
      setMessage(`${viewModels.length}개 repo 스캔 완료 · 변경 있음 ${viewModels.filter(hasAnyChange).length}개`);

      if (options.notify) {
        await notifyIfNeeded(viewModels, activeConfig);
      }
    } catch (error) {
      setMessage(`스캔 실패: ${String(error)}`);
    } finally {
      setScanning(false);
    }
  }

  async function notifyIfNeeded(items: RepoViewModel[], activeConfig: AppConfig) {
    const candidates = items.filter((repo) => shouldNotify(repo, repo.aiJudgement));
    if (candidates.length === 0) return;
    const granted = await ensureNotificationPermission(false);

    const alerts = getLastAlertMap();
    let sent = 0;
    let skippedByCooldown = 0;
    for (const repo of candidates) {
      if (!isCooldownElapsed(alerts[repo.repo.path], activeConfig.rules.cooldownMinutes)) {
        skippedByCooldown += 1;
        continue;
      }
      try {
        await invoke('send_native_notification', {
          title: `커밋 추천: ${repo.repo.name}`,
          body: notificationBody(repo, repo.aiJudgement),
        });
        setLastAlert(repo.repo.path);
        sent += 1;
      } catch (error) {
        setMessage(`알림 전송 실패: ${String(error)}`);
      }
    }
    if (sent > 0) {
      setMessage(granted ? `커밋 추천 알림 ${sent}개를 보냈습니다.` : `커밋 추천 알림 ${sent}개를 보냈습니다. 보이지 않으면 macOS 알림 설정을 확인하세요.`);
    } else if (skippedByCooldown > 0) {
      setMessage(`커밋 추천 ${skippedByCooldown}개 발견 · 알림 쿨다운(${activeConfig.rules.cooldownMinutes}분) 때문에 생략했습니다.`);
    }
  }

  function updateRules<K extends keyof AppConfig['rules']>(key: K, value: AppConfig['rules'][K]) {
    setConfig({ ...config, rules: { ...config.rules, [key]: value } });
  }

  function updateAi<K extends keyof AppConfig['ai']>(key: K, value: AppConfig['ai'][K]) {
    setConfig({ ...config, ai: { ...config.ai, [key]: value } });
  }

  return (
    <main className="shell">
      <header className="hero">
        <div>
          <p className="eyebrow">Commit Reminder</p>
          <h1>커밋 타이밍을 놓치지 않게 도와주는 메뉴바 유틸리티</h1>
          <p className="subtle">폴더 단위로 Git repo를 찾고, 규칙 + Gemini 판단으로 커밋할 만한 변경을 알려줍니다.</p>
        </div>
        <div className="actions">
          <button onClick={() => void scanNow({ notify: true, requestNotificationPermission: true })} disabled={scanning}>{scanning ? '스캔 중...' : '지금 스캔'}</button>
          <button className="secondary" onClick={() => void sendTestNotification()}>알림 테스트</button>
          <button className="secondary" onClick={() => void saveConfig()}>설정 저장</button>
          <label className="toggle"><input type="checkbox" checked={autoScan} onChange={(e) => setAutoScan(e.target.checked)} /> 자동 스캔</label>
        </div>
      </header>

      <section className="stats">
        <Stat label="감시 루트" value={config.rootFolders.length} />
        <Stat label="발견 repo" value={repos.length} />
        <Stat label="변경 있음" value={dirtyRepoCount} />
        <Stat label="커밋 추천" value={recommendedCount} accent />
      </section>
      <p className={notificationPermission === 'granted' ? 'permission ok' : 'permission warn'}>
        알림 권한: {notificationPermission === 'granted' ? '허용됨' : notificationPermission === 'denied' ? '거부됨' : '아직 허용되지 않음'}
        {notificationPermission !== 'granted' && ' · 자동 알림을 받으려면 알림 테스트 버튼으로 권한을 허용하세요.'}
      </p>

      <section className="grid">
        <div className="card span-2">
          <div className="card-title">
            <h2>감시 폴더</h2>
            <button className="secondary" onClick={() => void addRootFolder()}>폴더 추가</button>
          </div>
          {config.rootFolders.length === 0 ? <p className="empty">감시할 상위 폴더를 추가하세요.</p> : (
            <ul className="path-list">
              {config.rootFolders.map((root) => <li key={root}><code>{root}</code><button className="ghost" onClick={() => void removeRootFolder(root)}>제거</button></li>)}
            </ul>
          )}
        </div>

        <div className="card">
          <h2>규칙 기준</h2>
          <label><span>변경 줄 수</span><input type="number" value={config.rules.lineThreshold} onChange={(e) => updateRules('lineThreshold', Number(e.target.value))} /></label>
          <label><span>변경 파일 수</span><input type="number" value={config.rules.fileThreshold} onChange={(e) => updateRules('fileThreshold', Number(e.target.value))} /></label>
          <label><span>마지막 커밋 후 분</span><input type="number" value={config.rules.elapsedMinutesThreshold} onChange={(e) => updateRules('elapsedMinutesThreshold', Number(e.target.value))} /></label>
          <label><span>알림 쿨다운 분</span><input type="number" value={config.rules.cooldownMinutes} onChange={(e) => updateRules('cooldownMinutes', Number(e.target.value))} /></label>
          <label><span>스캔 주기 초</span><input type="number" value={config.scanIntervalSeconds} onChange={(e) => setConfig({ ...config, scanIntervalSeconds: Number(e.target.value) })} /></label>
          <label className="toggle"><input type="checkbox" checked={config.rules.excludeUntrackedFiles} onChange={(e) => updateRules('excludeUntrackedFiles', e.target.checked)} /> untracked 파일 제외</label>
        </div>

        <div className="card">
          <h2>AI 설정</h2>
          <label className="toggle"><input type="checkbox" checked={config.ai.enabled} onChange={(e) => updateAi('enabled', e.target.checked)} /> Gemini 판단 사용</label>
          <label><span>Provider</span><input value={config.ai.provider} onChange={(e) => updateAi('provider', e.target.value)} /></label>
          <label><span>Model</span><input value={config.ai.model} onChange={(e) => updateAi('model', e.target.value)} /></label>
          <label><span>최대 diff 글자</span><input type="number" value={config.ai.maxDiffChars} onChange={(e) => updateAi('maxDiffChars', Number(e.target.value))} /></label>
          <div className="api-key-row">
            <input type="password" placeholder={apiKeyStatus.configured ? 'API key 저장됨' : 'Gemini API key'} value={apiKeyDraft} onChange={(e) => setApiKeyDraft(e.target.value)} />
            <button className="secondary" onClick={() => void saveApiKey()}>키 저장</button>
          </div>
          <p className={apiKeyStatus.configured ? 'ok' : 'warn'}>{apiKeyStatus.configured ? 'Gemini API key가 설정되어 있습니다.' : 'Gemini API key가 없으면 규칙 기반으로만 판단합니다.'}</p>
        </div>
      </section>

      <section className="card">
        <div className="card-title">
          <div>
            <h2>Repo 상태</h2>
            <p className="subtle">마지막 스캔: {lastScanAt ? lastScanAt.toLocaleString() : '아직 없음'}</p>
          </div>
          <p className="message">{message}</p>
        </div>
        <div className="repo-list">
          {repos.length === 0 ? <p className="empty">스캔 결과가 없습니다.</p> : repos.map((repo) => <RepoCard key={repo.repo.path} repo={repo} />)}
        </div>
      </section>
    </main>
  );
}

function Stat({ label, value, accent = false }: { label: string; value: number; accent?: boolean }) {
  return <div className={accent ? 'stat accent' : 'stat'}><span>{label}</span><strong>{value}</strong></div>;
}

function RepoCard({ repo }: { repo: RepoViewModel }) {
  const changed = hasAnyChange(repo);
  const remind = shouldNotify(repo, repo.aiJudgement);
  return (
    <article className={remind ? 'repo remind' : 'repo'}>
      <div className="repo-head">
        <div>
          <h3>{repo.repo.name}</h3>
          <code>{repo.repo.path}</code>
        </div>
        <span className={changed ? 'badge dirty' : 'badge'}>{changed ? 'changed' : 'clean'}</span>
      </div>
      <div className="repo-metrics">
        <span>{changedFileCount(repo)} files</span>
        <span>+{repo.additions} / -{repo.deletions}</span>
        <span>{changedLineCount(repo)} lines</span>
      </div>
      {repo.recommendation.reasons.length > 0 && <ul className="reasons">{repo.recommendation.reasons.map((reason) => <li key={reason}>{reason}</li>)}</ul>}
      {repo.aiJudgement && <div className="ai-box"><strong>AI:</strong> {repo.aiJudgement.summary}{repo.aiJudgement.commitMessageCandidates[0] && <code>{repo.aiJudgement.commitMessageCandidates[0]}</code>}{repo.aiJudgement.splitSuggestion && <p>{repo.aiJudgement.splitSuggestion}</p>}</div>}
      {repo.aiError && <p className="warn">AI 오류: {repo.aiError}</p>}
    </article>
  );
}

createRoot(document.getElementById('root')!).render(<App />);

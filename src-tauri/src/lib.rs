use chrono::{DateTime, Utc};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::{Path, PathBuf}, process::Command};
use tauri::{Manager, AppHandle, menu::{Menu, MenuItem}, tray::TrayIconBuilder};
use tauri_plugin_notification::NotificationExt;

const APP_CONFIG_DIR: &str = "commit-reminder";
const KEYRING_SERVICE: &str = "com.commitreminder.desktop";
const GEMINI_PROVIDER: &str = "gemini";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub root_folders: Vec<String>,
    pub excluded_repos: Vec<String>,
    pub scan_interval_seconds: u64,
    pub rules: ReminderRules,
    pub ai: AiConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            root_folders: vec![],
            excluded_repos: vec![],
            scan_interval_seconds: 180,
            rules: ReminderRules::default(),
            ai: AiConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderRules {
    pub enabled: bool,
    #[serde(default = "default_exclude_untracked_files")]
    pub exclude_untracked_files: bool,
    pub line_threshold: u64,
    pub file_threshold: u64,
    pub elapsed_minutes_threshold: u64,
    pub cooldown_minutes: u64,
    pub excluded_path_patterns: Vec<String>,
}

fn default_exclude_untracked_files() -> bool {
    true
}

impl Default for ReminderRules {
    fn default() -> Self {
        Self {
            enabled: true,
            exclude_untracked_files: true,
            line_threshold: 200,
            file_threshold: 5,
            elapsed_minutes_threshold: 90,
            cooldown_minutes: 45,
            excluded_path_patterns: vec![
                "node_modules/".into(),
                "vendor/".into(),
                "dist/".into(),
                "build/".into(),
                "target/".into(),
                ".dart_tool/".into(),
                "Pods/".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub max_diff_chars: usize,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: GEMINI_PROVIDER.into(),
            model: "gemini-2.5-flash".into(),
            max_diff_chars: 40_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInfo {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryAnalysis {
    pub repo: RepositoryInfo,
    pub status_items: Vec<String>,
    pub changed_files: u64,
    pub untracked_files: u64,
    pub additions: u64,
    pub deletions: u64,
    pub last_commit_iso: Option<String>,
    pub minutes_since_last_commit: Option<i64>,
    pub recommendation: RuleRecommendation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleRecommendation {
    pub should_remind: bool,
    pub severity: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiJudgement {
    pub should_remind: bool,
    pub confidence: String,
    pub summary: String,
    pub commit_message_candidates: Vec<String>,
    pub split_suggestion: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyStatus {
    pub provider: String,
    pub configured: bool,
}

#[tauri::command]
fn get_config() -> Result<AppConfig, String> {
    load_config()
}

#[tauri::command]
fn save_config(config: AppConfig) -> Result<(), String> {
    write_config(&config)
}

#[tauri::command]
fn suggest_default_root() -> Result<Option<String>, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let candidate = if cwd.file_name().and_then(|n| n.to_str()) == Some("commit-reminder") {
        cwd.parent().map(Path::to_path_buf).unwrap_or(cwd)
    } else {
        cwd
    };
    if candidate.exists() {
        Ok(Some(candidate.to_string_lossy().to_string()))
    } else {
        Ok(None)
    }
}

#[tauri::command]
fn discover_repositories(root_folders: Vec<String>, excluded_repos: Vec<String>) -> Result<Vec<RepositoryInfo>, String> {
    let excluded: HashSet<String> = excluded_repos.into_iter().collect();
    let mut repos = Vec::new();
    let mut seen = HashSet::new();

    for root in root_folders {
        let root_path = PathBuf::from(root);
        if !root_path.exists() {
            continue;
        }
        discover_git_repos(&root_path, &excluded, &mut seen, &mut repos, 4)?;
    }

    repos.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    Ok(repos)
}

#[tauri::command]
fn analyze_repository(repo_path: String, config: AppConfig) -> Result<RepositoryAnalysis, String> {
    analyze_repo(Path::new(&repo_path), &config)
}

#[tauri::command]
fn analyze_repositories(config: AppConfig) -> Result<Vec<RepositoryAnalysis>, String> {
    let repos = discover_repositories(config.root_folders.clone(), config.excluded_repos.clone())?;
    repos.into_iter()
        .map(|repo| analyze_repo(Path::new(&repo.path), &config))
        .collect()
}

#[tauri::command]
fn get_api_key_status(provider: String) -> Result<ApiKeyStatus, String> {
    Ok(ApiKeyStatus {
        provider: provider.clone(),
        configured: read_api_key(&provider).is_ok(),
    })
}

#[tauri::command]
fn set_api_key(provider: String, api_key: String) -> Result<(), String> {
    if api_key.trim().is_empty() {
        return Err("API key is empty".into());
    }
    let entry = Entry::new(KEYRING_SERVICE, &provider).map_err(|e| e.to_string())?;
    entry.set_password(api_key.trim()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn ai_judge_repository(repo_path: String, config: AppConfig) -> Result<AiJudgement, String> {
    if !config.ai.enabled {
        return Err("AI analysis is disabled".into());
    }
    if config.ai.provider != GEMINI_PROVIDER {
        return Err(format!("Unsupported provider: {}", config.ai.provider));
    }

    let repo = Path::new(&repo_path);
    let analysis = analyze_repo(repo, &config)?;
    let diff_context = build_sanitized_diff_context(repo, &config, &analysis)?;
    if diff_context.trim().is_empty() {
        return Ok(AiJudgement {
            should_remind: false,
            confidence: "low".into(),
            summary: "분석할 안전한 diff가 없습니다.".into(),
            commit_message_candidates: vec![],
            split_suggestion: None,
        });
    }

    let api_key = read_api_key(GEMINI_PROVIDER)?;
    call_gemini(&api_key, &config.ai.model, &diff_context).await
}

#[tauri::command]
fn send_native_notification(app: AppHandle, title: String, body: String) -> Result<(), String> {
    app
        .notification()
        .builder()
        .title(title.clone())
        .body(body.clone())
        .sound("Ping")
        .show()
        .or_else(|notification_error| {
            #[cfg(target_os = "macos")]
            {
                let script = format!(
                    "display notification {} with title {} sound name \"Ping\"",
                    osascript_string(&body),
                    osascript_string(&title)
                );
                let output = Command::new("osascript")
                    .arg("-e")
                    .arg(script)
                    .output()
                    .map_err(|e| format!("native notification failed: {notification_error}; osascript failed: {e}"))?;
                if output.status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "native notification failed: {notification_error}; osascript failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ))
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(notification_error.to_string())
            }
        })
}

#[cfg(target_os = "macos")]
fn osascript_string(value: &str) -> String {
    format!("{:?}", value)
}

fn load_config() -> Result<AppConfig, String> {
    let path = config_file_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

fn write_config(config: &AppConfig) -> Result<(), String> {
    let path = config_file_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| e.to_string())
}

fn config_file_path() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or_else(|| "Could not locate config directory".to_string())?;
    Ok(base.join(APP_CONFIG_DIR).join("config.json"))
}

fn read_api_key(provider: &str) -> Result<String, String> {
    if provider == GEMINI_PROVIDER {
        if let Ok(value) = std::env::var("GEMINI_API_KEY").or_else(|_| std::env::var("GOOGLE_API_KEY")) {
            if !value.trim().is_empty() {
                return Ok(value);
            }
        }
    }
    let entry = Entry::new(KEYRING_SERVICE, provider).map_err(|e| e.to_string())?;
    entry.get_password().map_err(|_| format!("No API key configured for {provider}"))
}

fn discover_git_repos(
    dir: &Path,
    excluded: &HashSet<String>,
    seen: &mut HashSet<String>,
    repos: &mut Vec<RepositoryInfo>,
    depth_left: usize,
) -> Result<(), String> {
    if depth_left == 0 || should_skip_dir(dir) {
        return Ok(());
    }
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let canonical_str = canonical.to_string_lossy().to_string();
    if excluded.contains(&canonical_str) {
        return Ok(());
    }

    if dir.join(".git").exists() {
        if seen.insert(canonical_str.clone()) {
            repos.push(RepositoryInfo {
                name: dir.file_name().and_then(|n| n.to_str()).unwrap_or("repo").to_string(),
                path: canonical_str,
            });
        }
        return Ok(());
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_git_repos(&path, excluded, seen, repos, depth_left - 1)?;
        }
    }
    Ok(())
}

fn should_skip_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("node_modules" | "vendor" | "dist" | "build" | "target" | ".dart_tool" | "Pods" | ".omx")
    )
}

fn analyze_repo(repo: &Path, config: &AppConfig) -> Result<RepositoryAnalysis, String> {
    if !repo.join(".git").exists() {
        return Err(format!("Not a Git repository: {}", repo.display()));
    }

    let status = git(repo, &["status", "--short"])?;
    let status_items: Vec<String> = status
        .lines()
        .filter(|line| !(config.rules.exclude_untracked_files && line.starts_with("?? ")))
        .map(|line| line.to_string())
        .collect();
    let has_head = git(repo, &["rev-parse", "--verify", "HEAD"]).is_ok();
    let diff_args = if has_head {
        vec!["diff", "--numstat", "HEAD", "--"]
    } else {
        vec!["diff", "--numstat", "--cached", "--"]
    };
    let numstat = git(repo, &diff_args).unwrap_or_default();

    let mut additions = 0_u64;
    let mut deletions = 0_u64;
    let mut changed_files = 0_u64;
    for line in numstat.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 || should_exclude_path(parts[2], &config.rules) {
            continue;
        }
        changed_files += 1;
        additions += parts[0].parse::<u64>().unwrap_or(0);
        deletions += parts[1].parse::<u64>().unwrap_or(0);
    }

    let mut untracked_files = 0_u64;
    if !config.rules.exclude_untracked_files {
        for line in &status_items {
            if let Some(path) = line.strip_prefix("?? ") {
                if !should_exclude_path(path, &config.rules) {
                    untracked_files += 1;
                }
            }
        }
    }

    let (last_commit_iso, minutes_since_last_commit) = if has_head {
        let iso = git(repo, &["log", "-1", "--format=%cI"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let minutes = iso.as_ref().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|dt| {
            Utc::now().signed_duration_since(dt.with_timezone(&Utc)).num_minutes()
        });
        (iso, minutes)
    } else {
        (None, None)
    };

    let repo_info = RepositoryInfo {
        name: repo.file_name().and_then(|n| n.to_str()).unwrap_or("repo").to_string(),
        path: repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf()).to_string_lossy().to_string(),
    };
    let recommendation = recommend_by_rules(additions, deletions, changed_files, untracked_files, minutes_since_last_commit, &config.rules);

    Ok(RepositoryAnalysis {
        repo: repo_info,
        status_items,
        changed_files,
        untracked_files,
        additions,
        deletions,
        last_commit_iso,
        minutes_since_last_commit,
        recommendation,
    })
}

fn recommend_by_rules(
    additions: u64,
    deletions: u64,
    changed_files: u64,
    untracked_files: u64,
    minutes_since_last_commit: Option<i64>,
    rules: &ReminderRules,
) -> RuleRecommendation {
    if !rules.enabled {
        return RuleRecommendation { should_remind: false, severity: "off".into(), reasons: vec![] };
    }

    let total_lines = additions + deletions;
    let total_files = changed_files + untracked_files;
    let mut reasons = vec![];
    if total_lines >= rules.line_threshold {
        reasons.push(format!("변경 줄 수가 기준({}줄)을 넘었습니다: {}줄", rules.line_threshold, total_lines));
    }
    if total_files >= rules.file_threshold {
        reasons.push(format!("변경 파일 수가 기준({}개)을 넘었습니다: {}개", rules.file_threshold, total_files));
    }
    if let Some(minutes) = minutes_since_last_commit {
        if minutes >= rules.elapsed_minutes_threshold as i64 && total_files > 0 {
            reasons.push(format!("마지막 커밋 이후 {}분이 지났습니다", minutes));
        }
    }

    let severity = if reasons.len() >= 2 || total_lines >= rules.line_threshold.saturating_mul(2) {
        "high"
    } else if !reasons.is_empty() {
        "medium"
    } else {
        "low"
    };

    RuleRecommendation { should_remind: !reasons.is_empty(), severity: severity.into(), reasons }
}

fn build_sanitized_diff_context(repo: &Path, config: &AppConfig, analysis: &RepositoryAnalysis) -> Result<String, String> {
    let has_head = git(repo, &["rev-parse", "--verify", "HEAD"]).is_ok();
    if !has_head {
        return Ok(format!(
            "Repository: {}\nStatus:\n{}\n\nInitial repository without HEAD. Use rule-based data only: +{} -{}, changed files {}, untracked {}.",
            analysis.repo.name,
            analysis.status_items.join("\n"),
            analysis.additions,
            analysis.deletions,
            analysis.changed_files,
            analysis.untracked_files
        ));
    }

    let names = git(repo, &["diff", "--name-only", "HEAD", "--"])?;
    let safe_paths: Vec<String> = names
        .lines()
        .filter(|p| !should_exclude_path(p, &config.rules))
        .take(80)
        .map(|s| s.to_string())
        .collect();

    let mut context = format!(
        "Repository: {}\nChanged: +{} -{}, files {}, untracked {}\nStatus:\n{}\n\n",
        analysis.repo.name,
        analysis.additions,
        analysis.deletions,
        analysis.changed_files,
        analysis.untracked_files,
        analysis.status_items.join("\n")
    );

    if safe_paths.is_empty() {
        return Ok(context);
    }

    let mut args = vec!["diff".to_string(), "--no-ext-diff".into(), "--unified=80".into(), "HEAD".into(), "--".into()];
    args.extend(safe_paths);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let diff = git(repo, &arg_refs).unwrap_or_default();
    context.push_str(&diff);

    if context.len() > config.ai.max_diff_chars {
        context.truncate(config.ai.max_diff_chars);
        context.push_str("\n\n[Diff truncated by Commit Reminder safety limit]");
    }
    Ok(context)
}

fn should_exclude_path(path: &str, rules: &ReminderRules) -> bool {
    let lower = path.to_lowercase();
    let sensitive_name = lower.contains(".env")
        || lower.contains("secret")
        || lower.contains("private")
        || lower.contains("credential")
        || lower.contains("apikey")
        || lower.contains("api_key")
        || lower.contains("certificate")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".p12")
        || lower.ends_with(".mobileprovision")
        || lower.ends_with("package-lock.json")
        || lower.ends_with("pnpm-lock.yaml")
        || lower.ends_with("yarn.lock")
        || lower.ends_with("pubspec.lock")
        || lower.ends_with("cargo.lock");
    if sensitive_name {
        return true;
    }
    rules.excluded_path_patterns.iter().any(|pattern| !pattern.is_empty() && lower.contains(&pattern.to_lowercase()))
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

async fn call_gemini(api_key: &str, model: &str, diff_context: &str) -> Result<AiJudgement, String> {
    let prompt = format!(
        r#"You are a commit hygiene assistant. Analyze this sanitized git diff context and decide whether the developer should commit now.
Return ONLY compact JSON with this exact shape:
{{"shouldRemind": boolean, "confidence": "low"|"medium"|"high", "summary": "Korean one-sentence reason", "commitMessageCandidates": ["Korean conventional commit message"], "splitSuggestion": string|null}}
Prefer reminding when the diff looks like one coherent feature/fix/refactor, or when it has grown enough that a commit checkpoint is useful. If changes should be split, still remind and explain splitSuggestion.

DIFF_CONTEXT:
{}"#,
        diff_context
    );

    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}", model, api_key);
    let body = serde_json::json!({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {
            "temperature": 0.2,
            "responseMimeType": "application/json"
        }
    });

    let client = reqwest::Client::new();
    let value: serde_json::Value = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let text = value["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .ok_or_else(|| format!("Unexpected Gemini response: {value}"))?;
    let cleaned = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    serde_json::from_str(cleaned).map_err(|e| format!("Could not parse AI judgement: {e}. Raw: {cleaned}"))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            suggest_default_root,
            discover_repositories,
            analyze_repository,
            analyze_repositories,
            get_api_key_status,
            set_api_key,
            ai_judge_repository,
            send_native_notification,
        ])
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "Show Commit Reminder", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray_builder = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(true)
                .tooltip("Commit Reminder")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }
            let _tray = tray_builder.build(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

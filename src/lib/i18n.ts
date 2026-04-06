export type Locale = "en" | "zh-CN";

type MessageKey =
  | "appTitle"
  | "currentSession"
  | "currentQuota"
  | "login"
  | "openFolder"
  | "checkingRuntime"
  | "runtimeRunning"
  | "runtimeStopped"
  | "addProfiles"
  | "openCodex"
  | "contactUs"
  | "previous"
  | "next"
  | "addProfileTitle"
  | "addProfileCopy"
  | "folderName"
  | "cancel"
  | "create"
  | "folderNameRequired"
  | "profileMetadataMissing"
  | "subscriptionDaysLeft"
  | "noActiveProfile"
  | "switchToStart"
  | "quotaWillAppear"
  | "fiveHourAllowance"
  | "weeklyAllowance"
  | "refresh"
  | "profilesEmpty"
  | "profileSwitchDisabled"
  | "profileSwitchReady"
  | "switch"
  | "switchedTo"
  | "failedToSwitchProfile"
  | "openedProfileFolder"
  | "failedToOpenProfileFolder"
  | "openedCodex"
  | "failedToOpenCodex"
  | "loggedIn"
  | "failedToLogin"
  | "openedRepository"
  | "failedToOpenRepository"
  | "createdProfile"
  | "failedToCreateProfile"
  | "subscriptionFallback"
  | "languageEnglish"
  | "languageChinese";

type Messages = Record<MessageKey, string>;

const messages: Record<Locale, Messages> = {
  en: {
    appTitle: "Codex Switch",
    currentSession: "Current session",
    currentQuota: "Current quota",
    login: "Login",
    openFolder: "Open folder",
    checkingRuntime: "Checking runtime",
    runtimeRunning: "Codex running",
    runtimeStopped: "Codex not running",
    addProfiles: "Add Profiles",
    openCodex: "Open Codex",
    contactUs: "Contact Us",
    previous: "Previous",
    next: "Next",
    addProfileTitle: "Add Profile",
    addProfileCopy: "Create a new backup folder with template auth.json and profile.json.",
    folderName: "Folder name",
    cancel: "Cancel",
    create: "Create",
    folderNameRequired: "Folder name is required.",
    profileMetadataMissing: "Profile metadata not configured",
    subscriptionDaysLeft: "{plan} • {days} days left",
    noActiveProfile: "No active profile",
    switchToStart: "Switch a profile to start",
    quotaWillAppear: "Quota data will appear after a profile is configured.",
    fiveHourAllowance: "5h allowance",
    weeklyAllowance: "Weekly allowance",
    refresh: "Refresh {value}",
    profilesEmpty: "No profiles found. Use Add Profiles to create the first backup folder.",
    profileSwitchDisabled: "Profile is not switchable",
    profileSwitchReady: "Switch to this profile",
    switch: "Switch",
    switchedTo: "Switched to {profile}",
    failedToSwitchProfile: "Failed to switch profile.",
    openedProfileFolder: "Opened profile folder",
    failedToOpenProfileFolder: "Failed to open profile folder.",
    openedCodex: "Opened Codex",
    failedToOpenCodex: "Failed to open Codex.",
    loggedIn: "Logged in {profile}",
    failedToLogin: "Failed to log in current profile.",
    openedRepository: "Opened repository",
    failedToOpenRepository: "Failed to open repository.",
    createdProfile: "Created profile {profile}",
    failedToCreateProfile: "Failed to create profile.",
    subscriptionFallback: "Subscription • {days} days left",
    languageEnglish: "EN",
    languageChinese: "中文",
  },
  "zh-CN": {
    appTitle: "Codex Switch",
    currentSession: "当前账号",
    currentQuota: "当前额度",
    login: "登录",
    openFolder: "打开文件夹",
    checkingRuntime: "检查运行状态中",
    runtimeRunning: "Codex 运行中",
    runtimeStopped: "Codex 未运行",
    addProfiles: "添加账号",
    openCodex: "打开 Codex",
    contactUs: "联系我们",
    previous: "上一页",
    next: "下一页",
    addProfileTitle: "添加账号",
    addProfileCopy: "创建新的备份文件夹，并写入 auth.json 与 profile.json 模板。",
    folderName: "文件夹名",
    cancel: "取消",
    create: "创建",
    folderNameRequired: "请输入文件夹名。",
    profileMetadataMissing: "未配置账号元数据",
    subscriptionDaysLeft: "{plan} • 剩余 {days} 天",
    noActiveProfile: "暂无当前账号",
    switchToStart: "请先切换到一个账号",
    quotaWillAppear: "配置账号后，这里会显示额度信息。",
    fiveHourAllowance: "5小时额度",
    weeklyAllowance: "周额度",
    refresh: "刷新时间 {value}",
    profilesEmpty: "当前没有账号备份，请先点击“添加账号”创建第一个文件夹。",
    profileSwitchDisabled: "当前账号不可切换",
    profileSwitchReady: "切换到该账号",
    switch: "切换",
    switchedTo: "已切换到 {profile}",
    failedToSwitchProfile: "切换账号失败。",
    openedProfileFolder: "已打开账号文件夹",
    failedToOpenProfileFolder: "打开账号文件夹失败。",
    openedCodex: "已打开 Codex",
    failedToOpenCodex: "打开 Codex 失败。",
    loggedIn: "{profile} 登录完成",
    failedToLogin: "当前账号登录失败。",
    openedRepository: "已打开仓库地址",
    failedToOpenRepository: "打开仓库地址失败。",
    createdProfile: "已创建账号 {profile}",
    failedToCreateProfile: "创建账号失败。",
    subscriptionFallback: "订阅 • 剩余 {days} 天",
    languageEnglish: "EN",
    languageChinese: "中文",
  },
};

const STORAGE_KEY = "codex-switch-locale";

export function resolveInitialLocale(): Locale {
  const stored = globalThis.localStorage?.getItem(STORAGE_KEY);
  if (stored === "en" || stored === "zh-CN") {
    return stored;
  }

  const language = globalThis.navigator?.language?.toLowerCase() ?? "";
  return language.startsWith("zh") ? "zh-CN" : "en";
}

export function persistLocale(locale: Locale): void {
  globalThis.localStorage?.setItem(STORAGE_KEY, locale);
}

export function t(locale: Locale, key: MessageKey, variables?: Record<string, string | number>): string {
  let message = messages[locale][key];
  if (!variables) {
    return message;
  }

  for (const [name, value] of Object.entries(variables)) {
    message = message.split(`{${name}}`).join(String(value));
  }

  return message;
}

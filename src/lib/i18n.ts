export type Locale = "en" | "zh-CN";

const enMessages = {
  appTitle: "Codex Switch",
  currentSession: "Current session",
  currentQuota: "Current quota",
  login: "Login",
  openFolder: "Open folder",
  addProfiles: "Add Profiles",
  openCodex: "Open Codex",
  contactUs: "Contact Us",
  previous: "Previous",
  next: "Next",
  addProfileTitle: "Add Profile",
  addProfileCopy: "Create a new backup folder with template auth.json and profile.json.",
  renameProfileTitle: "Rename Profile",
  renameProfileCopy: "Enter a new folder name for this profile.",
  baseUrlTitle: "Custom Base Url",
  baseUrlCopy: "Must start with http:// or https://. Only fill this for API KEY logins, otherwise Codex will fail.",
  folderName: "Folder name",
  baseUrlLabel: "Base Url",
  baseUrlPlaceholder: "https://example.com/v1",
  cancel: "Cancel",
  create: "Create",
  rename: "Rename",
  save: "Save",
  folderNameRequired: "Folder name is required.",
  profileRenameDisabled: "The active profile cannot be renamed",
  profileRenameReady: "Rename this profile folder",
  profileBaseReady: "Set a custom Base Url for this profile",
  profileMetadataMissing: "Profile metadata not configured",
  subscriptionDaysLeft: "{plan} • {days} days left",
  noActiveProfile: "No active profile",
  switchToStart: "Switch a profile to start",
  quotaWillAppear: "Quota data will appear after a profile is configured.",
  fiveHourAllowance: "5h allowance",
  weeklyAllowance: "Weekly allowance",
  refresh: "Refresh {value}",
  refreshButton: "Refresh",
  baseButton: "Base",
  profilesEmpty: "No profiles found. Use Add Profiles to create the first backup folder.",
  profileRefreshDisabled: "Profile auth is not refreshable",
  profileRefreshReady: "Refresh this profile auth",
  profileRefreshQueued: "Queued for auth refresh",
  profileRefreshRunning: "Refreshing profile auth",
  profileSwitchDisabled: "Profile is not switchable",
  profileSwitchReady: "Switch to this profile",
  switch: "Switch",
  renamedProfile: "Renamed {from} to {to}",
  failedToRenameProfile: "Failed to rename profile.",
  savedBaseUrl: "Saved Base Url for {profile}",
  clearedBaseUrl: "Cleared Base Url for {profile}",
  failedToSaveBaseUrl: "Failed to save Base Url.",
  refreshedProfile: "Refreshed {profile}",
  failedToRefreshProfile: "Failed to refresh profile.",
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
} as const;

type MessageKey = keyof typeof enMessages;
type Messages = Record<MessageKey, string>;

const messages: Record<Locale, Messages> = {
  en: enMessages,
  "zh-CN": {
    appTitle: "Codex Switch",
    currentSession: "当前账号",
    currentQuota: "当前额度",
    login: "登录",
    openFolder: "打开文件夹",
    addProfiles: "添加账号",
    openCodex: "打开 Codex",
    contactUs: "联系我们",
    previous: "上一页",
    next: "下一页",
    addProfileTitle: "添加账号",
    addProfileCopy: "创建新的备份文件夹，并写入 auth.json 与 profile.json 模板。",
    renameProfileTitle: "重命名账号",
    renameProfileCopy: "请输入该账号新的文件夹名称。",
    baseUrlTitle: "请输入自定义 Base Url",
    baseUrlCopy: "以http://或https://开头，仅在API KEY登录时填写，否则会出错。",
    folderName: "文件夹名",
    baseUrlLabel: "Base Url",
    baseUrlPlaceholder: "https://example.com/v1",
    cancel: "取消",
    create: "创建",
    rename: "重命名",
    save: "保存",
    folderNameRequired: "请输入文件夹名。",
    profileRenameDisabled: "当前正在使用的账号不可重命名",
    profileRenameReady: "重命名该账号文件夹",
    profileBaseReady: "设置该账号的 Base Url",
    profileMetadataMissing: "未配置账号元数据",
    subscriptionDaysLeft: "{plan} • 剩余 {days} 天",
    noActiveProfile: "暂无当前账号",
    switchToStart: "请先切换到一个账号",
    quotaWillAppear: "配置账号后，这里会显示额度信息。",
    fiveHourAllowance: "5小时额度",
    weeklyAllowance: "周额度",
    refresh: "刷新时间 {value}",
    refreshButton: "刷新",
    baseButton: "Base",
    profilesEmpty: "当前没有账号备份，请先点击“添加账号”创建第一个文件夹。",
    profileRefreshDisabled: "当前账号授权不可刷新",
    profileRefreshReady: "刷新该账号授权",
    profileRefreshQueued: "已加入授权刷新队列",
    profileRefreshRunning: "正在刷新该账号授权",
    profileSwitchDisabled: "当前账号不可切换",
    profileSwitchReady: "切换到该账号",
    switch: "切换",
    renamedProfile: "已将 {from} 重命名为 {to}",
    failedToRenameProfile: "重命名账号失败。",
    savedBaseUrl: "已保存 {profile} 的 Base Url",
    clearedBaseUrl: "已清除 {profile} 的 Base Url",
    failedToSaveBaseUrl: "保存 Base Url 失败。",
    refreshedProfile: "已刷新 {profile}",
    failedToRefreshProfile: "刷新账号失败。",
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

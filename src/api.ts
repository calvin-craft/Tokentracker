import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

export type ProviderState = "ok" | "needs_login" | "error" | "loading";

export type ProviderKind = "codex" | "claude" | "gemini" | "antigravity" | "grok";

export const PROVIDER_NAMES: Record<ProviderKind, string> = {
  codex: "Codex",
  claude: "Claude",
  gemini: "Gemini",
  antigravity: "Antigravity",
  grok: "Grok",
};

/**
 * One tracked account. The built-in account of a provider has `id === provider`;
 * further accounts get ids like `claude-k3x9` and their own login storage.
 */
export interface Account {
  id: string;
  provider: ProviderKind;
  /** The user's nickname for the account, possibly empty. */
  label: string;
  shown: boolean;
  /** Day of the month the plan is billed on (1–31), set by the user. */
  billingDay: number | null;
}

export interface LimitWindow {
  key: string;
  label: string;
  usedPercent: number;
  /** epoch seconds */
  resetsAt: number | null;
  /** Length of the window, when the provider tells. */
  periodSeconds: number | null;
  /** Heading for limits from another app than the account's own (Antigravity). */
  group: string | null;
}

/** What is currently known about one account. */
export interface ProviderStatus {
  /** The account id. */
  id: string;
  provider: ProviderKind;
  name: string;
  label: string;
  state: ProviderState;
  plan: string | null;
  windows: LimitWindow[];
  message: string | null;
  /** Remark about the grouped limits, e.g. why there are none. */
  note: string | null;
  /** epoch seconds */
  updatedAt: number | null;
}

export interface Snapshot {
  providers: ProviderStatus[];
}

export interface Settings {
  refreshMinutes: number;
  opacity: number;
  layer: "bottom" | "top" | "wallpaper";
  /** One line per account instead of a full card. */
  compact: boolean;
  notify: boolean;
  autostart: boolean;
  language: Lang;
  /** Start Antigravity's language server while the app is closed. */
  antigravityLaunch: boolean;
  accounts: Account[];
}

export type Lang = "ko" | "en";

/** Picks the Korean or the English text: `t("주간", "Weekly")`. */
export type T = (ko: string, en: string) => string;

export const translator =
  (lang: Lang | undefined): T =>
  (ko, en) =>
    lang === "en" ? en : ko;

/** The webview's user agent names the system it runs on. */
export const IS_WINDOWS = navigator.userAgent.includes("Windows");
export const IS_MAC = navigator.userAgent.includes("Macintosh");

export const isBuiltin = (a: Pick<Account, "id" | "provider">) => a.id === a.provider;

/**
 * Built-in Codex and Antigravity use the apps' logins on this PC; all others
 * sign in here.
 */
export const signsInHere = (a: Pick<Account, "id" | "provider">) =>
  !(isBuiltin(a) && (a.provider === "codex" || a.provider === "antigravity"));

/** Antigravity has one login per PC, so only its built-in account. */
export const canAdd = (provider: ProviderKind, existing: Account[]) =>
  provider !== "antigravity" || !existing.some((a) => a.id === "antigravity");

/** The built-in account of the provider if it was removed, else a further one. */
export function newAccount(provider: ProviderKind, existing: Account[], t: T): Account {
  const count = existing.filter((a) => a.provider === provider).length;
  if (!existing.some((a) => a.id === provider)) {
    return { id: provider, provider, label: "", shown: true, billingDay: null };
  }
  return {
    id: `${provider}-${Date.now().toString(36)}`,
    provider,
    label: `${t("계정", "Account")} ${count + 1}`,
    shown: true,
    billingDay: null,
  };
}

export const api = {
  getSnapshot: () => invoke<Snapshot>("get_snapshot"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  refreshNow: () => invoke<void>("refresh_now"),
  openSettings: () => invoke<void>("open_settings"),
  hideWidget: () => invoke<void>("hide_widget"),
  /** Opens the sign-in window of an account. */
  login: (account: string) => invoke<void>("provider_login", { account }),
  disconnect: (account: string) => invoke<void>("provider_disconnect", { account }),
};

/** Live snapshot + settings, kept in sync with the Rust side. */
export function useAppState() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);

  useEffect(() => {
    if (settings) document.documentElement.lang = settings.language;
  }, [settings?.language]);

  useEffect(() => {
    let alive = true;
    api.getSnapshot().then((s) => alive && setSnapshot(s));
    api.getSettings().then((s) => alive && setSettings(s));
    const offSnap = listen<Snapshot>("snapshot", (e) => setSnapshot(e.payload));
    const offSet = listen<Settings>("settings", (e) => setSettings(e.payload));
    return () => {
      alive = false;
      offSnap.then((off) => off());
      offSet.then((off) => off());
    };
  }, []);

  return { snapshot, settings, t: translator(settings?.language) };
}

/** Re-renders on an interval so countdowns stay current. Returns epoch seconds. */
export function useNow(intervalMs = 30_000) {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now() / 1000), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return now;
}

export function untilReset(resetsAt: number | null, now: number, t: T): string {
  if (!resetsAt) return "";
  const s = resetsAt - now;
  if (s <= 0) return t("곧 리셋", "Resets soon");
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return t(`${d}일 ${h}시간 후 리셋`, `Resets in ${d}d ${h}h`);
  if (h > 0) return t(`${h}시간 ${m}분 후 리셋`, `Resets in ${h}h ${m}m`);
  const mins = Math.max(1, m);
  return t(`${mins}분 후 리셋`, `Resets in ${mins}m`);
}

/**
 * How close a limit's reset is: within 1 hour / 3 hours for windows of up to a
 * day, within 1 day / 3 days for longer ones.
 */
export function resetLevel(w: LimitWindow, now: number): "far" | "mid" | "soon" | "" {
  if (!w.resetsAt) return "";
  const left = w.resetsAt - now;
  // Without a known length, more than five hours to go means a long window.
  const long = w.periodSeconds ? w.periodSeconds > 86400 : left > 5 * 3600;
  const [soon, mid] = long ? [86400, 3 * 86400] : [3600, 3 * 3600];
  return left <= soon ? "soon" : left <= mid ? "mid" : "far";
}

/** The given day of a month, or its last day when the month is shorter. */
function dayOfMonth(year: number, month: number, day: number): Date {
  const last = new Date(year, month + 1, 0).getDate();
  return new Date(year, month, Math.min(day, last));
}

/**
 * Epoch seconds (local midnight) of the next monthly billing on the day the
 * user set, or null when no day is set. Today counts as the next one.
 */
export function nextBilling(day: number | null, now: number): number | null {
  if (!day) return null;
  const today = new Date(now * 1000);
  today.setHours(0, 0, 0, 0);
  let at = dayOfMonth(today.getFullYear(), today.getMonth(), day);
  if (at < today) at = dayOfMonth(today.getFullYear(), today.getMonth() + 1, day);
  return at.getTime() / 1000;
}

/** Whole days from today to the billing date. */
export function daysUntil(at: number, now: number): number {
  const today = new Date(now * 1000);
  today.setHours(0, 0, 0, 0);
  return Math.round((at * 1000 - today.getTime()) / 86_400_000);
}

const MONTHS_EN = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** "11월 15일" / "Nov 15". */
export function dateText(at: number, t: T): string {
  const d = new Date(at * 1000);
  return t(`${d.getMonth() + 1}월 ${d.getDate()}일`, `${MONTHS_EN[d.getMonth()]} ${d.getDate()}`);
}

/** "오늘" / "내일" / "D-12" and their English forms. */
export function daysText(days: number, t: T): string {
  if (days <= 0) return t("오늘", "today");
  if (days === 1) return t("내일", "tomorrow");
  return t(`D-${days}`, `in ${days} days`);
}

/** "다음 결제 11월 15일 · D-12". */
export function billingText(at: number, now: number, t: T): string {
  return `${t("다음 결제", "Next bill")} ${dateText(at, t)} · ${daysText(daysUntil(at, now), t)}`;
}

/** Billing within a day / within three days. */
export function billingLevel(at: number, now: number): "soon" | "mid" | "" {
  const days = daysUntil(at, now);
  return days <= 1 ? "soon" : days <= 3 ? "mid" : "";
}

export function agoText(updatedAt: number | null, now: number, t: T): string {
  if (!updatedAt) return "";
  const s = Math.max(0, now - updatedAt);
  if (s < 90) return t("방금 업데이트", "Updated just now");
  const m = Math.floor(s / 60);
  if (m < 60) return t(`${m}분 전 업데이트`, `Updated ${m} min ago`);
  const h = Math.floor(m / 60);
  return t(`${h}시간 전 업데이트`, `Updated ${h} h ago`);
}

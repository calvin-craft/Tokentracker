import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  IS_MAC,
  IS_WINDOWS,
  PROVIDER_NAMES,
  api,
  billingText,
  canAdd,
  isBuiltin,
  newAccount,
  nextBilling,
  signsInHere,
  useAppState,
  useNow,
} from "./api";
import type { Account, ProviderKind, ProviderStatus, Settings, T } from "./api";

const PROVIDER_TITLES: Record<ProviderKind, string> = {
  codex: "Codex (ChatGPT)",
  claude: "Claude",
  gemini: "Gemini",
  antigravity: "Antigravity",
  grok: "Grok",
};

/** What is special about an account of a provider. */
const accountNote = (account: Account, t: T): string | null => {
  switch (account.provider) {
    case "codex":
      return isBuiltin(account)
        ? t("이 PC의 Codex 로그인을 자동으로 사용합니다.", "Uses the Codex login on this PC.")
        : null;
    case "antigravity":
      return t(
        "이 PC의 Antigravity 로그인을 자동으로 사용합니다. Antigravity가 꺼져 있으면 마지막으로 확인한 값을 보여 줍니다.",
        "Uses the Antigravity login on this PC. While Antigravity is closed, the last values seen are shown.",
      );
    case "grok":
      return t(
        "실험적 기능: 실제 계정으로 아직 확인하지 못했습니다.",
        "Experimental: not yet tried with a real account.",
      );
    default:
      return null;
  }
};

const autostartTitle = (t: T): string =>
  IS_WINDOWS
    ? t("Windows 시작 시 자동 실행", "Start with Windows")
    : IS_MAC
      ? t("macOS 로그인 시 자동 실행", "Open at login")
      : t("로그인 시 자동 실행", "Start at login");

/** What each choice of the widget position does. */
const layerNote = (layer: Settings["layer"], t: T): string => {
  switch (layer) {
    case "top":
      return t("항상 다른 창 위에 표시합니다.", "Always on top of other windows.");
    case "wallpaper":
      return t(
        "배경화면처럼 아이콘 뒤에 표시합니다. 위젯을 누르거나 옮길 때는 트레이 아이콘을 눌러 앞으로 꺼내세요. Windows 11 24H2 이상에서 동작합니다.",
        "Behind the desktop icons, like a wallpaper. To click or move the widget, bring it forward with the tray icon. Needs Windows 11 24H2 or later.",
      );
    default:
      return t(
        "다른 창 뒤, 바탕화면 아이콘 위에 표시합니다.",
        "Behind other windows, above the desktop icons.",
      );
  }
};

function statusText(p: ProviderStatus | undefined, t: T): string {
  if (!p) return t("확인 중…", "Checking…");
  switch (p.state) {
    case "ok":
      return p.plan ? `${t("연결됨", "Connected")} · ${p.plan}` : t("연결됨", "Connected");
    case "needs_login":
      return p.message ?? t("로그인 필요", "Sign-in needed");
    case "error":
      return p.message ?? t("연결 오류", "Error");
    default:
      return t("확인 중…", "Checking…");
  }
}

function Toggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className={`toggle ${checked ? "on" : ""}`}
      onClick={() => onChange(!checked)}
    >
      <span />
    </button>
  );
}

const DAYS = Array.from({ length: 31 }, (_, i) => i + 1);

function AccountRow({
  account,
  status,
  onChange,
  onRemove,
  onMove,
  isFirst,
  isLast,
  t,
}: {
  account: Account;
  status: ProviderStatus | undefined;
  onChange: (patch: Partial<Account>) => void;
  onRemove: () => void;
  onMove: (delta: -1 | 1) => void;
  isFirst: boolean;
  isLast: boolean;
  t: T;
}) {
  const [label, setLabel] = useState(account.label);
  const [confirming, setConfirming] = useState(false);
  useEffect(() => setLabel(account.label), [account.label]);
  const now = useNow(60_000);
  const billing = nextBilling(account.billingDay, now);

  const title = PROVIDER_TITLES[account.provider];
  const commitLabel = () => {
    const next = label.trim();
    if (next !== account.label) onChange({ label: next });
  };

  return (
    <div className="row">
      <span className={`dot ${account.provider}`} />
      <div className="row-main">
        <div className="row-title">
          {title}
          <input
            className="label-input"
            value={label}
            maxLength={16}
            placeholder={t("별명", "Nickname")}
            aria-label={`${title} ${t("별명", "nickname")}`}
            onChange={(e) => setLabel(e.target.value)}
            onBlur={commitLabel}
            onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
          />
        </div>
        <div className={`row-sub ${account.shown ? (status?.state ?? "") : ""}`}>
          {account.shown ? statusText(status, t) : t("표시 안 함", "Hidden")}
        </div>
        {accountNote(account, t) && <div className="row-note">{accountNote(account, t)}</div>}
        <div className="billing-field">
          <span>{t("결제일", "Billing day")}</span>
          <select
            value={account.billingDay ?? ""}
            aria-label={`${title} ${t("결제일", "billing day")}`}
            onChange={(e) => onChange({ billingDay: e.target.value ? Number(e.target.value) : null })}
          >
            <option value="">{t("설정 안 함", "Not set")}</option>
            {DAYS.map((d) => (
              <option key={d} value={d}>
                {t(`매월 ${d}일`, `Day ${d} of the month`)}
              </option>
            ))}
          </select>
          {billing && <span className="billing-next">{billingText(billing, now, t)}</span>}
        </div>
        <div className="row-actions">
          {signsInHere(account) &&
            account.shown &&
            (status?.state === "ok" ? (
              <button className="btn" onClick={() => api.disconnect(account.id)}>
                {t("연결 해제", "Sign out")}
              </button>
            ) : (
              <button className="btn primary" onClick={() => api.login(account.id)}>
                {t("로그인", "Sign in")}
              </button>
            ))}
          {confirming ? (
            <>
              <button className="btn danger" onClick={onRemove}>
                {t("정말 삭제", "Really remove")}
              </button>
              <button className="btn" onClick={() => setConfirming(false)}>
                {t("취소", "Cancel")}
              </button>
            </>
          ) : (
            <button className="btn" onClick={() => setConfirming(true)}>
              {t("삭제", "Remove")}
            </button>
          )}
        </div>
      </div>
      <div className="move">
        <button
          className="move-btn"
          title={t("위로", "Up")}
          aria-label={`${title} ${t("위로 이동", "move up")}`}
          disabled={isFirst}
          onClick={() => onMove(-1)}
        >
          <svg viewBox="0 0 12 12">
            <path d="M2.5 7.5 6 4l3.5 3.5" />
          </svg>
        </button>
        <button
          className="move-btn"
          title={t("아래로", "Down")}
          aria-label={`${title} ${t("아래로 이동", "move down")}`}
          disabled={isLast}
          onClick={() => onMove(1)}
        >
          <svg viewBox="0 0 12 12">
            <path d="M2.5 4.5 6 8l3.5-3.5" />
          </svg>
        </button>
      </div>
      <Toggle
        label={`${title} ${t("표시", "shown")}`}
        checked={account.shown}
        onChange={(v) => onChange({ shown: v })}
      />
    </div>
  );
}

export function SettingsView() {
  const { snapshot, settings, t } = useAppState();
  // Without a title bar, Esc is the keyboard way to close the popup.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") getCurrentWindow().close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!settings) return <div className="settings" />;

  const update = (patch: Partial<Settings>) => api.setSettings({ ...settings, ...patch });
  const status = (id: string) => snapshot?.providers.find((p) => p.id === id);

  const changeAccount = (id: string, patch: Partial<Account>) =>
    update({ accounts: settings.accounts.map((a) => (a.id === id ? { ...a, ...patch } : a)) });
  const removeAccount = (id: string) =>
    update({ accounts: settings.accounts.filter((a) => a.id !== id) });
  const moveAccount = (id: string, delta: -1 | 1) => {
    const list = [...settings.accounts];
    const from = list.findIndex((a) => a.id === id);
    const to = from + delta;
    if (from < 0 || to < 0 || to >= list.length) return;
    [list[from], list[to]] = [list[to], list[from]];
    update({ accounts: list });
  };
  const addAccount = async (provider: ProviderKind) => {
    const account = newAccount(provider, settings.accounts, t);
    const added = [account];
    // One Google sign-in serves both: Gemini here, Antigravity in its app.
    if (provider === "gemini" && canAdd("antigravity", settings.accounts)) {
      added.push(newAccount("antigravity", settings.accounts, t));
    }
    await update({ accounts: [...settings.accounts, ...added] });
    if (signsInHere(account)) await api.login(account.id);
  };

  return (
    <div className="settings">
      <header className="settings-head" data-tauri-drag-region>
        <span className="title" data-tauri-drag-region>
          {t("설정", "Settings")}
        </span>
        <button className="icon-btn" title={t("닫기", "Close")} onClick={() => getCurrentWindow().close()}>
          <svg viewBox="0 0 16 16">
            <path d="M4 4l8 8M12 4l-8 8" />
          </svg>
        </button>
      </header>
      <div className="settings-body">
        <h2>{t("계정", "Accounts")}</h2>
        <div className="group">
          {settings.accounts.map((account, index) => (
            <AccountRow
              key={account.id}
              account={account}
              status={status(account.id)}
              onChange={(patch) => changeAccount(account.id, patch)}
              onRemove={() => removeAccount(account.id)}
              onMove={(delta) => moveAccount(account.id, delta)}
              isFirst={index === 0}
              isLast={index === settings.accounts.length - 1}
              t={t}
            />
          ))}
          <div className="row add-row">
            <div className="row-main">
              <div className="row-title">{t("계정 추가", "Add account")}</div>
              <div className="row-note">
                {t(
                  "같은 서비스의 다른 계정을 따로 로그인해 함께 볼 수 있습니다.",
                  "Sign in to more accounts of a service to see them side by side.",
                )}
              </div>
              <div className="row-actions">
                {(Object.keys(PROVIDER_NAMES) as ProviderKind[])
                  .filter((provider) => canAdd(provider, settings.accounts))
                  .map((provider) => (
                  <button key={provider} className="btn" onClick={() => addAccount(provider)}>
                    <span className={`dot ${provider}`} />
                    {PROVIDER_NAMES[provider]}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>

        <h2>{t("표시", "Display")}</h2>
        <div className="group">
          <label className="row">
            <div className="row-main">
              <div className="row-title">{t("언어", "Language")}</div>
            </div>
            <select
              value={settings.language}
              onChange={(e) => update({ language: e.target.value as Settings["language"] })}
            >
              <option value="ko">한국어</option>
              <option value="en">English</option>
            </select>
          </label>

          <div className="row">
            <div className="row-main">
              <div className="row-title">{t("간략 보기", "Compact view")}</div>
              <div className="row-note">
                {t("계정마다 한 줄로 줄여서 보여줍니다.", "One line per account.")}
              </div>
            </div>
            <Toggle
              label={t("간략 보기", "Compact view")}
              checked={settings.compact}
              onChange={(v) => update({ compact: v })}
            />
          </div>

          <label className="row">
            <div className="row-main">
              <div className="row-title">{t("갱신 주기", "Refresh every")}</div>
            </div>
            <select
              value={settings.refreshMinutes}
              onChange={(e) => update({ refreshMinutes: Number(e.target.value) })}
            >
              {[3, 5, 10, 15, 30].map((m) => (
                <option key={m} value={m}>
                  {t(`${m}분`, `${m} min`)}
                </option>
              ))}
            </select>
          </label>

          {settings.accounts.some((a) => a.provider === "antigravity") && (
            <div className="row">
              <div className="row-main">
                <div className="row-title">
                  {t("Antigravity가 꺼져 있을 때도 확인", "Check Antigravity while it is closed")}
                </div>
                <div className="row-note">
                  {t(
                    "Antigravity의 언어 서버를 잠깐 실행해 사용량을 읽습니다. Antigravity 밖에서 그 프로그램을 실행하는 방식이라 기본으로 꺼 두었습니다. (실험적)",
                    "Briefly starts Antigravity's language server to read the usage. That runs its program from outside Antigravity, so it is off by default. (Experimental)",
                  )}
                </div>
              </div>
              <Toggle
                label={t("Antigravity가 꺼져 있을 때도 확인", "Check Antigravity while it is closed")}
                checked={settings.antigravityLaunch}
                onChange={(v) => update({ antigravityLaunch: v })}
              />
            </div>
          )}

          <label className="row">
            <div className="row-main">
              <div className="row-title">{t("위젯 위치", "Widget position")}</div>
              <div className="row-note">{layerNote(settings.layer, t)}</div>
            </div>
            <select
              value={settings.layer}
              onChange={(e) => update({ layer: e.target.value as Settings["layer"] })}
            >
              <option value="bottom">{t("바탕화면에 고정", "On the desktop")}</option>
              <option value="top">{t("항상 위", "Always on top")}</option>
              {IS_WINDOWS && (
                <option value="wallpaper">{t("배경화면 모드", "Wallpaper mode")}</option>
              )}
            </select>
          </label>

          <label className="row">
            <div className="row-main">
              <div className="row-title">{t("배경 불투명도", "Background opacity")}</div>
            </div>
            <input
              type="range"
              min={0.2}
              max={1}
              step={0.05}
              value={settings.opacity}
              onChange={(e) => update({ opacity: Number(e.target.value) })}
            />
            <span className="range-value">{Math.round(settings.opacity * 100)}%</span>
          </label>
        </div>

        <h2>{t("동작", "Behavior")}</h2>
        <div className="group">
          <div className="row">
            <div className="row-main">
              <div className="row-title">{t("한도 알림", "Limit alerts")}</div>
              <div className="row-note">
                {t("사용률이 80%, 95%에 도달하면 알립니다.", "Notifies at 80% and 95% usage.")}
              </div>
            </div>
            <Toggle
              label={t("한도 알림", "Limit alerts")}
              checked={settings.notify}
              onChange={(v) => update({ notify: v })}
            />
          </div>
          <div className="row">
            <div className="row-main">
              <div className="row-title">{autostartTitle(t)}</div>
            </div>
            <Toggle
              label={autostartTitle(t)}
              checked={settings.autostart}
              onChange={(v) => update({ autostart: v })}
            />
          </div>
        </div>
      </div>
    </div>
  );
}

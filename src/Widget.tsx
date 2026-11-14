import { Fragment, useEffect, useRef, useState } from "react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import {
  agoText,
  api,
  billingLevel,
  billingText,
  daysText,
  daysUntil,
  nextBilling,
  resetLevel,
  signsInHere,
  untilReset,
  useAppState,
  useNow,
} from "./api";
import type { LimitWindow, ProviderStatus, T } from "./api";

const WIDGET_WIDTH = 300;

function level(pct: number): "low" | "mid" | "high" {
  if (pct >= 85) return "high";
  if (pct >= 60) return "mid";
  return "low";
}

function Bar({ w, now, t }: { w: LimitWindow; now: number; t: T }) {
  const pct = Math.max(0, Math.min(100, w.usedPercent));
  return (
    <div className="limit">
      <div className="limit-head">
        <span className="limit-label">{w.label}</span>
        <span className={`limit-reset ${resetLevel(w, now)}`}>{untilReset(w.resetsAt, now, t)}</span>
        <span className={`limit-pct ${level(pct)}`}>{Math.round(pct)}%</span>
      </div>
      <div className="track">
        <div className={`fill ${level(pct)}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

type RowProps = { p: ProviderStatus; billing: number | null; now: number; t: T };

function Provider({ p, billing, now, t }: RowProps) {
  // The account's own limits first, then those of another app (Antigravity).
  const own = p.windows.filter((w) => !w.group);
  const grouped = p.windows.filter((w) => w.group);
  return (
    <section className="provider">
      <div className="provider-head">
        <span className={`dot ${p.provider}`} />
        <span className="provider-name">{p.name}</span>
        {p.label && <span className="provider-label">{p.label}</span>}
        {p.plan && <span className="badge">{p.plan}</span>}
        {p.state === "error" && (
          <span className="warn" title={p.message ?? ""}>
            {t("연결 오류", "Error")}
          </span>
        )}
      </div>

      {own.map((w) => (
        <Bar key={w.key} w={w} now={now} t={t} />
      ))}

      {p.state === "loading" && own.length === 0 && <p className="hint">{t("불러오는 중…", "Loading…")}</p>}
      {p.state === "needs_login" && (
        <p className="hint">
          {p.message ?? t("로그인이 필요합니다.", "Sign-in required.")}
          {signsInHere(p) && (
            <button className="link" onClick={() => api.login(p.id)}>
              {t("로그인", "Sign in")}
            </button>
          )}
        </p>
      )}
      {p.state === "error" && own.length === 0 && (
        <p className="hint">{p.message ?? t("정보를 가져오지 못했습니다.", "Could not get the data.")}</p>
      )}

      {grouped.map((w, i) => (
        <Fragment key={w.key}>
          {w.group !== grouped[i - 1]?.group && <div className="group-cap">{w.group}</div>}
          <Bar w={w} now={now} t={t} />
        </Fragment>
      ))}
      {p.note && <p className="hint note">{p.note}</p>}
      {billing && <p className={`billing ${billingLevel(billing, now)}`}>{billingText(billing, now, t)}</p>}
    </section>
  );
}

/** The small bars of one compact line. */
function CompactBars({ windows, now, t }: { windows: LimitWindow[]; now: number; t: T }) {
  return (
    <>
      {windows.map((w) => {
        const pct = Math.max(0, Math.min(100, w.usedPercent));
        return (
          <div
            key={w.key}
            className="compact-win"
            title={[w.label, untilReset(w.resetsAt, now, t)].filter(Boolean).join(" · ")}
          >
            <div className="compact-head">
              <span>{w.label}</span>
              <strong className={level(pct)}>{Math.round(pct)}%</strong>
            </div>
            <div className="track">
              <div className={`fill ${level(pct)}`} style={{ width: `${pct}%` }} />
            </div>
          </div>
        );
      })}
    </>
  );
}

/**
 * Compact version of `Provider`: one entry per account, with its name and
 * first two limits. Limits of another app that belong to the account
 * (Antigravity's, in Gemini) go on a second line of the same entry.
 */
function CompactRow({ p, billing, now, t }: RowProps) {
  const own = p.windows.filter((w) => !w.group).slice(0, 2);
  const groups = [...new Set(p.windows.flatMap((w) => (w.group ? [w.group] : [])))];
  return (
    <section className="compact">
      <div className="compact-line">
        <span className={`dot ${p.provider}`} />
        <span
          className="compact-name"
          title={[p.name, p.label, p.plan, billing && billingText(billing, now, t)].filter(Boolean).join(" · ")}
        >
          {p.name}
          {p.label && <span className="provider-label"> {p.label}</span>}
        </span>
        {billing && daysUntil(billing, now) <= 3 && (
          <span className={`compact-bill ${billingLevel(billing, now)}`} title={billingText(billing, now, t)}>
            {daysText(daysUntil(billing, now), t)}
          </span>
        )}
        {p.state === "error" && own.length > 0 && (
          <span className="warn" title={p.message ?? t("연결 오류", "Error")}>
            !
          </span>
        )}
        <CompactBars windows={own} now={now} t={t} />
        {own.length === 0 && (
          <span className="compact-state">
            {p.state === "needs_login" ? (
              signsInHere(p) ? (
                <button className="link" onClick={() => api.login(p.id)}>
                  {t("로그인", "Sign in")}
                </button>
              ) : (
                t("로그인 필요", "Sign-in needed")
              )
            ) : p.state === "error" ? (
              t("연결 오류", "Error")
            ) : (
              t("불러오는 중…", "Loading…")
            )}
          </span>
        )}
      </div>
      {groups.map((group) => (
        <div key={group} className="compact-line">
          <span className="dot-space" />
          <span className="compact-name compact-sub" title={group}>
            {group}
          </span>
          <CompactBars
            windows={p.windows.filter((w) => w.group === group).slice(0, 2)}
            now={now}
            t={t}
          />
        </div>
      ))}
    </section>
  );
}

export function Widget() {
  const { snapshot, settings, t } = useAppState();
  const now = useNow();
  const cardRef = useRef<HTMLDivElement>(null);
  const [spinning, setSpinning] = useState(false);

  // Keep the (transparent) window exactly as tall as the card.
  useEffect(() => {
    const el = cardRef.current;
    if (!el) return;
    const win = getCurrentWindow();
    const sync = () => {
      const h = Math.ceil(el.getBoundingClientRect().height);
      if (h > 0) win.setSize(new LogicalSize(WIDGET_WIDTH, h)).catch(() => {});
    };
    const ro = new ResizeObserver(sync);
    ro.observe(el);
    sync();
    return () => ro.disconnect();
  }, []);

  const accounts = new Map((settings?.accounts ?? []).map((a) => [a.id, a]));
  const visible = (snapshot?.providers ?? []).filter((p) => accounts.get(p.id)?.shown);
  const billing = (p: ProviderStatus) => nextBilling(accounts.get(p.id)?.billingDay ?? null, now);
  const lastUpdate = visible.reduce<number | null>(
    (acc, p) => (p.updatedAt && (!acc || p.updatedAt > acc) ? p.updatedAt : acc),
    null,
  );

  const refresh = () => {
    setSpinning(true);
    api.refreshNow().finally(() => setTimeout(() => setSpinning(false), 700));
  };

  return (
    <div
      className="card"
      ref={cardRef}
      style={{ ["--alpha" as string]: String(settings?.opacity ?? 0.85) }}
    >
      <header className="card-head" data-tauri-drag-region>
        <span className="title" data-tauri-drag-region>
          TokenTracker
        </span>
        {settings && (
          <button
            className="icon-btn"
            title={settings.compact ? t("자세히 보기", "Detailed view") : t("간략 보기", "Compact view")}
            onClick={() => api.setSettings({ ...settings, compact: !settings.compact })}
          >
            {settings.compact ? (
              <svg viewBox="0 0 16 16">
                <rect x="2.5" y="2.5" width="11" height="4.5" rx="1.2" />
                <rect x="2.5" y="9" width="11" height="4.5" rx="1.2" />
              </svg>
            ) : (
              <svg viewBox="0 0 16 16">
                <path d="M2.5 4h11M2.5 8h11M2.5 12h11" />
              </svg>
            )}
          </button>
        )}
        <button className="icon-btn" title={t("새로고침", "Refresh")} onClick={refresh}>
          <svg viewBox="0 0 16 16" className={spinning ? "spin" : ""}>
            <path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9" />
            <path d="M13.5 2.5v3h-3" />
          </svg>
        </button>
        <button className="icon-btn" title={t("설정", "Settings")} onClick={() => api.openSettings()}>
          <svg viewBox="0 0 16 16">
            {/* A cog: eight teeth around a ring with a hole. */}
            <path d="M6.9 1.5h2.2l.35 1.75 1.05.45 1.5-1 1.55 1.55-1 1.5.45 1.05 1.75.35v2.2l-1.75.35-.45 1.05 1 1.5-1.55 1.55-1.5-1-1.05.45-.35 1.75H6.9l-.35-1.75-1.05-.45-1.5 1-1.55-1.55 1-1.5-.45-1.05L1.25 9.1V6.9L3 6.55l.45-1.05-1-1.5L4 2.45l1.5 1 1.05-.45z" />
            <circle cx="8" cy="8" r="2" />
          </svg>
        </button>
      </header>

      {settings?.compact ? (
        <div className="compact-list">
          {visible.map((p) => (
            <CompactRow key={p.id} p={p} billing={billing(p)} now={now} t={t} />
          ))}
        </div>
      ) : (
        visible.map((p) => <Provider key={p.id} p={p} billing={billing(p)} now={now} t={t} />)
      )}
      {snapshot && visible.length === 0 && (
        <p className="hint">
          {t("표시할 서비스가 없습니다. 설정에서 켜 주세요.", "Nothing to show. Turn services on in Settings.")}
        </p>
      )}

      <footer className="card-foot">{agoText(lastUpdate, now, t)}</footer>
    </div>
  );
}

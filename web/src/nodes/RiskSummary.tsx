import { zhCN as t } from "../i18n";
import type { TestRecord, TestState } from "../types";

export function RiskSummary({ record, operation, eligible }: {
  record: TestRecord | undefined;
  operation: TestState | undefined;
  eligible: boolean;
}) {
  const effective = operation?.state === "succeeded" || operation?.state === "failed"
    ? operation.record
    : record;

  if (operation?.state === "queued" || operation?.state === "running") {
    return (
      <div className="test-progress" aria-live="polite">
        <span className="activity-spinner" aria-hidden="true" />
        <div>
          <strong>{operation.state === "queued" ? t.queued : t.running}</strong>
          <span>独立 worker · 远端 DNS</span>
        </div>
      </div>
    );
  }
  if (effective?.error !== undefined) {
    return (
      <div className="test-error">
        <strong>{t.testFailed}</strong>
        <span title={effective.error}>{effective.error}</span>
        <time dateTime={effective.testedAt}>{effective.durationMs} ms</time>
      </div>
    );
  }
  if (effective?.result === undefined) {
    return eligible ? (
      <div className="auto-test-state">
        <span className="activity-spinner" aria-hidden="true" />
        <div><strong>{t.awaitingAutoTest}</strong><span>检测完成后自动更新</span></div>
      </div>
    ) : <span className="empty-value">{t.noResult}</span>;
  }

  const score = Math.max(0, Math.min(100, effective.result.fraudScore));
  const scoreTone = score <= 25 ? "low" : score <= 70 ? "medium" : "high";
  return (
    <div className="risk-summary">
      <div className="risk-summary__score">
        <span>{t.fraudScore}</span>
        <strong className={`risk-number risk-number--${scoreTone}`}>{effective.result.fraudScore}</strong>
      </div>
      <div
        className="risk-meter"
        role="meter"
        aria-label={`${t.fraudScore} ${effective.result.fraudScore}`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={score}
      >
        <span className={`risk-meter__fill risk-meter__fill--${scoreTone}`} style={{ width: `${score}%` }} />
      </div>
      <div className="risk-tags">
        <span className={`tag ${effective.result.isResidential ? "tag--good" : "tag--warn"}`}>
          {effective.result.isResidential ? t.residential : t.nonResidential}
        </span>
        <span className={`tag ${effective.result.isBroadcast ? "tag--warn" : "tag--good"}`}>
          {effective.result.isBroadcast ? t.broadcast : t.notBroadcast}
        </span>
      </div>
      <div className="risk-summary__meta">
        {effective.result.exitIp !== undefined && <code>{effective.result.exitIp}</code>}
        <time dateTime={effective.testedAt} title={new Date(effective.testedAt).toLocaleString("zh-CN")}>
          {effective.durationMs} ms · {new Date(effective.testedAt).toLocaleDateString("zh-CN")}
        </time>
      </div>
    </div>
  );
}

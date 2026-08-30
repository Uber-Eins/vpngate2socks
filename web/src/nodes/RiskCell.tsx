import { zhCN as t } from "../i18n";
import type { TestRecord, TestState } from "../types";
import { Badge } from "../ui/Badge";
import { Spinner } from "../ui/Feedback";
import { RiskMeter, riskTone } from "../ui/Meter";

/**
 * Risk column. A live operation for this node wins over the stored record, so a
 * re-test shows progress instead of the value it is about to replace.
 */
export function RiskCell({ record, operation, eligible }: {
  record: TestRecord | undefined;
  operation: TestState | undefined;
  eligible: boolean;
}) {
  if (operation?.state === "queued" || operation?.state === "running") {
    return (
      <div className="risk-cell__pending" aria-live="polite">
        <Spinner />
        {operation.state === "queued" ? t.queuedShort : t.runningShort}
      </div>
    );
  }

  const effective = operation?.state === "succeeded" || operation?.state === "failed"
    ? operation.record
    : record;

  if (effective?.error !== undefined) {
    return (
      <div className="risk-cell__error" title={effective.error}>
        {t.testFailed} · {effective.error}
      </div>
    );
  }

  if (effective?.result === undefined) {
    return eligible ? (
      <div className="risk-cell__pending">
        <Spinner />
        {t.awaitingAutoTest}
      </div>
    ) : (
      <span className="risk-cell__untested">{t.notTested}</span>
    );
  }

  const { fraudScore, isResidential, isBroadcast } = effective.result;
  return (
    <div className="risk-cell">
      <span className={`risk-cell__score risk-value--${riskTone(fraudScore)}`}>{fraudScore}</span>
      <RiskMeter score={fraudScore} label={`${t.fraudScore} ${fraudScore}`} />
      <div className="risk-cell__tags">
        <Badge tone={isResidential ? "accent" : "neutral"}>
          {isResidential ? t.residential : t.nonResidential}
        </Badge>
        <Badge tone={isBroadcast ? "warning" : "neutral"}>
          {isBroadcast ? t.broadcast : t.notBroadcast}
        </Badge>
      </div>
    </div>
  );
}

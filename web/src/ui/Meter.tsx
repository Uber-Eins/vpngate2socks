export type RiskTone = "low" | "medium" | "high";

export function riskTone(score: number): RiskTone {
  if (score <= 25) return "low";
  return score <= 70 ? "medium" : "high";
}

/**
 * Fraud-score bar drawn as SVG. A CSS width would have to come from a `style`
 * attribute, which the served `default-src 'self'` CSP blocks; an SVG geometry
 * attribute is markup and is not.
 */
export function RiskMeter({ score, label }: { score: number; label: string }) {
  const clamped = Math.max(0, Math.min(100, score));
  return (
    <svg
      className="meter"
      viewBox="0 0 100 6"
      preserveAspectRatio="none"
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={clamped}
    >
      <rect className="meter__track" x="0" y="0" width="100" height="6" rx="3" />
      <rect
        className={`meter__fill meter__fill--${riskTone(clamped)}`}
        x="0"
        y="0"
        width={clamped}
        height="6"
        rx="3"
      />
    </svg>
  );
}

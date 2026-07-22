import { zhCN as t } from "../i18n";
import { Notice } from "./Notice";

export function LoadingScreen({ error }: { error: string | undefined }) {
  return (
    <main className="loading-screen">
      <div className="loading-screen__brand" aria-hidden="true"><span /></div>
      <div>
        <p className="eyebrow">{t.brand}</p>
        <strong>{error === undefined ? "正在启动安全控制面…" : "控制面暂不可用"}</strong>
      </div>
      {error !== undefined && <Notice tone="danger">{error}</Notice>}
    </main>
  );
}

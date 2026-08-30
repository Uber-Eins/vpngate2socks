import { zhCN as t } from "../i18n";
import { Button } from "./Button";
import { Notice, Spinner } from "./Feedback";
import { ShieldIcon } from "./Icon";

export function LoadingScreen({ error }: { error: string | undefined }) {
  return (
    <main className="entry">
      <div className="entry__card">
        <aside className="entry__aside">
          <span className="brand-mark"><ShieldIcon size={17} /></span>
          <h1>{t.brand}</h1>
          <p>{t.loginBody}</p>
        </aside>
        <div className="entry__form">
          {error === undefined ? (
            <div className="entry__status">
              <Spinner large />
              <span>{t.booting}</span>
            </div>
          ) : (
            <>
              <h2>{t.bootFailed}</h2>
              <Notice tone="danger">{error}</Notice>
              <Button variant="primary" wide onClick={() => window.location.reload()}>
                {t.retry}
              </Button>
            </>
          )}
        </div>
      </div>
    </main>
  );
}

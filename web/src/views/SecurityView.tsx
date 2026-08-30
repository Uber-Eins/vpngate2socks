import { zhCN as t } from "../i18n";
import type { SessionState, StatusSnapshot } from "../types";
import { Button } from "../ui/Button";
import { Card } from "../ui/Card";
import { AlertIcon, CheckCircleIcon, InfoIcon, LogOutIcon } from "../ui/Icon";

type Level = "ok" | "warn" | "bad";

export function SecurityView({ status, session, busy, onLogout }: {
  status: StatusSnapshot | undefined;
  session: SessionState;
  busy: string | undefined;
  onLogout: () => void;
}) {
  const lanMode = status?.lanMode === true;
  const tls = status?.tlsConfigured === true;
  const tlsLevel: Level = tls ? "ok" : lanMode ? "bad" : "warn";
  const tlsBody = tls ? t.tlsOn : lanMode ? t.tlsOffLan : t.tlsOffLocal;

  return (
    <>
      <div className="view-heading">
        <div>
          <h1>{t.securityTitle}</h1>
          <p>{t.securitySubtitle}</p>
        </div>
      </div>

      <div className="grid-2">
        <Card title={t.deploymentStatus}>
          <div className="checklist">
            <Item level={tlsLevel} title={t.tlsTitle} body={tlsBody} />
            <Item
              level={lanMode ? "warn" : "ok"}
              title={t.lanTitle}
              body={lanMode ? t.lanOn : t.lanOff}
            />
            <Item level="warn" title={t.leakTitle} body={t.leakBody} />
            <Item level="ok" title={t.isolationTitle} body={t.isolationBody} />
          </div>
        </Card>

        <Card
          title={t.sessionTitle}
          footer={
            session.csrfToken === undefined ? undefined : (
              <Button variant="danger" disabled={busy !== undefined} onClick={onLogout}>
                <LogOutIcon size={14} />
                {t.logout}
              </Button>
            )
          }
        >
          <p className="muted">{t.sessionBody}</p>
        </Card>
      </div>
    </>
  );
}

function Item({ level, title, body }: { level: Level; title: string; body: string }) {
  return (
    <div className="checklist__item">
      <span className={`checklist__mark checklist__mark--${level}`}>
        {level === "ok" ? <CheckCircleIcon /> : level === "warn" ? <InfoIcon /> : <AlertIcon />}
      </span>
      <div>
        <strong>{title}</strong>
        <p>{body}</p>
      </div>
    </div>
  );
}

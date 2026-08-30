import { zhCN as t } from "../i18n";
import { useConsole } from "../state/useConsole";
import { useView, type View } from "../state/useView";
import type { SessionState } from "../types";
import { Notice } from "../ui/Feedback";
import { Toasts } from "../ui/Toasts";
import { NodesView } from "../views/NodesView";
import { OverviewView } from "../views/OverviewView";
import { PolicyView } from "../views/PolicyView";
import { SecurityView } from "../views/SecurityView";
import { Rail } from "./Rail";
import { TopBar } from "./TopBar";

const TITLES: Record<View, string> = {
  overview: t.overviewTitle,
  nodes: t.nodesTitle,
  policy: t.policyTitle,
  security: t.securityTitle
};

export function Console({ session, onLoggedOut }: {
  session: SessionState;
  onLoggedOut: () => void;
}) {
  const model = useConsole(onLoggedOut, t);
  const [view, setView] = useView();

  return (
    <>
      <div className="shell">
        <Rail
          view={view}
          nodeCount={model.status?.lastRefresh?.accepted}
          canLogout={session.csrfToken !== undefined}
          busy={model.busy !== undefined}
          onView={setView}
          onLogout={model.logout}
        />
        <div className="shell__main">
          <TopBar
            title={TITLES[view]}
            status={model.status}
            live={model.live}
            busy={model.busy}
            onRefresh={model.refresh}
            onDisconnect={model.disconnect}
          />
          <main className="content">
            <div className="content__inner">
              {model.error !== undefined && (
                <div className="notice-stack">
                  <Notice tone="danger">{model.error}</Notice>
                </div>
              )}
              {view === "overview" && <OverviewView model={model} onNavigate={setView} />}
              {view === "nodes" && <NodesView model={model} />}
              {view === "policy" && (
                <PolicyView
                  settings={model.autoConnect}
                  busy={model.busy}
                  onSave={model.saveAutoConnect}
                />
              )}
              {view === "security" && (
                <SecurityView
                  status={model.status}
                  session={session}
                  busy={model.busy}
                  onLogout={model.logout}
                />
              )}
            </div>
          </main>
        </div>
      </div>
      <Toasts toasts={model.toasts} closeLabel={t.close} onDismiss={model.dismissToast} />
    </>
  );
}

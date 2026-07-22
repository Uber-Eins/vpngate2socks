import { useEffect, useState } from "react";

import * as api from "./api";
import { Login } from "./auth/Login";
import { Dashboard } from "./dashboard/Dashboard";
import type { SessionState } from "./types";
import { LoadingScreen } from "./ui/LoadingScreen";
import { errorMessage } from "./utils/errorMessage";

export function App() {
  const [session, setSession] = useState<SessionState>();
  const [startupError, setStartupError] = useState<string>();

  useEffect(() => {
    void api
      .session()
      .then(setSession)
      .catch((reason: unknown) => setStartupError(errorMessage(reason)));
  }, []);

  if (session === undefined) {
    return <LoadingScreen error={startupError} />;
  }
  if (!session.authenticated) {
    return <Login onAuthenticated={setSession} />;
  }
  return <Dashboard session={session} onLoggedOut={() => setSession({ authenticated: false })} />;
}

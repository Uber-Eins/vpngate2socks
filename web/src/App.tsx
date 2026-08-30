import { useEffect, useState } from "react";

import * as api from "./api";
import { Console } from "./app/Console";
import { Login } from "./auth/Login";
// Imported for its side effect: resolves and applies the stored theme to <html>.
import "./state/useTheme";
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
  return <Console session={session} onLoggedOut={() => setSession({ authenticated: false })} />;
}

import { useState, type FormEvent } from "react";

import * as api from "../api";
import { zhCN as t } from "../i18n";
import type { SessionState } from "../types";
import { Button } from "../ui/Button";
import { Notice } from "../ui/Feedback";
import { CheckIcon, ShieldIcon } from "../ui/Icon";
import { errorMessage } from "../utils/errorMessage";

const POINTS = [t.loginPointIsolation, t.loginPointFailClosed, t.loginPointRemoteDns];

export function Login({ onAuthenticated }: {
  onAuthenticated: (session: SessionState) => void;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string>();
  const [submitting, setSubmitting] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(undefined);
    try {
      onAuthenticated(await api.login(username, password));
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setPassword("");
      setSubmitting(false);
    }
  };

  return (
    <main className="entry">
      <div className="entry__card">
        <aside className="entry__aside">
          <span className="brand-mark"><ShieldIcon size={17} /></span>
          <h1>{t.loginHeadline}</h1>
          <p>{t.loginBody}</p>
          <div className="entry__points">
            {POINTS.map((point) => (
              <span className="entry__point" key={point}>
                <CheckIcon size={14} />
                {point}
              </span>
            ))}
          </div>
        </aside>

        <form className="entry__form" onSubmit={(event) => void submit(event)}>
          <div>
            <h2>{t.loginTitle}</h2>
            <p className="entry__form-hint">{t.loginHint}</p>
          </div>

          {window.location.protocol !== "https:" && (
            <Notice tone="danger">{t.cleartextWarning}</Notice>
          )}

          <div className="field">
            <label className="field__label" htmlFor="login-username">{t.username}</label>
            <input
              id="login-username"
              className="input"
              autoComplete="username"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              required
            />
          </div>

          <div className="field">
            <label className="field__label" htmlFor="login-password">{t.password}</label>
            <input
              id="login-password"
              className="input"
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              required
            />
          </div>

          {error !== undefined && <p className="entry__error" role="alert">{error}</p>}

          <Button type="submit" variant="primary" wide busy={submitting}>
            {submitting ? t.loggingIn : t.login}
          </Button>
        </form>
      </div>
    </main>
  );
}

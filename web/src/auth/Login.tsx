import { useState, type FormEvent } from "react";

import * as api from "../api";
import { zhCN as t } from "../i18n";
import type { SessionState } from "../types";
import { Notice } from "../ui/Notice";
import { errorMessage } from "../utils/errorMessage";

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
    <main className="login-shell">
      <section className="login-intro" aria-label="产品说明">
        <div className="brand-mark brand-mark--large" aria-hidden="true"><span /></div>
        <p className="eyebrow">{t.brand}</p>
        <h1>安全出口，从验证开始。</h1>
        <p>{t.subtitle}</p>
        <span className="security-stamp"><i aria-hidden="true" />{t.secureRoute}</span>
      </section>
      <form className="login-card" onSubmit={(event) => void submit(event)}>
        <div>
          <p className="section-kicker">AUTHENTICATION</p>
          <h2>{t.loginTitle}</h2>
        </div>
        {window.location.protocol !== "https:" && (
          <Notice tone="danger">{t.cleartextWarning}</Notice>
        )}
        <label>
          <span>{t.username}</span>
          <input
            autoComplete="username"
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            required
          />
        </label>
        <label>
          <span>{t.password}</span>
          <input
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            required
          />
        </label>
        {error !== undefined && <p className="form-error" role="alert">{error}</p>}
        <button className="button button--primary button--wide" type="submit" disabled={submitting}>
          {submitting ? "验证中…" : t.login}
        </button>
      </form>
    </main>
  );
}

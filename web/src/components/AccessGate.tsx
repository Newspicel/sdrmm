import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useEffect, useState } from "react";
import { authQuery, ME_KEY, meQuery, signInUrl } from "../lib/api";
import { adoptSignInResult, onTokenRejected, onUnauthorized, setToken } from "../lib/auth";
import { RoleContext } from "../lib/role";
import type { AuthInfo } from "../lib/types";
import { Button, Form, Input } from "./BaseControls";
import { BTN, FIELD } from "./controls";

const SIGN_IN_TEXT: Record<string, string> = {
  denied: "This account has no access.",
  failed: "Sign-in failed.",
};

/** Shows the app once the server knows who this is, and asks for sign-in or the token until then. */
export function AccessGate({ onToken, children }: { onToken: () => void; children: ReactNode }) {
  const queryClient = useQueryClient();
  const auth = useQuery(authQuery());
  const me = useQuery({ ...meQuery(), enabled: auth.data !== undefined });
  const [signIn] = useState(() => adoptSignInResult(window.location, window.history));
  const [refused, setRefused] = useState(false);

  useEffect(() => {
    const askAgain = () => {
      if (queryClient.getQueryState(ME_KEY)?.status === "success") {
        void queryClient.invalidateQueries({ queryKey: ME_KEY });
      }
    };
    const stopRefused = onTokenRejected(() => setRefused(true));
    const stopUnauthorized = onUnauthorized(askAgain);
    return () => {
      stopRefused();
      stopUnauthorized();
    };
  }, [queryClient]);

  const guarded = auth.data?.token_required === true || auth.data?.sign_in === true;
  if (auth.data === undefined || (guarded && me.status === "pending")) {
    return null;
  }
  if (!guarded || me.status === "success") {
    return <RoleContext value={me.data?.role ?? "admin"}>{children}</RoleContext>;
  }
  return (
    <Gate
      auth={auth.data}
      notice={refused ? "That token was refused." : SIGN_IN_TEXT[signIn ?? ""]}
      signInHref={signInUrl(window.location)}
      onToken={(token) => {
        setToken(token);
        setRefused(false);
        void queryClient.invalidateQueries();
        onToken();
      }}
    />
  );
}

export function Gate({
  auth,
  notice,
  signInHref,
  onToken,
}: {
  auth: AuthInfo;
  notice: string | undefined;
  signInHref: string;
  onToken: (token: string) => void;
}) {
  const [entry, setEntry] = useState("");
  return (
    <div className="flex min-h-full items-center justify-center bg-bg px-4 py-10">
      <div className="flex w-full max-w-sm flex-col gap-3 rounded-control border border-line bg-panel px-4 py-4">
        <div className="flex items-center gap-2">
          <img src="/icon.svg" alt="" width={28} height={28} className="shrink-0" />
          <div className="font-mono text-lg font-semibold text-accent">SDRmm</div>
        </div>
        {notice !== undefined && (
          <p role="alert" className="font-mono text-sm text-danger">
            {notice}
          </p>
        )}
        {auth.sign_in && (
          <a className={`${BTN} text-center`} href={signInHref}>
            Sign in
          </a>
        )}
        {auth.token_required && (
          <Form
            className="flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault();
              const token = entry.trim();
              if (token !== "") {
                onToken(token);
              }
            }}
          >
            <Input
              className={`${FIELD} w-full`}
              type="password"
              autoComplete="current-password"
              aria-label="Shared token"
              placeholder="Shared token"
              value={entry}
              onChange={(event) => setEntry(event.target.value)}
            />
            <Button type="submit" className={BTN} disabled={entry.trim() === ""}>
              Connect
            </Button>
          </Form>
        )}
      </div>
    </div>
  );
}

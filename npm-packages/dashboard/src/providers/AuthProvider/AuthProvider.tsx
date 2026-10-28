import React, {
  useState,
  useEffect,
  useCallback,
  ReactNode,
  useMemo,
} from "react";
import { WorkOSSession } from "server/workos";
import { User } from "@workos-inc/node";
import { AuthContext, AuthContextType } from "./AuthContext";

interface AuthProviderProps {
  children: ReactNode;
}

export function AuthProvider({ children }: AuthProviderProps) {
  const [session, setSession] = useState<WorkOSSession | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  const fetchSession = useCallback(async () => {
    let redirecting = false;
    try {
      setError(null);
      const response = await fetch("/api/auth/session");

      if (response.ok) {
        const sessionData = await response.json();
        setSession(sessionData);
      } else if (response.status === 401) {
        setSession(null);
      } else if (
        response.status === 503 &&
        window.location.pathname !== "/login-error"
      ) {
        redirecting = true;
        window.location.assign(
          `/login-error?returnTo=${encodeURIComponent(
            window.location.pathname + window.location.search,
          )}`,
        );
        return;
      } else {
        throw new Error("Failed to fetch session");
      }
    } catch (err) {
      setError(err as Error);
      setSession(null);
    } finally {
      if (!redirecting) {
        setIsLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    void fetchSession();
  }, [fetchSession]);

  const user: User | null = useMemo(
    () => (session ? (session.user as User) : null),
    [session],
  );

  const value: AuthContextType = useMemo(
    () => ({
      user,
      isAuthenticated: !!session,
      isLoading,
      error,
    }),
    [user, session, isLoading, error],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

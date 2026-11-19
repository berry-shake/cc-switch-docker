import { useState, type FormEvent } from "react";
import { Eye, EyeOff, Loader2 } from "lucide-react";
import ccswitchLogo from "@/assets/icons/logo.svg";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { login } from "./auth";
import { webText } from "./strings";

interface LoginPageProps {
  version?: string;
  /** 会话过期被踢回来时给出提示 */
  notice?: string | null;
  onSuccess: () => void;
}

export function LoginPage({ version, notice, onSuccess }: LoginPageProps) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [remember, setRemember] = useState(true);
  const [showPassword, setShowPassword] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (submitting) return;
    if (!username.trim() || !password) {
      setError(webText("required"));
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await login(username.trim(), password, remember);
      onSuccess();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setPassword("");
      setSubmitting(false);
    }
  };

  const message = error ?? notice;

  return (
    <div className="flex min-h-screen items-center justify-center bg-app px-4 py-10 text-fg-1">
      <div className="w-full max-w-[380px]">
        <div className="mb-7 flex flex-col items-center gap-3 text-center">
          <img
            src={ccswitchLogo}
            alt=""
            className="h-12 w-12 select-none"
            draggable={false}
          />
          <div className="space-y-1">
            <h1 className="text-title text-fg-1">CC Switch</h1>
            <p className="text-body text-fg-3">{webText("subtitle")}</p>
          </div>
        </div>

        <form
          onSubmit={handleSubmit}
          noValidate
          className="rounded-dialog border border-border bg-surface p-6 shadow-sm"
        >
          <div className="space-y-4">
            <div className="space-y-1.5">
              <label
                htmlFor="ccs-username"
                className="block text-strong text-fg-2"
              >
                {webText("username")}
              </label>
              <Input
                id="ccs-username"
                name="username"
                autoComplete="username"
                autoFocus
                value={username}
                aria-invalid={error != null && !username.trim()}
                onChange={(event) => setUsername(event.target.value)}
                disabled={submitting}
                className="h-9"
              />
            </div>

            <div className="space-y-1.5">
              <label
                htmlFor="ccs-password"
                className="block text-strong text-fg-2"
              >
                {webText("password")}
              </label>
              <div className="relative">
                <Input
                  id="ccs-password"
                  name="password"
                  type={showPassword ? "text" : "password"}
                  autoComplete="current-password"
                  value={password}
                  aria-invalid={error != null && !password}
                  onChange={(event) => setPassword(event.target.value)}
                  disabled={submitting}
                  className="h-9 pe-10"
                />
                <button
                  type="button"
                  onClick={() => setShowPassword((value) => !value)}
                  className="absolute inset-y-0 end-0 flex w-9 items-center justify-center rounded-e-control text-fg-3 transition-colors hover:text-fg-1 focus-visible:text-fg-1 focus-visible:outline-none"
                  aria-label={webText(
                    showPassword ? "hidePassword" : "showPassword",
                  )}
                  tabIndex={-1}
                >
                  {showPassword ? (
                    <EyeOff className="h-4 w-4" strokeWidth={1.75} />
                  ) : (
                    <Eye className="h-4 w-4" strokeWidth={1.75} />
                  )}
                </button>
              </div>
            </div>

            <label className="flex cursor-pointer select-none items-center gap-2 text-body text-fg-2">
              <Checkbox
                checked={remember}
                onCheckedChange={setRemember}
                disabled={submitting}
              />
              {webText("remember")}
            </label>

            <div
              role="alert"
              aria-live="polite"
              className={cn(
                "min-h-[20px] text-caption",
                error ? "text-danger" : "text-fg-3",
              )}
            >
              {message}
            </div>

            <Button
              type="submit"
              variant="solid"
              size="lg"
              className="w-full rounded-control"
              disabled={submitting}
            >
              {submitting ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  {webText("signingIn")}
                </>
              ) : (
                webText("signIn")
              )}
            </Button>
          </div>
        </form>

        {version && (
          <p className="mt-6 text-center text-caption text-fg-3">
            cc-switch-server v{version}
          </p>
        )}
      </div>
    </div>
  );
}

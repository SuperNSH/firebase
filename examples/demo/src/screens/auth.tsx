// The Authentication screen: an nsh-form sign-in/sign-up form over the
// auth library with core's emulator config, and the currentUser signal.
import { Scroll, Text, View, signal } from "nsh";
import { createForm, email, inputProps, minLength, required } from "nsh-form";
import { AUTH_CODES } from "nsh-firebase/auth";
import type { FirebaseError } from "nsh-firebase/core";
import { demoAuth } from "../firebase";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";

interface Credentials {
  email: string;
  password: string;
}

// A server reply maps to the field it names; everything else stays a
// form-level message with the code, as the library worded it.
function submitErrors(error: FirebaseError): { fields?: Record<string, string>; form?: string } {
  switch (error.code) {
    case AUTH_CODES.userNotFound:
      return { fields: { email: "No account with that email." } };
    case AUTH_CODES.wrongPassword:
    case AUTH_CODES.invalidCredential:
      return { fields: { password: "That password is wrong for the account." } };
    case AUTH_CODES.emailAlreadyInUse:
      return { fields: { email: "That email already has an account." } };
    case AUTH_CODES.weakPassword:
      return { fields: { password: "That password is too weak." } };
    default:
      return { form: `${error.code}: ${error.message}` };
  }
}

// Field rules, run on every change and again on submit.
const emailRules = ({ value }: { value: string }) =>
  required("Enter your email.")({ value }) ?? email("That is not an email address.")({ value });
const passwordRules = ({ value }: { value: string }) =>
  required("Enter your password.")({ value }) ?? minLength(8, "Use at least 8 characters.")({ value });

export function AuthScreen() {
  const auth = demoAuth();
  const mode = signal<"signin" | "signup">("signin");

  // The server call is the form's submit validator, so a rejected sign-in
  // comes back as field and form errors the way TanStack Form reports them.
  const form = createForm(() => ({
    defaultValues: { email: "", password: "" } as Credentials,
    validators: {
      onSubmitAsync: async ({ value }: { value: Credentials }) => {
        try {
          if (mode() === "signup") {
            await auth.createUserWithEmailAndPassword(value.email, value.password);
          } else {
            await auth.signInWithEmailAndPassword(value.email, value.password);
          }
          return undefined;
        } catch (error) {
          return submitErrors(error as FirebaseError);
        }
      },
    },
  }));
  const isSubmitting = form.useStore((state) => state.isSubmitting);
  const formError = form.useStore((state) => state.errors[0]);

  const submit = (next: "signin" | "signup") => {
    mode.set(next);
    void form.handleSubmit();
  };

  const user = auth.currentUser;
  return (
    <Scroll class="flex-1 bg-background">
      <View class="gap-4 p-4 pb-[env(safe-area-inset-bottom)]">
        <View class="rounded-(--radius) border border-border bg-card p-4">
          <Text class="text-sm text-muted-foreground">
            {user() === null ? "Signed out" : `Signed in as ${user()!.uid}`}
          </Text>
          <If when={user()?.email}>
            {(emailText) => <Text class="text-sm text-muted-foreground">{emailText()}</Text>}
          </If>
          <If when={user() !== null}>
            <Button
              variant="outline"
              accessibilityLabel="Sign out"
              class="mt-2 self-start"
              onPress={() => void auth.signOut()}
            >
              <Text class="text-sm font-medium text-foreground">Sign out</Text>
            </Button>
          </If>
        </View>

        <If
          when={user() === null}
          else={
            <Text class="text-sm text-muted-foreground">
              The session persists; reload keeps this user signed in.
            </Text>
          }
        >
          <View class="gap-3">
            <View class="gap-1">
              <Text class="text-sm font-medium text-foreground">Email</Text>
              <form.Field name="email" validators={{ onChange: emailRules }}>
                {(field) => (
                  <View class="gap-1">
                    <Input
                      {...inputProps(field)}
                      placeholder="you@example.test"
                      keyboardType="email-address"
                      autoCapitalize="none"
                      testID="auth-email"
                    />
                    <If when={field().state.meta.errors[0]}>
                      {(message) => (
                        <Text class="text-sm text-destructive" testID="auth-email-error">
                          {String(message())}
                        </Text>
                      )}
                    </If>
                  </View>
                )}
              </form.Field>
            </View>
            <View class="gap-1">
              <Text class="text-sm font-medium text-foreground">Password</Text>
              <form.Field name="password" validators={{ onChange: passwordRules }}>
                {(field) => (
                  <View class="gap-1">
                    <Input
                      {...inputProps(field)}
                      placeholder="At least 8 characters"
                      masked
                      testID="auth-password"
                    />
                    <If when={field().state.meta.errors[0]}>
                      {(message) => (
                        <Text class="text-sm text-destructive" testID="auth-password-error">
                          {String(message())}
                        </Text>
                      )}
                    </If>
                  </View>
                )}
              </form.Field>
            </View>
            <If when={formError()}>
              {(message) => (
                <Text class="text-sm text-destructive" testID="auth-form-error">
                  {String(message())}
                </Text>
              )}
            </If>
            <View class="flex-row gap-2">
              <Button
                accessibilityLabel="Sign in"
                disabled={isSubmitting()}
                onPress={() => submit("signin")}
              >
                <Text class="text-sm font-medium text-primary-foreground">Sign in</Text>
              </Button>
              <Button
                variant="outline"
                accessibilityLabel="Create account"
                disabled={isSubmitting()}
                onPress={() => submit("signup")}
              >
                <Text class="text-sm font-medium text-foreground">Create account</Text>
              </Button>
            </View>
            <Text class="text-xs text-muted-foreground">
              Against the demo-nsh emulator on 127.0.0.1:19099; a fake project with fake
              credentials.
            </Text>
          </View>
        </If>
      </View>
    </Scroll>
  );
}

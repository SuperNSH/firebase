// The app's button: one wrapper over nsh/base's Button; screens never style the base control.
// Text colors stay on the caller's Text: the wrapper styles the control, a Text carries its own token.
import { type JSX } from "nsh";
import { Button as BaseButton } from "nsh/base";

const variants = {
  primary: "bg-primary active:bg-primary/90",
  outline: "border border-border bg-background active:bg-accent",
  ghost: "active:bg-muted",
};

export function Button(props: {
  variant?: keyof typeof variants;
  disabled?: boolean;
  accessibilityLabel?: string;
  testID?: string;
  onPress?: () => void;
  class?: string;
  children?: JSX.Element;
}) {
  const variant = () => props.variant ?? "primary";
  return (
    <BaseButton
      disabled={props.disabled}
      accessibilityLabel={props.accessibilityLabel}
      testID={props.testID}
      onPress={props.onPress}
      class={`min-h-9 flex-row items-center justify-center gap-2 rounded-(--radius) px-4 py-2 transition-colors disabled:opacity-50 focus:outline-2 focus:outline-ring ${variants[variant()]} ${props.class ?? ""}`}
    >
      {props.children}
    </BaseButton>
  );
}

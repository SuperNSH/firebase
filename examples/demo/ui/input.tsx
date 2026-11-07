// The app's text input: one wrapper over nsh/base's Input; screens never
// style the base control. Its props cover nsh-form's inputProps() spread.
import type { InputRef } from "nsh";
import { Input as BaseInput } from "nsh/base";

export function Input(props: {
  value?: string;
  onChange?: (event: { value: string }) => void;
  onBlur?: () => void;
  onSubmit?: () => void;
  placeholder?: string;
  masked?: boolean;
  keyboardType?: "default" | "email-address";
  autoCapitalize?: "none" | "sentences";
  returnKeyType?: "done" | "next";
  disabled?: boolean;
  autoFocus?: boolean;
  testID?: string;
  ref?: (node: InputRef | null) => void;
  class?: string;
}) {
  return (
    <BaseInput
      value={props.value}
      onChange={props.onChange}
      onBlur={props.onBlur}
      onSubmit={props.onSubmit}
      placeholder={props.placeholder}
      masked={props.masked}
      keyboardType={props.keyboardType}
      autoCapitalize={props.autoCapitalize}
      returnKeyType={props.returnKeyType}
      readonly={props.disabled}
      autoFocus={props.autoFocus}
      testID={props.testID}
      ref={props.ref}
      class={`min-h-9 rounded-(--radius) border border-input px-3 py-2 text-foreground placeholder:text-muted-foreground focus:outline-2 focus:outline-ring disabled:opacity-50 ${props.class ?? ""}`}
    />
  );
}

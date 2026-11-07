// The app's progress bar: track and indicator styled once, a height the
// user's text does not clip.
import { Progress as BaseProgress } from "nsh/base";

export function Progress(props: {
  value: number;
  accessibilityLabel?: string;
  class?: string;
}) {
  return (
    <BaseProgress
      value={props.value}
      accessibilityLabel={props.accessibilityLabel}
      class={`h-2 w-full overflow-hidden rounded-full bg-muted ${props.class ?? ""}`}
      trackClass="h-2 w-full rounded-full bg-muted"
      indicatorClass="h-2 rounded-full bg-primary"
    />
  );
}

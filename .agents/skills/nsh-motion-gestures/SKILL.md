---
name: nsh-motion-gestures
description: Choose and write Native Shell (NSH) animation and touch handling that stays on the UI thread, covering motions, transitions, animated values, layout animations, bound pan, pinch and rotation gestures, the responder system, scroll conflicts, and reduced motion; use when adding any animation, drag, swipe, or press feedback to an NSH app, or when one stutters while the app is busy.
---

# NSH motion and gestures

Every NSH animation runs on the UI thread. You describe it once from Rust or TSX, and the platform plays it without calling your code. A busy engine thread cannot make it stutter. The mistake to avoid is writing a signal every frame, which works, looks fine in a demo, and costs a commit per frame. This skill helps you pick the highest-level tool that does the job. Samples were compiled and tested on `nsh` 0.4.0 (e626a8d) on 2026-11-06.

## When to Use

- Adding an entrance, exit, press effect, spring, or looping animation.
- Making something draggable, swipeable, pinchable or rotatable.
- An animation or drag stutters while the app is doing work.
- A drag fights with a scroll view around it.
- Reviewing NSH code for animation done through signals or `requestAnimationFrame`.

Read the official `nsh-design-guides` skill's Motion section first for when something should move at all.

## Pick the tool

Take the first row that fits. Each one below it gives more control and more to get right.

| You want | Tool | TSX | Rust |
|---|---|---|---|
| A style change that animates | Transition | `transition duration-150`, `transitionProperty` | `.transition().duration(150.0)` |
| A spinner or skeleton pulse | Keyframe animation | `animate-spin`, `animate-pulse` | `.animate_spin()`, `.animate_pulse()` |
| A view that enters, leaves or draws the eye | Motion | `createMotion({ enter: "fadeInUp" })` | `motion().entering(MotionPreset::FadeInUp)` |
| Items inserted, removed or reordered | Layout animation | `LayoutAnimation.easeInEaseOut()` | `animate_layout(LayoutAnimation::EASE_IN_EASE_OUT)` |
| A spring, or something driven by a value | Animated value | `createAnimatedValue(0)` | `animated_value(0.0)` |
| Several steps as one | Composite | `Animated.sequence([...])` | `Animation::sequence([...])` |
| Something follows the scroll | `scrollValue` | `scrollValue={y}` | `.scroll_value(&y)` |
| Something follows a finger | Bound gesture | `pan={{ x, y }}` | `.pan_values(Some(&x), Some(&y))` |
| A gesture native recognizers lack | Responder | `onStartShouldSetResponder` | `.claim_touches(TouchClaim::on_start())` |

What runs where, from the [Animation guide](https://nsh.dev/docs/animation.md#what-runs-where):

| Tool | Runs on | Your code per frame |
|---|---|---|
| Transitions, keyframes, motions, `timing`, `spring`, `decay`, composites | UI thread | None; you hear when it ends |
| `interpolate`, arithmetic, `diffClamp` | UI thread | None |
| `scrollValue`, `pan`, `pinch`, `rotation` | UI thread | None; you hear start and end |
| Layout animations | UI thread | None |
| `requestAnimationFrame`, `onScroll`, `PanResponder`, the responder | Your code, then an update | Every frame |

Reach for the last row only for what cannot be described ahead of time, such as a physics simulation in your code.

## Press feedback and transitions

There is no built-in press feedback. Use state styles, which apply natively, and add a transition so the change animates:

```tsx
// press.tsx
import { Text, View } from "nsh";

export function SaveButton(props: { onPress: () => void }) {
  return (
    <View
      onPress={props.onPress}
      role="button"
      class="group rounded-(--radius) bg-primary px-5 py-3 transition duration-150 active:scale-95 active:bg-primary/90"
    >
      <Text class="font-semibold text-primary-foreground group-active:opacity-85">Save</Text>
    </View>
  );
}
```

```rust
// src/press.rs
use nsh_base::theme;
use nsh_core::prelude::*;

pub fn save_button(on_press: impl Fn() + 'static) -> impl IntoElement {
    view()
        .on_press(on_press)
        .role(AccessibilityRole::Button)
        .group()
        .rounded(move || theme().radius)
        .bg(move || theme().primary)
        .px_5()
        .py_3()
        .transition()
        .duration(150.0)
        .active(|s| s.scale(0.95))
        .child(
            text("Save")
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || theme().primary_foreground)
                .group_active(|s| s.opacity(0.85)),
        )
}
```

A transition starts from what is on screen, so changing the target mid-way turns smoothly. Layout properties do not transition: use a layout animation for those.

## Motions

A motion moves a view's opacity and transform through keyframes. The presets are data, so this is the shortest route to a good entrance or a "wrong password" shake. Create the motion in the component that owns the view.

```tsx
// motion.tsx
import { createMotion, LayoutAnimation, signal, Text, View } from "nsh";

function Row(props: { title: string; index: number; onRemove: () => void }) {
  // Cap the stagger so a row added far down the list does not wait.
  const row = createMotion({ enter: "fadeInUp", delay: Math.min(props.index, 10) * 60 });
  const remove = async () => {
    await row.play("fadeOutLeft");
    LayoutAnimation.easeInEaseOut();
    props.onRemove();
  };
  return (
    <View class="rounded-xl bg-card p-4" style={row.style} onPress={remove}>
      <Text class="text-card-foreground">{props.title}</Text>
    </View>
  );
}

export function Rows() {
  const items = signal([
    { id: 1, title: "One" },
    { id: 2, title: "Two" },
    { id: 3, title: "Three" },
  ]);
  return (
    <View class="gap-2 p-4">
      <Each of={items()} key={(item) => item.id}>
        {(item, index) => (
          <Row
            title={item().title}
            index={index()}
            onRemove={() => items.set(items().filter((i) => i.id !== item().id))}
          />
        )}
      </Each>
    </View>
  );
}
```

```rust
// src/motion.rs
use nsh_core::element::LayoutAnimation;
use nsh_core::prelude::*;

pub fn card(items: Signal<Vec<u32>>, id: u32, index: usize) -> impl IntoElement {
    let delay = index.min(10) as f32 * 60.0;
    let row = motion().entering(MotionPreset::FadeInUp.spec().timing(|t| t.delay(delay)));
    let remove = move || {
        spawn(async move {
            row.play(MotionPreset::FadeOutLeft).await;
            animate_layout(LayoutAnimation::EASE_IN_EASE_OUT);
            items.update(|items| items.retain(|i| *i != id));
        });
    };
    view().p_4().rounded_xl().bind_motion(&row).on_press(remove).child(text(format!("Item {id}")))
}
```

`If` and `Each` are globals in TSX, so they take no import. Importing them from `nsh` is a type error.

Useful presets, all 0.4.0 names:

| Group | TSX names | Rust |
|---|---|---|
| Enter | `fadeIn`, `fadeInUp`, `fadeInDown`, `zoomIn`, `popIn`, `slideInUp`, `flipInX` | `MotionPreset::FadeInUp`, ... |
| Exit | `fadeOut`, `fadeOutLeft`, `zoomOut`, `popOut`, `slideOutDown` | `MotionPreset::FadeOutLeft`, ... |
| Attention | `shake`, `headShake`, `pulse`, `heartbeat`, `wiggle`, `tada` | `MotionPreset::Shake`, ... |
| Loop | `spin`, `float`, `breathe`, `sway`, `bob`, `glow` | `MotionPreset::Spin`, ... |

Rules from the guide:

- A motion moves opacity and transform only. Colors move with an animated value and `interpolate`.
- Spreading `m.style` sets the view's `opacity` and `transform`. To keep your own opacity, bind only `transform: m.transform`.
- `play` resolves with `{ finished }`, which is `false` when something interrupted it. `await` it before removing the view, as above.
- Motions follow the user's reduced motion setting on their own: they land where they end, without moving.
- For a composite, `m.animation("fadeInUp")` gives the motion unstarted, ready for `Animated.stagger`. Give those motions a hidden `initial`, or they show before their turn.

## Animated values

An animated value is a number that lives on the UI thread. Bind it to styles, then animate it, and the views follow it every frame with no update.

```tsx
// values.tsx
import { Animated, createAnimatedValue, Easing, Text, View } from "nsh";

export function Values() {
  const x = createAnimatedValue(0);
  const fade = createAnimatedValue(1);
  const color = x.interpolate({ inputRange: [0, 240], outputRange: ["#0969da", "#8250df"] });

  const play = async () => {
    await x.timing(240, { duration: 600, easing: Easing.out(Easing.cubic) });
    await fade.timing(0.3, { duration: 200 });
    x.spring(0, { stiffness: 180, damping: 8 });
    fade.timing(1);
  };

  const pulse = Animated.loop(
    Animated.sequence([
      Animated.timing(fade, { toValue: 0.5, duration: 400 }),
      Animated.timing(fade, { toValue: 1, duration: 400 }),
    ]),
  );

  return (
    <View class="flex-1 justify-center gap-6 p-6">
      <View style={{ width: 60, height: 60, borderRadius: 12, backgroundColor: color, opacity: fade, translate: [x, 0] }} />
      <View onPress={play} role="button">
        <Text>Play</Text>
      </View>
      <View onPress={() => pulse.start()} role="button">
        <Text>Pulse</Text>
      </View>
    </View>
  );
}
```

```rust
// src/values.rs
use nsh_core::prelude::*;

#[component]
pub fn Values() -> impl IntoElement {
    let x = animated_value(0.0);
    let opacity = animated_value(1.0);
    let play = move || {
        spawn(async move {
            x.timing(240.0, 600.0, Easing::Curve(Curve::Cubic, EaseMode::Out)).await;
            opacity.timing(0.3, 200.0, Easing::EaseInOut).await;
            x.spring(0.0, Spring { stiffness: 180.0, damping: 8.0, ..Spring::default() });
            opacity.timing_with(1.0, Timing::default());
        });
    };
    view()
        .flex_1()
        .justify_center()
        .p_6()
        .gap_6()
        .child(view().size(px(60.0)).rounded(px(12.0)).bind_opacity(&opacity).bind_translate_x(&x))
        .child(view().on_press(play).child(text("Play")))
}
```

Things that catch people:

- **Interpolations clamp by default.** Outside the input range the output stays at the nearest end. Pass `extrapolate: "extend"` (`Extrapolate::Extend`) when the line should continue.
- **Reading a value is async.** It moves on the UI thread, so `await x.getValue()` in TSX and `x.value().await` in Rust.
- **Ownership.** Create values in the component that owns them, not at module level when each instance needs its own. A running animation does not keep its values alive, so an endless loop stops when its component goes.
- **A new animation interrupts smoothly.** Awaiters of the old one get `finished: false`.
- **Spring defaults** are stiffness 170, damping 26, mass 1. Lower damping overshoots more.
- **Every velocity is points per second,** so a pan's velocity goes straight into a spring or decay.

## Layout animations

Call one right before the state change. It animates every frame that update changes, anywhere on screen, so a list refreshing in the same update moves with it.

```tsx
// layout.tsx
import { LayoutAnimation, signal, Text, View } from "nsh";

export function Reorder() {
  const items = signal([1, 2, 3]);
  const shuffle = () => {
    LayoutAnimation.configureNext(LayoutAnimation.Presets.spring);
    items.set([...items()].reverse());
  };
  return (
    <View class="gap-2 p-4" onPress={shuffle}>
      <Each of={items()}>{(n) => <Text>{`Item ${n()}`}</Text>}</Each>
    </View>
  );
}
```

In Rust, `ElementRef::animate_layout(transition)` limits the animation to what one element's change moves: its own frame and those of the views its size pushes along. Use it for a collapsing panel next to live data.

## Gestures: bind, do not follow

Touch handling has three levels, and the [Touch and gestures guide](https://nsh.dev/docs/touch-and-gestures.md) says to pick the highest that does the job.

Bind values to the gesture for anything that only moves things. The recognizer writes the values on the UI thread, and the view moves in the same frame, even while your code is busy.

```tsx
// swipe.tsx
import { createAnimatedValue, Text, View } from "nsh";

const ACTION_WIDTH = 96;

export function SwipeRow(props: { title: string; onDelete: () => void }) {
  const x = createAnimatedValue(0);
  return (
    <View class="overflow-hidden">
      <View class="absolute top-0 right-0 bottom-0 w-24 items-center justify-center bg-destructive" onPress={props.onDelete}>
        <Text class="text-sm text-primary-foreground">Delete</Text>
      </View>
      <View
        pan={{ x }}
        onPanEnd={(e) => {
          // Project where the fling lands, then settle on a snap point.
          const projected = (e.value?.x ?? 0) + e.velocity.x * 0.2;
          x.spring(projected < -ACTION_WIDTH / 2 ? -ACTION_WIDTH : 0, { velocity: e.velocity.x });
        }}
        class="bg-card p-4"
        style={{ translate: [x, 0] }}
      >
        <Text class="text-card-foreground">{props.title}</Text>
      </View>
    </View>
  );
}
```

```rust
// src/swipe.rs
use nsh_core::prelude::*;

const ACTION_WIDTH: f32 = 96.0;

pub fn swipe_row(title: &'static str) -> impl IntoElement {
    let x = animated_value(0.0);
    view()
        .pan_values(Some(&x), None)
        .on_pan_end(move |e: &PanEvent| {
            let projected = e.value_x.unwrap_or(0.0) + e.velocity_x * 0.2;
            let to = if projected < -ACTION_WIDTH / 2.0 { -ACTION_WIDTH } else { 0.0 };
            x.spring(to, Spring { velocity: Some(e.velocity_x), ..Spring::default() });
        })
        .bind_translate_x(&x)
        .p_4()
        .child(text(title))
}
```

What the native side handles for you:

- **A drag continues from where the value is.** Grabbing a value mid-spring stops the spring there.
- **Binding one axis lets the other scroll.** With only `x` bound, the drag begins only when the finger moves more horizontally than vertically, so this row works inside a vertical list. This is decided natively.
- **Pan, pinch and rotation run together** on one view: `pinch={{ scale }}` and `rotation={{ angle }}`, with angle in degrees.

## Responder and scroll conflicts

Use the responder system only when your code must own the touch sequence: finger drawing, a custom slider, a gesture no recognizer offers. Every move is an event to your code, so it is only as smooth as your code is fast.

A chart scrubbed sideways inside a vertical scroll view is the common hard case. Claiming on start blocks scrolling from the first pixel, and deciding in your code is too late, because the scroll view may already have taken the touch. Pan activation lets the platform decide:

```tsx
// scrub.tsx
import { signal, Text, View } from "nsh";

export function Scrub() {
  const at = signal<number | null>(null);
  return (
    <View
      panActivation={{ activeOffsetX: 10, failOffsetY: 10 }}
      onResponderGrant={(e) => at.set(e.locationX)}
      onResponderMove={(e) => at.set(e.locationX)}
      onResponderRelease={() => at.set(null)}
      class="h-40 bg-muted"
    >
      <Text>{at() === null ? "Drag to scrub" : `At ${Math.round(at()!)}`}</Text>
    </View>
  );
}
```

```rust
// src/scrub.rs
use nsh_core::prelude::*;

pub fn scrub() -> impl IntoElement {
    let at = signal(None::<Points>);
    view()
        .claim_touches(TouchClaim::on_pan(PanActivation::horizontal(px(10.0), px(10.0))))
        .on_responder_grant(move |e: &ResponderEvent| at.set(Some(e.location.x)))
        .on_responder_move(move |e: &ResponderEvent| at.set(Some(e.location.x)))
        .on_responder_release(move |_: &ResponderEvent| at.set(None))
        .h(px(160.0))
        .child(text(move || match at.get() {
            Some(x) => format!("At {:.0}", x.0),
            None => "Drag to scrub".to_owned(),
        }))
}
```

Other touch facts worth keeping in mind:

- Only the innermost pressable view responds. Inside a scroll view a press waits about 130 ms, so a drag that starts on a button scrolls.
- Hit slop works only inside the parent's bounds, because hit testing reaches a child through its parent.
- `pointerEvents: "box-none"` lets a full-screen layer's empty space pass touches through while its buttons keep working.
- A prop such as `popOnBack` or `panActivation` is a prop rather than a callback because the main thread never waits for your code.
- In TSX, returning `true` from `onResponderGrant` keeps native scroll views from taking the touches. Rust blocks them by default.

## Reduced motion

Motions follow the setting by themselves. Everything else plays as written, so choose a calmer version yourself:

```tsx
// reduced.tsx
import { createAnimatedValue, reduceMotion, View } from "nsh";

export function Panel() {
  const panel = createAnimatedValue(320);
  const open = () => (reduceMotion() ? panel.set(0) : panel.spring(0));
  return (
    <View onPress={open} class="motion-safe:animate-bounce" style={{ translate: [panel, 0] }} />
  );
}
```

The `motion-safe:` and `motion-reduce:` classes follow the setting, and Rust has `reduce_motion()`. Pass `ignoreReduceMotion: true` only for motion that carries meaning the user needs, such as a progress spinner.

Motion with a purpose should still leave a clear state behind when it does not play. Check that by opening with `reducedMotion: true` in a test.

## Test it

Native animations run headless, frame by frame, to their end, and a manual clock makes them take no real time. A `drag` step exercises a bound pan. Read the end state; a headless run does not show you how it looked.

```tsx
// App.tsx
import { createMotion, mount, reduceMotion, signal, Text, View } from "nsh";

function App() {
  const removed = signal(false);
  const row = createMotion({ enter: "fadeIn" });
  const remove = async () => {
    await row.play("fadeOutLeft");
    removed.set(true);
  };
  return (
    <View class="flex-1 gap-4 p-4">
      <If when={!removed()} else={<Text>Removed</Text>}>
        <View style={row.style} onPress={remove} role="button" accessibilityLabel="Row">
          <Text>Row</Text>
        </View>
      </If>
      <Text>{reduceMotion() ? "calm" : "moving"}</Text>
    </View>
  );
}

mount(App);
```

```tsx
// App.test.tsx
import { expect, open, test } from "nsh/test";

test("the row leaves once its exit finishes", async () => {
  const app = await open("./App.tsx", { clock: {} });
  await app.press("label:Row");
  await app.waitFor("text:Removed");
  expect(app.shows("label:Row")).toBe(false);
});

test("reduced motion still ends removed", async () => {
  const app = await open("./App.tsx", { reducedMotion: true, clock: {} });
  expect(app.texts()).toContain("calm");
  await app.press("label:Row");
  await app.waitFor("text:Removed");
});
```

## Copy these rules into the project

```markdown
## NSH motion and gesture rules
- No signal is written every frame to animate. Use a transition, motion or animated value.
- No requestAnimationFrame or onScroll for what an animated value or scrollValue can do.
- Drags that only move views bind values (pan, pinch, rotation). The responder is for owning the sequence.
- Interactive controls have pressed styles. There is no built-in press feedback.
- Exits await play() before removing the view. Layout animations are called right before the change.
- Animated values are created in the component that owns them.
- Interpolations that must continue past their range set extrapolate explicitly.
- Non-motion animations check reduceMotion(), and a test opens with reducedMotion: true.
- Motion is checked on a device or a real --target. Headless proves end states only.
```

## Review checklist

- [ ] No signal is written from a timer or frame callback to animate.
- [ ] The tool chosen is the highest in the table that does the job.
- [ ] Pressable views have an `active` style.
- [ ] Horizontal drags in a vertical scroll bind one axis, or use `panActivation`.
- [ ] Exits wait for `play()` before the view goes.
- [ ] Reduced motion has a calm path outside of motions.
- [ ] Someone looked at it on a real target.

## Grounded in

- NSH Animation guide, tools, presets, animated values, composites, what runs where: https://nsh.dev/docs/animation.md
- NSH Touch and gestures guide, presses, bound gestures, the responder, pan activation: https://nsh.dev/docs/touch-and-gestures.md
- NSH Performance guide, animate natively: https://nsh.dev/docs/performance.md#5-animate-natively
- NSH Scrolling guide, `scrollValue`: https://nsh.dev/docs/scrolling.md
- NSH Accessibility guide, reduced motion: https://nsh.dev/docs/accessibility.md#reduced-motion
- NSH Testing guide, what headless runs play of the platform, manual clocks: https://nsh.dev/docs/testing-and-trace.md
- Official `nsh-design-guides` skill, Motion section: https://github.com/longbridge/nsh/tree/main/skills/nsh-design-guides

---
name: nsh-lists-performance
description: Build long and endless lists in Native Shell (NSH) that survive cell recycling, and measure NSH app performance with mount-item counts from --trace rather than guesses, covering row identity, per-item state, chat and paging patterns, scroll-linked effects, and CI budgets; use when writing or debugging List or virtual_list, when rows show the wrong data after scrolling, or when an NSH app feels slow.
---

# NSH lists and performance

NSH's list reuses a small pool of cells, and most list bugs come from treating it as one component per item. NSH's performance work also has a different shape from React's: a component runs once, so the question is not "what re-rendered" but "how many native operations did this tap produce". That number is deterministic and easy to read. This skill covers both. Samples were compiled and run on `nsh` 0.4.0 (e626a8d) on 2026-11-06, and the counts quoted under "Read the output" were measured on that build.

## When to Use

- Writing `List` (TSX) or `virtual_list` (Rust), including chat, feeds and grids.
- Rows that show another row's data, lose state, or flicker after scrolling.
- Choosing between `List`, `Each` and a plain `.map()`.
- An NSH app feels slow, or a change might have made it slower.
- Adding a performance guard to CI.

## Pick the right container

| Data | TSX | Rust | Why |
|---|---|---|---|
| Can grow without a known limit: chat, feed, history | `List` | `virtual_list` | Only rows near the screen exist |
| Small and bounded, needs its rows kept | `Each` | `each` | Rows kept by key, all created |
| Never changes after the first render | `.map()` | `dynamic` or `children` | Fine once; rebuilds everything on any change |
| Page that scrolls as one, with a long section | `List viewport="parent"` in `Scroll` | `.viewport(ListViewport::Parent)` | The page scrolls, and the list still recycles |

Per the [Performance guide](https://nsh.dev/docs/performance.md), appending a fourth item to a three-item collection costs 22 mount items with `.map()` and 4 with `Each`. The official `nsh` skill's rule is to use a virtualized list by default for anything that can grow, including chat and history.

## The two recycling rules

The row function runs **once per cell**, not once per item. When a row scrolls far away, its cell is handed another row, and only the bindings that read the position update. From the [Lists guide](https://nsh.dev/docs/lists.md#how-recycling-works):

1. **Read the position where you use it.** A value copied out at the top of the row function stays with whichever row the cell showed first.
2. **Keep per-item state in your data, keyed by id.** A signal made inside a row belongs to the cell and travels with it to the next item.

This row breaks both rules and passes a casual look, because the first screen renders correctly:

```tsx
// wrong.tsx
import { List, signal, Text, View } from "nsh";

type Post = { id: number; title: string };

export function Feed(props: { posts: Post[] }) {
  return (
    <List each={props.posts} estimatedItemSize={64} style={{ flex: 1 }}>
      {(post) => {
        const title = post().title; // Rule 1: stuck on the first item this cell showed.
        const liked = signal(false); // Rule 2: follows the cell to other posts.
        return (
          <View class="flex-row p-4" onPress={() => liked.set(!liked())}>
            <Text class="flex-1">{title}</Text>
            <Text>{liked() ? "♥" : "♡"}</Text>
          </View>
        );
      }}
    </List>
  );
}
```

The fix reads `post()` inside the JSX and keeps likes in a set keyed by id, with a new `Set` per change:

```tsx
// right.tsx
import { List, signal, Text, View } from "nsh";

type Post = { id: number; title: string };

export function Feed(props: { posts: Post[] }) {
  const liked = signal<Set<number>>(new Set());
  const toggle = (id: number) =>
    liked.update((s) => {
      const next = new Set(s);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  return (
    <List each={props.posts} estimatedItemSize={64} style={{ flex: 1 }}>
      {(post) => (
        <View class="flex-row p-4" onPress={() => toggle(post().id)}>
          <Text class="flex-1">{post().title}</Text>
          <Text>{liked().has(post().id) ? "♥" : "♡"}</Text>
        </View>
      )}
    </List>
  );
}
```

The same in Rust, where the row gets its position as `at.index()`:

```rust
// src/feed.rs
use std::collections::HashSet;

use nsh_core::prelude::*;

#[derive(Clone, PartialEq)]
pub struct Post {
    pub id: u32,
    pub title: String,
}

pub fn feed(posts: Signal<Vec<Post>>) -> impl IntoElement {
    let liked = signal(HashSet::<u32>::new());
    let toggle = move |id: u32| {
        liked.update(|s| {
            if !s.remove(&id) {
                s.insert(id);
            }
        })
    };
    virtual_list(move || posts.with(Vec::len), move |at| {
        // A computed reads the position, so it follows the cell's current row.
        let post = computed(move || posts.with(|p| p[at.index()].clone()));
        view()
            .flex_row()
            .p_4()
            .child(text(move || post.with(|p| p.title.clone())).flex_1())
            .child(
                text(move || if liked.with(|s| s.contains(&post.with(|p| p.id))) { "♥" } else { "♡" })
                    .on_press(move || toggle(post.with(|p| p.id))),
            )
    })
    .keys(move || posts.with(|p| p.iter().map(|p| p.id).collect::<Vec<_>>()))
    .estimated_item_size(px(64.0))
    .flex_1()
}
```

Prove rule 2 with a test that scrolls past the first screen and back. A list that only renders its first screen in tests hides every recycling bug:

```tsx
// App.tsx
import { List, mount, signal, Text, View } from "nsh";

const posts = Array.from({ length: 500 }, (_, i) => ({ id: i, title: `Post ${i}` }));

function App() {
  const liked = signal<Set<number>>(new Set());
  const toggle = (id: number) =>
    liked.update((s) => {
      const next = new Set(s);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  return (
    <List testID="feed" each={posts} estimatedItemSize={64} style={{ flex: 1 }}>
      {(post) => (
        <View class="flex-row p-4" onPress={() => toggle(post().id)}>
          <Text class="flex-1">{post().title}</Text>
          <Text>{liked().has(post().id) ? "♥" : "♡"}</Text>
        </View>
      )}
    </List>
  );
}

mount(App);
```

```tsx
// App.test.tsx
import { expect, open, test } from "nsh/test";

test("a like stays with its post after recycling", async () => {
  // A manual clock: this test took 13.7 s on the system clock, and 18 ms on this one.
  const app = await open("./App.tsx", { clock: {} });
  await app.press("text:Post 2");
  expect(app.count("text:♥")).toBe(1);

  await app.scroll("id:feed", 4000);
  expect(app.shows("text:Post 2")).toBe(false);
  // Whatever cells are on screen now, none of these posts was liked.
  expect(app.count("text:♥")).toBe(0);

  await app.scroll("id:feed", -4000);
  await app.waitFor("text:Post 2");
  expect(app.count("text:♥")).toBe(1);
});
```

We ran the wrong version through this test on 0.4.0 and the second assertion failed with `expected 1 to be 0`, with the heart showing on `Post 108`. The heart had moved with the cell.

## Changing the data

TSX compares arrays **by item identity** (`===`), keeps the common start and end, and treats the middle as removed and inserted. So:

- Update immutably: `posts.set([...posts(), ...more])`. A `push` on the same array is not seen.
- A changed item must be a new object, `{ ...post, title }`, or its row keeps showing the old one.
- `keyExtractor` names items in viewability tokens only. It does not affect how changes are matched in TSX.

Rust gives the count as a signal or closure. Rows added or removed at the end need nothing more. When rows come and go elsewhere, give their identities with `.keys(..)`, as in the sample above. In both languages measurements stay with their rows, and rows inserted above the viewport do not move what is on screen.

## List patterns

Give `estimatedItemSize` close to your typical row: it drives the scroll indicator and jumps to far items. The default is 50 points. The list is the viewport, so it needs a size from its parent (`flex-1` or a height), and a `List` with no height shows nothing.

| Need | TSX | Rust |
|---|---|---|
| Chat, newest at the bottom | `inverted` | `.inverted(true)` |
| Load older pages | `onLoadMore`, `loadMoreThreshold` | `.on_load_more(..)`, `.load_more_threshold(..)` |
| Load newer pages after opening mid-history | `onLoadMoreAtStart` | `.on_load_more_at_start(..)` |
| Grid | `numColumns` | `.columns(n)` |
| Section headers | `stickyHeaderIndices` | `.sticky_header_indices(..)` |
| Stick to the end while growing, not inverted | `maintainVisibleContentPosition={{ autoscrollToBottomThreshold: 40 }}` | `.follow_end(px(40.0))` |
| Open at an item | `initialScrollIndex` | `.initial_scroll_index(i)` |
| Jump to an item | `ref.scrollToIndex({ index, viewPosition })` | `list.scroll_to_index(i, percentage(0.5), px(0.0), true)` |
| Analytics, autoplay | `onViewableItemsChange`, `viewabilityConfig` | `.on_viewable_items_change(..)`, `.viewability(..)` |
| Empty state | `empty` | `.empty(..)` |

`separator` applies to one-column lists. `stickyHeaderIndices` needs one column and not inverted. `viewport` is chosen when the list is made, and switching means rendering a new list.

Guard against overlapping loads, because the end may be reported again while a page is on its way:

```tsx
// paging.tsx
import { List, signal, Text } from "nsh";

type Post = { id: number; title: string };

export function PagedFeed() {
  const posts = signal<Post[]>([]);
  let loading = false;
  let page = 0;
  const loadMore = async () => {
    if (loading) return;
    loading = true;
    try {
      const response = await fetch(`https://api.example.com/posts?page=${++page}`);
      const next: Post[] = await response.json();
      posts.set([...posts(), ...next]);
    } finally {
      loading = false;
    }
  };
  loadMore();
  return (
    <List
      each={posts()}
      estimatedItemSize={72}
      class="flex-1"
      onLoadMore={loadMore}
      loadMoreThreshold={0.5}
      footer={<Text class="p-4 text-center text-muted-foreground">Loading…</Text>}
    >
      {(post) => <Text class="p-4">{post().title}</Text>}
    </List>
  );
}
```

For a chat that opens mid-history, `onLoadMore` loads older messages and `onLoadMoreAtStart` loads newer ones. Once the newest message is in, stop loading: new messages then arrive at the start as in any chat.

## Effects that follow the scroll

Use `scrollValue` (`scroll_value`) rather than `onScroll` for anything that tracks the scroll position. It writes the offset into an animated value in the same frame, with no call into your code. A hiding header needs no handler at all:

```tsx
// header.tsx
import { createAnimatedValue, List, Text, View } from "nsh";

export function HidingHeader(props: { items: { id: number; title: string }[] }) {
  const scroll = createAnimatedValue(0);
  const offset = scroll.diffClamp(0, 56, { snap: true });
  return (
    <View class="flex-1">
      <List each={props.items} scrollValue={scroll} estimatedItemSize={48} class="flex-1" contentStyle={{ paddingTop: 56 }}>
        {(item) => <Text class="p-4">{item().title}</Text>}
      </List>
      <View
        class="absolute top-0 right-0 left-0 h-14 justify-center bg-background px-4"
        style={{ translate: [0, offset.mul(-1)] }}
      >
        <Text class="font-semibold text-foreground">Inbox</Text>
      </View>
    </View>
  );
}
```

## How NSH spends time

Keep this model in mind when reading a trace. From the [Performance](https://nsh.dev/docs/performance.md) and [Architecture](https://nsh.dev/docs/architecture.md) pages:

- App code runs on an engine thread, never the main thread.
- One turn, meaning one handler and everything it changes, becomes **one commit** and one batch to the main thread. Ten `set` calls in one handler are still one commit, so you do not need to coalesce writes for performance. Use `batch` when effects must see all the writes at once.
- Components run once. Only bindings that read a changed signal run again.
- Animations, `scrollValue` and bound gestures run on the UI thread and cost your code nothing per frame.
- In TSX, QuickJS is an interpreter with no JIT. Heavy work per keystroke costs more than it would in V8, so cache in `computed` or move it into a Rust crate.
- A scroll frame that reveals no new row runs none of your code and produces no batch.

## Measure with --trace

Trust counts over microseconds. Mount items are deterministic; timings depend on the CPU's clock state.

```bash
echo 'await app.press("text:Post 2")' | nsh test App.tsx --trace
```

```bash
echo 'await app.scroll("id:feed", 3000)' | nsh test App.tsx --trace
```

### Read the output

These lines are from the app above, on this build:

```text
perf event js: 90µs
perf commit: 35 ops, 18µs (list js 0µs), 1 mount items, 212 nodes
── press(text:Post 2): 1 items
   Text { id: 36, text: "♥", content: 8x17 @ 0,0 }
```

That is a good result: one heart changed, and one mount item reached the native side. The 35 ops are expected here, because every live row reads the shared `liked` set, so each of those bindings runs again and only the one that changed sends anything.

| Field | Meaning | Look for |
|---|---|---|
| `ops` | Operations JS queued | Grows with the bindings that ran, so read it next to mount items |
| `mount items` | Native view operations | Should match what visibly changed |
| `list js` | JS spent binding rows | High means rows are expensive |
| `event js` | JS cost of a handler | High means work to move or cache |
| `nodes` | Nodes alive | Should stay flat while scrolling a recycled list |
| `Create`, `Delete` under a press | Views made or freed | A tap that recreates views means a binding is wrong |

A press that changes one heart and produces 40 items is the thing to chase. The usual causes:

- A `.map()` or `dynamic` over a collection that changes: use `Each` or `List`.
- A ternary in a closure that rebuilds a branch: use `<If>` or `when`.
- A signal written every frame to animate: use an animated value.
- An `onScroll` handler driving layout: use `scrollValue`.

On a device, `nsh run --target android --trace --release` logs the same commit lines with the platform's own text engine. `nsh run --target macos --fps --release` shows frame rate, CPU, GPU and memory over the running app. In a Rust test, set `config.trace = true` and call `app.observe(..)`, as in the [Testing guide](https://nsh.dev/docs/testing-and-trace.md#tracing-with---trace).

Timing rules: use release builds, compare the minimum of several runs, and stay on one machine. Headless text is measured with an approximate engine, so text sizes differ slightly from a device, while view counts and operation counts stay the same.

## Guard it in CI

The guide's advice is to record item counts for a few key interactions and fail when they grow. Counts are deterministic, and timings on shared CI machines are not. There is no built-in budget flag on 0.4.0, so a small script does it:

```bash
n=$(echo 'await app.press("text:Post 2")' | nsh test App.tsx --trace 2>&1 | sed -n 's/^── press(text:Post 2): \([0-9]*\) items.*/\1/p'); test "$n" -le 3 || { echo "press cost $n mount items, budget 3"; exit 1; }
```

Pick the budget from today's count. Raise it only in a commit that explains why.

## Copy these rules into the project

```markdown
## NSH list and performance rules
- Collections that can grow use List or virtual_list. Each is for small bounded sets; .map() for static ones.
- Rows read their position inside bindings. Nothing is copied out at the top of a row function.
- No signal is created inside a row. Per-item state lives in data keyed by id.
- Changed items are new objects, changed arrays are new arrays.
- Lists set estimatedItemSize near the real row height.
- Tests scroll past the first screen and back, then check row state.
- Scroll-linked effects use scrollValue, animations use animated values. No per-frame signal writes.
- No heavy work per keystroke or per scroll event. Cache in computed, or move it to Rust.
- A performance claim comes with --trace counts, before and after.
```

## Review checklist

- [ ] No value is read from `item()` or `at.index()` outside a binding.
- [ ] No `signal` is created inside a row function.
- [ ] TSX list updates make new arrays and new item objects.
- [ ] Rust lists whose rows change mid-list set `.keys(..)`.
- [ ] Loaders guard against overlapping calls.
- [ ] A test scrolls past the first screen and back.
- [ ] A tap's `mount items` count matches what visibly changed.
- [ ] Timing comparisons used release builds and several runs.

## Grounded in

- NSH Lists guide, recycling, props, chat, paging and viewability: https://nsh.dev/docs/lists.md
- NSH Performance guide, the six rules, `--trace` and `--fps`: https://nsh.dev/docs/performance.md
- NSH Testing guide, trace output, mount items and CI: https://nsh.dev/docs/testing-and-trace.md
- NSH Architecture, threads and commits: https://nsh.dev/docs/architecture.md
- NSH Animation guide, `diffClamp` and `scrollValue`: https://nsh.dev/docs/animation.md
- NSH List element reference: https://nsh.dev/elements/list.md
- Official `nsh` skill, non-negotiables on virtualized lists: https://github.com/longbridge/nsh/tree/main/skills/nsh

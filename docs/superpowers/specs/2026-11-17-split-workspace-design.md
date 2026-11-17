# Split workspace: snap and polish

Date: 2026-11-17
Status: approved for implementation after the interactive preview and user's "continue"

## Intent and decisions

OpenRemote should let a person work with several agents without fighting pane
controls. The user selected **Snap and polish**: Windows-like grab, preview, and
release within the app, together with fixes that stop split view feeling fragile.
They called out repeated pane layout controls as redundant and requested one
icon in the bottom of the sidebar. Keep the existing dark theme and icon family.

## Approaches

1. **Snap and polish (selected).** Preserve the tested layout tree and add clear
   docking, swapping, presets, maximize/restore, and reliable pane rendering.
   More work than cosmetic fixes, but addresses rearrangement and fragility.
2. **Polish existing controls only.** Fix responsive content and control sizes,
   retaining menu-based arrangement. Smaller change, but misses the requested
   grab-and-release experience.

Do not build an OS window manager: no floating windows, overlapping panes, or
cross-window dragging. This is a docked workspace with up to four panes.

## One place for layouts

- Add a recognizable layout/grid icon to the sidebar footer, with tooltip and
  accessible name **Workspace layout**. It opens a compact anchored panel above
  the footer. On a desktop with the sidebar hidden, retain a discoverable route
  through the window's existing split control and command palette.
- The panel offers visual presets: two columns, two rows, main pane with a right
  stack, and a four-pane grid. For three panes offer the main/stack shape; for
  four panes also offer even columns/rows when space permits. Include equalize,
  save layout, and existing saved layouts. Single-pane users can add panes from
  a preset; new slots contain independent New chat composers.
- Applying a preset rearranges existing pane identities in reading order and
  adds only the missing slots. A preset with fewer slots than open panes is
  disabled with a short explanation; it must not silently close a draft.
- Remove repeated **Layouts**, direct split, and split-overflow buttons from
  each pane header. No second workspace toolbar above the panes.
- Keep current shortcuts and manual split actions available through this panel,
  pane context menus, and palette. Existing named layouts remain compatible.

## Pane chrome

- A compact header identifies the pane by App mark, chat title, project when
  space permits, and existing running/waiting/reconnecting status. New chat
  gets a quiet label; avoid repeating the full ChatView title block immediately
  beneath a pane header in split mode.
- Header title area is the drag handle. Add a subtle grip on hover/focus and a
  grab cursor, without giving the whole transcript a drag affordance.
- Header actions are maximize/restore and close, shown on pane hover, keyboard
  focus, and while their menu is open. Use 32px desktop targets and 18-20px
  glyphs. On touch, actions remain visible. Context menu supplies split/move
  actions for people who do not drag.
- Active pane has a clear but quiet accent at its header. Both pointer and
  keyboard focus select it. Sidebar navigation targets the active pane;
  selecting a chat already visible focuses its pane instead of duplicating it.
- Every pane can close when more than one exists, including the original pane.
  Closing a pane never archives/stops its chat. If the navigation-owning pane
  closes, promote an adjacent survivor and preserve its view and composer scope.
  The final pane has no close action. Closing a pane is distinct from maximizing.

## Grab, preview, release

- Dragging starts after a small threshold on the title handle, so ordinary
  clicks do not rearrange anything. Every pane can move, including the original
  pane and empty New chat panes. Drag the pane identity, not a copy of its chat.
- During dragging, display a lightweight title ghost and clearly distinguish
  the source. Show a target overlay only on a valid pane under the pointer:
  center **Swap**, edges **Dock left/right/above/below**.
- The preview shows the actual resulting destination rectangle before release,
  with a short action label. Center swaps the two pane positions; edges remove
  the source from its old position and dock it beside the target. Pane count
  does not grow. Rearranging still works at the four-pane cap.
- Commit once on release. Escape, lost capture, cancellation, or releasing
  outside a valid target leaves the arrangement unchanged and clears all tint.
  Dropping onto the source is a no-op. No hover-only state is persisted.
- Sidebar chat dragging remains supported. Center opens in the target pane;
  an edge adds a pane when capacity and space allow. A chat already visible
  focuses its existing pane on center and moves that pane on an edge. Previews
  must match the committed action, especially at capacity.
- Reuse the tested pure layout model; keep geometry, drop decisions, and tree
  transformations independently testable. Pointer-based internal pane dragging
  is a builder choice to make threshold, cancellation, and empty-pane moves
  deterministic. Sidebar's existing drag payload remains supported.

## Reliability and readable content

- Render pane contents with stable sibling identities, separate from the
  changing nesting of the layout tree. Moving, splitting another pane,
  swapping, resizing, maximizing, or applying a preset must not remount surviving
  chats/composers. Preserve drafts, attachments, model/project selections,
  transcript scroll position, and expanded file cards.
- Maximize is temporary visibility of one pane, not deleting its neighbors.
  Hidden panes retain state and stream subscriptions; they are not tabbable or
  clickable. Restore returns to the previous arrangement and proportions.
- Fix the measured 404px composer overflowing a 323px pane. Use pane/container
  width for responsive spacing, wrapped greetings, truncation, and composer
  controls. Narrow panes can use a second controls row; send and attachment
  actions remain available without horizontal clipping.
- Preserve the existing desktop 240px minimum per pane on the horizontal axis;
  use a sensible vertical floor sufficient for header and composer. Check
  geometry against the actual workspace after sidebar/browser docks, not the
  whole window. Do not offer an impossible snap/preset. Window shrink preserves
  the pane set, uses compact content, and keeps maximize available.
- Divider dragging paints positions without re-rendering transcripts at pointer
  rate and commits once. Pointer cancellation rolls back. Dividers support
  keyboard arrows, visible focus, and double-click equalize.
- Existing stored layout/session data restores safely. Keep the mobile
  single-pane behavior. Honor reduced motion and use the existing ContextMenu
  engine for action menus. No new dependencies or daemon changes are expected.

## Validation

- Pure tests cover rearrangement of original/empty panes, swap versus dock,
  nested tree normalization, four-pane moves, no-ops, primary close/promotion,
  preset identity preservation, geometry refusal, and canceled gestures.
- Browser checks cover actual header dragging and preview/release, all preset
  shapes, zoom/restore, primary and secondary close, sidebar opening behavior,
  keyboard operation, and compact panel dismissal.
- In a narrow split, both new and existing chat composers fit; attachments and
  send remain reachable. Verify at the current 909px window and a wider desktop.
- Verify local UI state survives rearrangement with an unsent draft, a selected
  model/project, an expanded file card, and transcript scroll. Use the sample
  backend; no paid agent turns are needed.
- Run the relevant tests, full console suite, production frontend build, and
  native debug build. Review the diff without reverting other in-flight work.

## Builder discretion

Choose exact icon, spacing within these bounds, overlay arrangement, responsive
breakpoints, vertical floor, and internal module boundaries to fit the existing
app. Add no animation beyond brief action feedback. Preserve compatible APIs
where useful; update tests that intentionally encoded the replaced behavior.
Do not commit unrelated existing edits or install over the running app.

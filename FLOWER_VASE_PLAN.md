# Flower vases and shopping plan

## Intended behavior

- A native 16x16 transparent flower-vase sprite, used by the placed `flower vase` prop and its carried item. Both final sprites are installed and visually verified at the final resolution.
- A placed vase permanently emits +1 pleasant scent per existing 30-world-second scent step. It uses the existing diffusion, doors, decay and mental-health response; it does not wither or require power.
- Some noble flats start with one vase: a deterministic 50% chance per eligible property after initial residents receive their class and home. Use a separate random stream so decoration does not change identities, jobs or the noble lottery.
- Commoners with a home may shop on an actual scheduled day off. Unemployed commoners also qualify. Being between shifts or on medical leave does not count as a day off. Sleeping, illness, danger and urgent needs take precedence.
- Interpret `decor section (new tile)` as an editor-placeable furniture tile called `decor section`: a shelf in the existing store, with a feature used from beside it. This follows the store's counter/superfridge architecture rather than assigning shopping behavior to terrain.
- A shopping trip purchases one vase, carries it home and places it. The limit is two per flat, shared by every resident, including noble starter vases and purchases still being brought home.
- Price: $20 per vase, editable through the existing economy tariffs. Decoration requires sufficient cash; do not create debt for a discretionary purchase. Decor stock is unlimited initially; manufacturing, deliveries and additional store staffing are outside this change.

## Findings from the implementation

- `src/sim/outposts.rs` owns construction plans and claims. `BuildGoal` queues claim/fetch/lay/construct tasks; `effect::apply` rechecks ownership, distance, occupancy and materials before updating the map. Completed fixtures enter facility indexes through `Facilities::install`.
- Reuse that task/effect boundary and placement guards. A vase needs no outpost plan, builder job, steel requirement or two-hour build progress.
- `src/map/district/store.rs` builds the functional public store. It currently places only a shop counter and superfridge. Add the shelf there, with free approaches, and update layout/catalogue assertions.
- `Shift::works_on` supplies the weekly schedule; `is_due` includes commute time and is insufficient to identify a whole day off. Preserve the midnight-shift commute exception so a shopping trip cannot compete with tomorrow's shift.
- Existing fridge purchases use `Effect::Buy`, allow debt and update food reserves. The vase transaction must use the shared money/bookkeeping APIs without entering the food-supply path.
- Several goals consume whatever occupies the hand before taking food. Keep an interrupted vase in normal stowed inventory so another need cannot accidentally consume/delete it. The purchase receipt remains pending until placement; any visible carry presentation must preserve that rule.
- `Properties` supplies the actual home key; use this rather than spawn location or family to count a flat's vases.
- Ordinary map props are drawn by `editor::props::sync_prop_window` from `CurrentMap`, gated by visible rectangle/dirty state. Dynamic placement must draw the simulation map and trigger a refresh even with a stationary camera. Outposts demonstrate dynamic drawing but should remain a separate subsystem.
- Placed site objects already persist through `Map::save_site`; inventory saves item names. Only purchases in transit need new saved shared state. Rebuild derived decoration/scent indexes on load.

## Chosen implementation

1. **Content and store.** Register the vase prop, vase item/art, and decor-section prop/art/feature. Append stable catalogue IDs. Add the shelf to generated stores in all orientations, preserving the counter, reserve, doorway and approach cells. Add a named vase tariff with a backward-compatible default for older configs and saves.
2. **Household decoration index.** Add a small plain-Rust facility (for example `sim/decor.rs`). Derive per-property installed vase cells, room cells and safe placement candidates from the map. Keep pending purchases keyed by resident UID, with destination property and reserved cell. Reads by a routine are indexed counts; no routine scans the map or crowd.
3. **Safe placement.** Pick free ground against an interior wall, excluding doors, occupied cells, corpses, furniture and reserved cells. Keep the room connected and preserve access to existing furniture, including the generator's two-free-neighbor rule. Analyze only the affected room on purchase/placement, with deterministic ordering. Skip cramped homes. Noble starter vases use the same placement code, after residents arrive and before simulation ticks.
4. **Atomic purchase.** Add a short buy-vase task that requests a world effect. The world validates the shelf and reach, current class/home/day-off eligibility, cash, inventory capacity, a safe destination, and `installed + pending < 2`. Commit payment, the stowed item and reservation together, in existing slot order. Rejected requests charge nothing and receive no item. Use the normal budget/sales bookkeeping, without superfridge consumption.
5. **Shopping brain.** Append a shopping routine and a distinct `FurnishHome` goal; do not reuse the existing professional decorator's `DecorateGoal`. Priority is above wandering and below needs/work/sleep/safety. A deterministic daily opportunity and saved retry cooldown prevent residents synchronizing or polling failed shops each tick. Reuse `Seek`, indexed feature selection, move-beside tasks and bounded retries. Resume from the actual inventory and pending receipt, not an assumed stage. A purchased vase can be brought home later even if the day off ends; starting a new purchase still requires a day off.
6. **Place-vase task/effect.** Walk home and interact from beside the reserved cell. The world rechecks ownership, reach, the shared cap, the carried item and current placement safety; on success remove exactly one item, add one map prop, release the receipt and register the permanent scent source. Temporary obstruction causes bounded waiting/replanning; it does not destroy the purchase or charge again. The effect slot cannot also be used for another transaction that tick.
7. **Lifecycle and saves.** Save pending receipts and retry timing using new optional protobuf fields; older saves start with none. Append the routine to preserve prefix-compatible brain restore. Rebuild installed counts and scent sources from saved map props. Validate receipts against living owners, home, item and destination. Death/removal releases the reservation without inventing a refund; existing inventory/death handling determines the item's fate. Home changes invalidate the old reservation and revalidate placement before reuse. Never silently place in somebody else's flat.
8. **Rendering.** Read the simulation map for props during gameplay and `CurrentMap` in the editor. Add a derived object revision so additions/removals refresh the visible prop window without camera movement or a full-map scan each frame. Keep the shared palette, painter depth, nearest sampling and 3x art scale. Register dynamic vase emissions without rescanning all map objects each scent step.

## Verification

- Unit tests: actual day off versus before/after shift, unemployment, nobles excluded from new shopping, medical leave, no home, insufficient cash, full pack, missing/unreachable shelf and cramped home.
- Transaction tests: two residents buying concurrently with one remaining slot, duplicate requests, cap including starter/in-transit vases, no charge on rejection, exact item/money conservation, occupied destination, interrupted purchase/return/placement, death/removal and home changes.
- Placement tests: room connectivity, door clearance and access to every furniture item across generated seeds; deterministic noble starter placement with unrelated random streams unchanged.
- Scent tests: vase emits without planting/growth, reaches neighboring air, respects walls/closed doors, persists after multiple world days, and continues after save/load.
- Save tests: old archives/configs load; new archives saved before purchase, in transit and just before placement continue identically after restore. Both serial and parallel simulation remain deterministic.
- Scripted QA: visible decor shelf and vase, commoner day-off purchase/return/place, no third vase, stationary-camera placement refresh, pleasant-scent overlay, and save/load. Inspect captured frames and check the pixel grid.
- Run `cargo test`, relevant store/economy/save/decoration QA, then release simulation performance QA. Compare performance only in release, including a day-off scenario where shopping actually runs.

## Work status

Implemented at the authorized $20 price. Content, shared household transactions, shopping behavior, save fields and rendering refresh are complete. The unrelated edit to `src/sim/brain/goals/haul.rs` is preserved.

Validation:

- `cargo test`: 1,586 passed, 4 existing ignored probes. The vase save/load test also passes with two shoppers and enough frozen background units to run the parallel rounds.
- Gameplay QA passes: flower vase shopping, noble starter vases, smell, store, economy, save/load.
- Release QA passes for both the real shopping flow (18,000 measured ticks) and simulation scaling. The 4,000-human median is 0.521 ms per simulation tick on this machine; no earlier comparable release baseline was available.
- A UI-free capture with both sprites passes the pixel grid at 4x. Both assets are 16x16 RGBA.

The cleaner scheduling fixture omits the decor shelf to retain its original noon deadline and exact nine-inspection cadence independently of discretionary shopping traffic. The store staffing fixture disables Mind before its first processing tick, so random mental sick leave cannot invalidate a staffing test. Neither adjustment changes production behavior.

Dynamic furniture also required retaining the map navigation revision and suspended search revision in saves. This prevents a route search invalidated by a newly placed vase from resuming against stale ground after loading; a dedicated regression test covers this and older archives retain their existing fallback.


## Art provenance

Built-in ImageGen produced the vase and decor shelf drafts. The final assets
are native 16x16 RGBA PNGs in `assets/flower_vase.png` and
`assets/decor_section.png`, sampled with nearest-neighbor filtering.

Vase prompt: single front-view flower vase on a 16x16 logical pixel grid,
transparent background, elegant blue ceramic vase, green stems and leaves,
three pink/cream/purple blossoms, restrained palette and dark pixel outlines,
no text, ground, gradients or antialiasing.

Shelf prompt: front-view decor-section shelf on a 16x16 logical pixel grid,
transparent background, dark wooden two-tier shelf displaying small blue vases
with pink/cream flowers and green leaves, simple dark outlines, no text,
labels, ground, shadows or antialiasing.

# Parity-harness choice-surfacing gaps — FOR REVIEW

> These are cases where the **Rust engine is correct vs the C# decomp**
> (`../sts2-wiki/decompiled/v0.103.2/`, the source of truth), but the
> **C# headless parity harness** (`../sts2-pycli/src/Sts2Headless/RunSimulator.cs`)
> fails to surface a player choice that the shipped game pauses for. The
> result is a `LEGAL_ACTION_DRIFT` in `parity_fuzz` even though Rust matches
> the game. Per the standing rule (2026-05-25): **do not regress the Rust
> implementation to match a deficient harness** — record here for review and
> ideally fix the harness.
>
> Tracking beads: `sts2-rs-8ukqd` (the engine-correctness umbrella that
> surfaced these), `sts2-rs-fdd9g` (Armaments harness gap).

---

## 0. RESOLVED (2026-05-25) — Armaments harness gap was a misdiagnosis

**Empirically retested under bead `sts2-rs-fdd9g`** by driving a controlled
scenario through the headless harness (Ironclad deck
`[ARMAMENTS, POMMEL_STRIKE×4]`, play Armaments with 4 upgradable cards in
hand): the harness **does** surface the choice — `play_card` returns
`{type: decision, decision: card_select, cards: [4× Pommel Strike]}` and
`legal_actions` includes the select. Confirmed load-bearing-test by
reverting any harness change: the choice surfaces with the *stock*
`RunSimulator.cs` (`DetectDecisionPoint`'s `checkCardSelect` already exposes
`_cardSelector.HasPending`), so **no harness change is needed**.

The ~21 original `LEGAL_ACTION_DRIFT` Armaments trials were the **count==1**
case: pre-`8ukqd` Rust opened a 1-option `UpgradeInHand` choice where the
decomp (and the harness) auto-resolve at the `list.Count <= 1` threshold.
That was a real *Rust* bug, fixed under `8ukqd` (Armaments + Headbutt now
match the `<=1 ⇒ auto` threshold). With that fixed, both branches agree:
`>=2` upgradable ⇒ both surface a choice; `<=1` ⇒ both auto-resolve.

Section 1 below is retained for historical context but its conclusion
("harness deficient, needs a `DoPlayCard` fix") is **superseded** by this
section. Bead `sts2-rs-fdd9g` closed as not-a-bug.

---

## 1. Armaments base — `FromHandForUpgrade` choice (SUPERSEDED — see §0)

**Decomp (source of truth):** `Armaments.OnPlay`
(`MegaCrit.Sts2.Core.Models.Cards/Armaments.cs`) calls
`CardSelectCmd.FromHandForUpgrade` (`CardSelectCmd.cs:517-554`). That method:

[decompiled C# omitted — see the cited path]

So with **2+ upgradable hand cards the game pauses for a player choice**;
with ≤1 it auto-resolves. `IsUpgradable` (`CardModel.cs:622`) is just
`CurrentUpgradeLevel < MaxUpgradeLevel` ≈ `!upgraded` for single-upgrade
cards.

**Rust (correct):** `play_armaments` (`crates/sts2-content/src/cards/ironclad.rs`)
opens a `ChoiceKind::UpgradeInHand` pending choice for ≥2 upgradable cards,
auto-upgrades for exactly 1, no-op for 0 — matching the decomp exactly.

**Harness (deficient):** with 2 un-upgraded cards in hand
(`parity_fuzz` `parity_state_drift4/trial_0`, MAWLER, hand `[HEMOKINESIS,
HEMOKINESIS]`), after the player plays Armaments the C# harness's
`legal_actions` returns `{end_turn, play_card …}` — i.e. it **auto-resolved
the upgrade internally and never surfaced a `choose` action**. Rust offers
the two `ResolveChoice` (`choose`) options. Net: `LEGAL_ACTION_DRIFT`
(`rust_only: [choose ×2]`, `csharp_only: [end_turn]`) on ~21/100 fuzz trials.

`RunSimulator.cs` *does* have the machinery to surface card-selects
(`HeadlessCardSelector` + `DetectDecisionPoint` line ~1968 surfaces
`HasPending`; `DoRemoveCard` line ~1564 explicitly `WaitFor(HasPending …)`).
But `DoPlayCard` (line ~1219) just calls `WaitForActionExecutor()` then
`DetectDecisionPoint()`, and empirically the Armaments upgrade choice is not
pending at that point — it auto-resolved. Root cause not yet pinned (either
`HeadlessCardSelector.GetSelectedCards` auto-resolves this flow, or
`WaitForActionExecutor` drains/cancels the pending before
`DetectDecisionPoint` runs).

**Proposed harness fix:** make `DoPlayCard` handle a card-play that opens a
pending card-select the same way `DoRemoveCard` does — wait for
`_cardSelector.HasPending || executor idle`, and if pending, return
`DetectDecisionPoint()` (which surfaces `choose`). Verify against a fixture
that plays base Armaments with ≥2 upgradable hand cards.

**Interim:** Rust keeps the correct choice. The ~21 Armaments trials are
expected `LEGAL_ACTION_DRIFT` until the harness is fixed — they are harness
artifacts, not Rust bugs. (Note: the **count==1** Armaments case was a real
Rust bug — Rust used to open a 1-option choice where the game auto-resolves —
and is now fixed to match the decomp threshold.)

---

## 2. Headbutt — same `≤1 ⇒ auto` threshold (now matched)

`Headbutt.OnPlay` uses `CardSelectCmd.FromSimpleGrid` with
`CardSelectorPrefs(prompt, 1)` (MinSelect=MaxSelect=1 ⇒
`RequireManualConfirmation == false`, `CardSelectorPrefs.cs:56`).
`FromSimpleGrid` (`CardSelectCmd.cs:232`) auto-resolves when
`cards.Count <= MinSelect` (≤1 discard card) and only prompts for 2+.

Rust `play_headbutt` previously opened a `PutBack` choice for **any**
non-empty discard — a real Rust bug on the 1-discard-card case (same class
as Armaments count==1). **Now fixed**: ≤1 discard ⇒ auto-move the single
card to draw-top (no choice), ≥2 ⇒ open the choice. This removes the
1-discard `LEGAL_ACTION_DRIFT` and matches the decomp in real play.

The ≥2-discard Headbutt choice IS surfaced by the harness (it's the same
`FromSimpleGrid`/`HasPending` path that worked before), so no harness gap
there — only Armaments' `FromHandForUpgrade` path is affected by gap #1.

---

## Auto-pick approximations (separate, pre-existing — NOT harness gaps)

Several cards still auto-pick the first candidate instead of opening the
decomp's choice (the inverse shortcut — Rust under-surfaces). These are
documented pre-existing approximations, **not** introduced here, and only
matter once their cards appear with 2+ candidates:

- `True Grit+` (upgraded) — auto-picks first hand card to exhaust
  (`ironclad.rs` ~781); decomp lets the player choose.
- `Hologram` (Defect) — auto-picks first discard card (`defect.rs` ~337).
- `Scavenge` (Defect) — auto-picks first hand card to exhaust
  (`defect.rs` ~618).
- Necrobinder discard-return (`necrobinder.rs` ~553).

These should eventually get real `pending_choice` surfaces (tracked under
the relevant card-fidelity beads). When they do, they may hit the **same
harness gap #1** if their decomp path is `FromHandForUpgrade`-like and the
harness auto-resolves it — re-check against this doc at that time.

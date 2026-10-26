# sts2-cli JSONL schema (v1)

`sts2-cli --jsonl <path>` writes one JSON object per simulator step. The file
has no header, no envelope; each line is self-contained and parseable
independently. Designed for streaming consumption by ML training pipelines.

## Per-step record

```json
{
  "step": 0,
  "phase": "PlayerPlayPhase",
  "round": 1,
  "state_summary": {
    "player_hp": 80,
    "player_block": 0,
    "player_energy": 3,
    "draw": 5,
    "discard": 0,
    "monsters": [
      { "hp": 53, "max_hp": 53, "block": 0, "intent": "Some(Buff)" }
    ],
    "hand": [
      { "card_id": "StrikeIronclad", "cost": 1 },
      { "card_id": "DefendIronclad", "cost": 1 }
    ]
  },
  "action": { "type": "play_card", "hand_index": 4, "target": 0 },
  "reward": 6,
  "result": "Advanced"
}
```

## Fields

| Field | Type | Notes |
|-------|------|-------|
| `step` | `u32` | Zero-based, monotonically increasing per file. Resets per process. |
| `phase` | string | Debug-printed `CombatPhase` variant. Always `PlayerPlayPhase` in v1 (the only phase the agent gets called in). |
| `round` | `u16` | Combat round, 1-indexed. |
| `state_summary` | object | Snapshot of the state **before** `action` was applied. See below. |
| `action` | object | The action applied this step. See below. |
| `reward` | `i32` | See "Reward function" below. |
| `result` | string | Debug-printed `StepResult`: `Advanced`, `PendingChoice`, `CombatWon`, `CombatLost`. |

### `state_summary`

| Field | Type | Notes |
|-------|------|-------|
| `player_hp` | `i32` | Current HP. |
| `player_block` | `i32` | Current block (resets each player turn). |
| `player_energy` | `i8` | Energy available before this action. |
| `draw` | `usize` | Draw-pile card count. |
| `discard` | `usize` | Discard-pile card count. |
| `monsters[]` | array | One entry per `enemy_uid` slot, in slot order. Dead monsters present until cleared by the engine. |
| `monsters[].hp` / `.max_hp` / `.block` | `i32` | Self-explanatory. |
| `monsters[].intent` | string | Debug-printed `Option<Intent>` (e.g. `"Some(Attack { damage: 6 })"`, `"None"`). |
| `hand[]` | array | One entry per card in hand, in hand-index order. |
| `hand[].card_id` | string | Debug-printed `CardId` variant (e.g. `"StrikeIronclad"`). |
| `hand[].cost` | `i8` | Energy cost. |

### `action`

Tagged union on the `type` field:

| `type` | Other fields |
|--------|--------------|
| `"play_card"` | `hand_index: u8`, optional `target: u8` |
| `"end_turn"` | (none) |
| `"use_potion"` | `slot: u8`, optional `target: u8` |
| `"resolve_choice"` | `selection: u8[]` |

### Reward function (v1)

Computed deterministically from the state delta:

```
reward = enemy_hp_total(before) - enemy_hp_total(after)        // damage dealt
       - (player_hp(before) - player_hp(after))                // damage taken
       + 10 if result == CombatWon
       - 10 if result == CombatLost
```

Block-mitigated damage counts as zero. Overkill counts as exactly the HP that
was removed (saturating). Negative deltas (heal) count toward the player.

## Stability

- v1 is **frozen on field names** — additive changes (new optional fields)
  may land in v1.x. Renames or removals bump to v2.
- `intent` is a debug-formatted string in v1; a structured representation
  may follow once the engine exposes a stable JSON shape.
- The schema is intentionally **simple and string-heavy** so a downstream
  consumer can `json.loads(line)` and start training without engine bindings.

## Example: validate every line

```bash
sts2-cli --agent random --seed 42 --steps 1000 --jsonl run.jsonl
python3 -c "import json; [json.loads(l) for l in open('run.jsonl')]; print('ok')"
```

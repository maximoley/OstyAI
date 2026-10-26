# Workbench golden fixtures

Recorded HTTP responses of the **Python** workbench server
(`sts2-pycli/python/workbench/server.py`, `create_app(...)`) — the oracle the
Rust `sts2-workbench` port is measured against. A nextest test replays each
scenario's requests through the port, normalizes its own response the same
way, and compares the SHA-256 (and the full body where one is stored).

Produced by `sts2-pycli/scripts/dump_workbench_golden.py`
(`just workbench-golden-dump` from `sts2-pycli/`; `just workbench-golden-check`
re-dumps into a temp dir and `diff -r`s against this directory). The scenario
definitions live in `sts2-pycli/tests/workbench/golden/scenarios/*.json`.
**Do not hand-edit anything here** — regenerate.

## Layout

```
<scenario>/
  requests.jsonl      one line per request, in order (i == 0 is POST /api/live/start)
  responses/<i>.json  full normalized body for a subset of requests (see below)
  meta.json           scenario params, engine build, python, patches, coverage tags
```

`requests.jsonl` line:

```json
{"i": 7, "method": "POST", "route": "/api/live/action",
 "body": {"action": "play_card", "args": {"card_index": 0, "target_index": 0}},
 "status": 200, "sha256": "<hex of the normalized response>",
 "decision": "combat_play"}
```

`body` is `null` for GETs. `decision` is the `decision` of the Step the
response carries (top-level for `start` / `state` / `saves/load`, under
`"step"` for `action` / `seek`) and `null` for responses without a Step
(`map`, `timeline`, `save`, `saves`).

`responses/<i>.json` exists for: (a) every probe request, (b) the first
request of the scenario whose resulting `(decision, context)` pair had not
been seen yet in that scenario, (c) every request with `status != 200`, and
(d) every request in the first 40 policy steps of a scenario flagged
`"full": true` (the three `long_*` scenarios). Encoded with
`json.dumps(obj, indent=1, sort_keys=True, ensure_ascii=False)`.

## Normalization (apply the SAME on the Rust side before comparing)

1. Recursively sort object keys.
2. Drop the keys `log_path` and `path` wherever they occur.
3. Round every float to 4 decimals (Python `round(x, 4)`). Floats stay
   floats: `1.0` is **not** turned into `1`. Ints / bools / strings / nulls
   are untouched.
4. `sha256` = SHA-256 of
   `json.dumps(normalized, sort_keys=True, separators=(",", ":"), ensure_ascii=False)`
   encoded as UTF-8 — compact JSON, no whitespace, sorted keys, non-ASCII
   left unescaped.

## How the oracle was configured (determinism)

* `STS2_ASSETS_BASE_URL=https://assets.test` (every art URL is
  `https://assets.test/<rel>`), art existence resolved against the SAME
  `asset_manifest.json` / `creature_scales.json` this crate embeds under
  `data/`, wiki manifests from the sibling `sts2-wiki/data`.
* Rust engine backend (`"backend": "rust"` on `/api/live/start`); seeds are
  canonical already (`G0LDEN0001`, `G0LDEN0002`).
* `saved_at` pinned to `2026-01-01T00:00:00Z`.
* The PPO run-policy advisors are stubbed to `None`: timeline node `value`s
  are `null` and the map `path_plan` is unseeded — as the browser port renders
  them. No hint/advisor route is ever requested.

## Scenarios

40 = 5 characters x 2 seeds x ascension {0, 10} x act1 {overgrowth,
underdocks}, 150 steps each, plus 3 `long_*` scenarios (Ironclad / Silent /
Defect, A0, `act1: "roll"`, 2000 steps) that try to reach `game_over`.

Every scenario interleaves probes at fixed step numbers (a "step" is one
`POST /api/live/action` issued by the policy; probes at step *N* run after
*N* actions): `GET /api/live/map` @40, `GET /api/live/timeline` @41, a seek
@42 (`GET /api/live/timeline` then `POST /api/live/seek` to the node 5 up the
active path — play continues from there, forking the tree), `POST
/api/live/save` @60, `GET /api/saves` @61, `POST /api/saves/load` @62, and
`GET /api/live/state` every 25 steps.

The policy is deterministic and reads only the previous response — see the
`Policy` class in the dump script: first playable hand card (targeting
`enemies[0]`), one potion in round 1 of each combat, map preference Shop >
RestSite > Treasure > Elite > Event > Monster > Ancient > Boss, shop = buy first
card / relic / potion then the card-removal service then leave, `SMITH` at
every rest site, event option 0 (Neow: option 1 on the second seed), card /
bundle / card-select pick 0. The `long_*` scenarios use the `"policy":
"survivor"` variant (`SurvivorPolicy`): greedy combat, `HEAL` below 60% HP,
Elites last — the basic policy dies before the act-1 boss on every character,
so this is what covers the boss-relic pick and act 2.

Note: a treasure room never surfaces as an `event_choice` on the rust backend —
the engine resolves the chest on entry and returns to the map; the evidence is
the `room_reveal` payload on the following state.

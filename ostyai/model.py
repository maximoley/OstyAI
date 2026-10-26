"""NUM1 -> compositional tokens -> four-layer policy/value transformer.

All record decoding, categorical lookup, field projection and relation binding
run on the input device. No Python/Rust state traversal or JSON in forward().
Transport row 0 becomes a learned summary token, not a metadata embedding.
"""

from dataclasses import asdict, dataclass

import torch
from fast_tokenizer import assemble
from torch import nn
from torch.nn import functional as F


@dataclass(frozen=True)
class Config:
    width: int = 128
    heads: int = 4
    layers: int = 4
    ff: int = 512
    slots: int = 128
    card_definitions: int = 2048
    catalog_names: int = 4096
    enum_size: int = 256
    numeric_fields: int = 40


class Vocabulary:
    """Disjoint semantic namespaces, shared across record types where appropriate."""

    def __init__(self, cfg):
        sizes = dict(
            kind=20,
            card=cfg.card_definitions,
            name=cfg.catalog_names,
            enchant=cfg.enum_size,
            affliction=cfg.enum_size,
            intent=cfg.enum_size,
            choice=cfg.enum_size,
            action=8,
            upgrade=2,
            pile=8,
            card_type=16,
            target_type=32,
            phase=16,
        )
        offset = 1  # 0 = absent property, fixed zero embedding
        self.offsets = {}
        for name, size in sizes.items():
            self.offsets[name] = offset
            offset += size
        self.size = offset


def decode_records(raw, cfg, offsets):
    """GPU tensor-only decoder. The raw input is [batch, slots, 16] int32.

    The first slot is NUM1 header+metadata. References are converted from
    entity-relative row indices to slot indices (+1); missing references = 0.
    Numeric IDs, random teacher choices and allocation metadata are not features.
    """
    b, s, _ = raw.shape
    slot = torch.arange(s, device=raw.device)[None, :]
    valid = slot <= raw[:, 0, 10, None]
    entity = valid & (slot > 0)
    # Sanitize padding BEFORE every lookup; reused native arenas can contain junk.
    rows = torch.where(entity[..., None], raw, 0).long()
    k, d, owner, index = (rows[..., i] for i in range(4))
    values = rows[..., 4:16]
    card = (k == 3) | (k == 11)
    catalog = (
        (k == 4)
        | (k == 5)
        | (k == 6)
        | (k == 7)
        | (k == 14)
        | (k == 15)
        | (k == 16)
        | ((k == 8) & (index == 0))
    )
    main = torch.zeros_like(k)
    main = torch.where(card, offsets["card"] + (d - 1) // 2, main)
    main = torch.where(catalog, offsets["name"] + d, main)
    for kind, name in [(9, "intent"), (10, "choice"), (12, "action")]:
        main = torch.where(k == kind, offsets[name] + d, main)
    main = torch.where((k == 8) & (index == 1), offsets["enchant"] + d, main)
    main = torch.where((k == 8) & (index == 2), offsets["affliction"] + d, main)
    upgrade = torch.where(card, offsets["upgrade"] + (d - 1) % 2, 0)
    pile = torch.where(
        k == 3,
        offsets["pile"] + values[..., 0],
        torch.where(k == 11, offsets["pile"] + values[..., 1], 0),
    )
    card_type = torch.where(k == 3, offsets["card_type"] + values[..., 9], 0)
    target_type = torch.where(k == 3, offsets["target_type"] + values[..., 10], 0)
    move = torch.where(k == 4, offsets["name"] + values[..., 4], 0)
    potion = torch.where((k == 12) & (d == 3), offsets["name"] + values[..., 3], 0)
    phase = torch.where(k == 1, offsets["phase"] + values[..., 1], 0)
    cats = torch.stack(
        [offsets["kind"] + k, main, upgrade, pile, card_type, target_type, move, potion, phase], -1
    )
    cats = torch.where(entity[..., None], cats, 0)
    col = torch.arange(12, device=raw.device)
    targeted = (k == 12) & ((d == 2) | (d == 3))
    excluded = (
        ((targeted | (k == 11))[..., None] & (col == 0))
        | ((k == 5)[..., None] & (col == 3))
        | ((k == 4)[..., None] & (col == 4))
        | ((k == 3)[..., None] & ((col == 0) | (col >= 8)))
        | ((k == 11)[..., None] & (col == 1))
        | (((k == 12) & (d == 3))[..., None] & (col == 3))
        | ((k == 1)[..., None] & (col == 1))
    )
    numeric = torch.where(excluded, 0, values).float()
    reward_fields = raw[..., 4:16].contiguous().view(torch.float32)
    numeric = torch.where(((k >= 17) & (k <= 19))[..., None], reward_fields, numeric)
    flags = ((values[..., 8, None] >> torch.arange(8, device=raw.device)) & 1).float()
    flags = flags * (k == 3)[..., None]
    keyword = values[..., 11]
    keyword_present = (k == 3) & (keyword >= 0)
    bits = ((keyword[..., None] >> torch.arange(16, device=raw.device)) & 1).float()
    bits = bits * keyword_present[..., None]
    numeric = torch.cat(
        [
            numeric,
            index[..., None].float(),
            flags,
            bits,
            keyword_present[..., None].float(),
            torch.zeros((b, s, 2), device=raw.device),
        ],
        -1,
    )
    numeric = numeric.sign() * torch.log1p(numeric.abs())
    numeric = numeric * entity[..., None]
    own = torch.where(entity & (owner >= 0), owner + 1, 0)
    ref = torch.where(targeted | (k == 11), values[..., 0], torch.where(k == 5, values[..., 3], -1))
    ref = torch.where(entity & (ref >= 0), ref + 1, 0)
    # Children carrying attributes of specific entities, not the action list.
    child = (
        entity & ((k == 5) | (k == 8) | (k == 9) | (k == 16) | (k == 18) | (k == 19)) & (own > 0)
    )
    action = entity & (k == 12)
    return cats, numeric, valid, own, ref, child, action


def gather_rows(x, indices):
    return x.gather(1, indices[..., None].expand(-1, -1, x.shape[-1]))


class Block(nn.Module):
    def __init__(self, cfg):
        super().__init__()
        self.heads = cfg.heads
        self.norm1 = nn.LayerNorm(cfg.width)
        self.qkv = nn.Linear(cfg.width, 3 * cfg.width)
        self.proj = nn.Linear(cfg.width, cfg.width)
        self.norm2 = nn.LayerNorm(cfg.width)
        self.ff1 = nn.Linear(cfg.width, cfg.ff)
        self.ff2 = nn.Linear(cfg.ff, cfg.width)

    def forward(self, x, mask):
        b, s, d = x.shape
        q, k, v = self.qkv(self.norm1(x)).reshape(b, s, 3, self.heads, d // self.heads).unbind(2)
        z = F.scaled_dot_product_attention(
            q.transpose(1, 2),
            k.transpose(1, 2),
            v.transpose(1, 2),
            attn_mask=mask[:, None, None, :],
            dropout_p=0.0,
        )
        x = x + self.proj(z.transpose(1, 2).reshape(b, s, d))
        return x + self.ff2(F.gelu(self.ff1(self.norm2(x)), approximate="tanh"))


class CombatTransformer(nn.Module):
    def __init__(self, cfg=Config()):
        super().__init__()
        self.cfg = cfg
        self.vocabulary = Vocabulary(cfg)
        self.semantic = nn.Embedding(self.vocabulary.size, cfg.width, padding_idx=0)
        self.numeric = nn.Linear(cfg.numeric_fields, cfg.width, bias=False)
        self.kind_gate = nn.Embedding(20, cfg.width)
        self.owner_projection = nn.Linear(cfg.width, cfg.width, bias=False)
        self.reference_projection = nn.Linear(cfg.width, cfg.width, bias=False)
        self.child_projection = nn.Linear(cfg.width, cfg.width, bias=False)
        self.input_norm = nn.LayerNorm(cfg.width)
        self.summary = nn.Parameter(torch.randn(cfg.width) * 0.02)
        self.blocks = nn.ModuleList([Block(cfg) for _ in range(cfg.layers)])
        self.output_norm = nn.LayerNorm(cfg.width)
        self.policy = nn.Sequential(
            nn.Linear(4 * cfg.width, cfg.width), nn.GELU(), nn.Linear(cfg.width, 1)
        )
        self.value = nn.Sequential(
            nn.Linear(cfg.width, cfg.width), nn.GELU(), nn.Linear(cfg.width, 1)
        )
        nn.init.normal_(self.semantic.weight, std=0.02)
        nn.init.zeros_(self.semantic.weight.data[0])
        nn.init.zeros_(self.kind_gate.weight)

    def encode(self, raw):
        cats, nums, valid, owner, ref, child, action = decode_records(
            raw, self.cfg, self.vocabulary.offsets
        )
        projected = self.numeric(nums)
        if raw.is_cuda and not torch.is_grad_enabled():
            x = assemble(
                cats,
                projected,
                self.semantic.weight,
                self.kind_gate.weight,
                self.vocabulary.offsets["kind"],
            )
        else:
            atom = self.semantic(cats).sum(-2) / 3.0
            kind = torch.where(valid, raw[..., 0], 0).long()
            kind = torch.where(torch.arange(raw.shape[1], device=raw.device)[None, :] > 0, kind, 0)
            x = atom + projected * (1 + self.kind_gate(kind).tanh())
        # Bind child attributes to their exact parent before attention. Thus two
        # otherwise identical cards remain distinguishable by their enchantments.
        messages = self.child_projection(x) * child[..., None]
        pooled = torch.zeros_like(x).scatter_add(1, owner[..., None].expand_as(x), messages)
        base = x + pooled
        x = base + self.owner_projection(gather_rows(base, owner)) * (owner > 0)[..., None]
        x = x + self.reference_projection(gather_rows(base, ref)) * (ref > 0)[..., None]
        is_summary = torch.arange(raw.shape[1], device=raw.device)[None, :] == 0
        x = torch.where(is_summary[..., None], self.summary, x)
        x = self.input_norm(x).to(projected.dtype) * valid[..., None]
        return x, valid, owner, ref, action

    def forward_tokens(self, encoded, raw):
        x, valid, owner, ref, _action = encoded
        for block in self.blocks:
            x = block(x, valid)
        x = self.output_norm(x)
        # Gather in the simulator's ordered legal-action order, never a fixed
        # global card vocabulary. Terminal states have zero legal entries.
        order = torch.arange(raw.shape[1], device=raw.device)[None, :]
        legal = order < raw[:, 0, 12, None]
        at = torch.where(legal, raw[:, 0, 11, None] + 1 + order, 0).long()
        h = gather_rows(x, at)
        own = gather_rows(x, owner.gather(1, at))
        target = gather_rows(x, ref.gather(1, at))
        summary = x[:, :1].expand_as(h)
        logits = self.policy(torch.cat([h, own, target, summary], -1)).squeeze(-1).float()
        logits = logits.masked_fill(~legal, -torch.inf)
        value = self.value(x[:, 0]).squeeze(-1).float()
        return logits, value, legal

    def forward(self, raw):
        return self.forward_tokens(self.encode(raw), raw)

    def describe(self):
        return dict(
            config=asdict(self.cfg),
            vocabulary=self.vocabulary.size,
            parameters=sum(p.numel() for p in self.parameters()),
            components={
                name: sum(p.numel() for p in module.parameters())
                for name, module in self.named_children()
            },
        )


def choose_action(logits, legal):
    """Safe even for terminal states; no softmax on an all-masked action set."""
    return torch.where(legal.any(-1), logits.argmax(-1), -1).to(torch.int32)

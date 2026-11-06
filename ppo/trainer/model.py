"""NUM1 -> compositional tokens -> public token encoder for asymmetric actor-critic.

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
    width: int = 256
    heads: int = 8
    layers: int = 8
    ff: int = 1024
    slots: int = 128
    card_definitions: int = 585
    catalog_names: int = 3447
    enum_size: int = 256
    numeric_fields: int = 40


class Vocabulary:
    """Disjoint semantic namespaces, shared across record types where appropriate."""

    def __init__(self, cfg):
        sizes = dict(
            kind=64,
            card=cfg.card_definitions,
            name=cfg.catalog_names,
            enchant=cfg.enum_size,
            affliction=cfg.enum_size,
            intent=cfg.enum_size,
            choice=cfg.enum_size,
            action=64,
            run=4096,
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
    card = (k == 3) | (k == 11) | (k == 27) | (k == 35)
    catalog = (
        (k == 4)
        | (k == 37)
        | (k == 5)
        | (k == 6)
        | (k == 7)
        | (k == 14)
        | (k == 15)
        | (k == 16)
        | ((k == 8) & (index == 0))
    )
    run_id = torch.where(k == 20, d, torch.where((k == 21) | (k == 23), 100 + d, torch.where(k == 24, 200 + d, torch.where(k == 25, 256 + d, torch.where(k == 26, 1000 + d, 1100 + d)))))
    main = torch.where(k >= 20, offsets['run'] + run_id, 0)
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
    excluded = excluded | (((k == 12) | (k == 22))[..., None] & (col == 0))
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
    extra_ref = (k == 22) | (k == 12)
    ref = torch.where(extra_ref, values[..., 0], ref)
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
    def __init__(self,cfg):
        super().__init__();self.heads=cfg.heads
        self.norm1=nn.LayerNorm(cfg.width);self.qkv=nn.Linear(cfg.width,3*cfg.width);self.proj=nn.Linear(cfg.width,cfg.width)
        self.norm2=nn.LayerNorm(cfg.width);self.ff1=nn.Linear(cfg.width,cfg.ff);self.ff2=nn.Linear(cfg.ff,cfg.width)
    def forward(self,x,valid):
        b,s,d=x.shape
        q,k,v=self.qkv(self.norm1(x)).reshape(b,s,3,self.heads,d//self.heads).unbind(2)
        z=F.scaled_dot_product_attention(q.transpose(1,2),k.transpose(1,2),v.transpose(1,2),attn_mask=valid[:,None,None,:],dropout_p=0.)
        x=x+self.proj(z.transpose(1,2).reshape(b,s,d))
        return x+self.ff2(F.gelu(self.ff1(self.norm2(x)),approximate='tanh'))

class PrivilegedCriticEncoder(nn.Module):
    """Critic-only card order and future encounters; never called by actor()."""
    def __init__(self,cfg):
        super().__init__();d=cfg.width
        self.kind=nn.Embedding(8,d,padding_idx=0);self.card=nn.Embedding(cfg.card_definitions+1,d,padding_idx=0)
        self.upgrade=nn.Embedding(2,d);self.encounter=nn.Embedding(513,d,padding_idx=0);self.position=nn.Embedding(2048,d)
        self.numeric=nn.Linear(12,d,bias=False);self.norm=nn.LayerNorm(d)
        self.ff=nn.Sequential(nn.Linear(d,d),nn.GELU(),nn.Linear(d,d))
        self.query=nn.Linear(d,d,bias=False);self.key=nn.Linear(d,d,bias=False);self.value=nn.Linear(d,d,bias=False)
    def forward(self,raw,summary):
        slot=torch.arange(raw.shape[1],device=raw.device)[None,:]
        valid=(slot>0)&(slot<=raw[:,0,10,None]);r=torch.where(valid[...,None],raw,0).long()
        k,identity,pos=r[...,0],r[...,1],r[...,3]
        card_id=torch.where(k==1,1+(identity-1)//2,0)
        encounter_id=torch.where(k==2,identity,0)
        nums=r[...,4:16].float();nums=nums.sign()*nums.abs().log1p()
        h=self.kind(k)+self.card(card_id)+self.upgrade(((identity-1)%2).clamp_min(0))*(k==1)[...,None]+self.encounter(encounter_id)+self.position(pos.clamp_min(0))+self.numeric(nums)
        h=self.norm(h+self.ff(h));score=(self.query(summary)[:,None,:]*self.key(h)).sum(-1)/(summary.shape[-1]**.5)
        weights=score.masked_fill(~valid,torch.finfo(score.dtype).min).softmax(-1)*valid
        weights=weights/weights.sum(-1,keepdim=True).clamp_min(1e-8)
        return (self.value(h)*weights[...,None]).sum(1)

class ActorCritic(nn.Module):
    def __init__(self,cfg=Config()):
        super().__init__();self.cfg=cfg;self.vocabulary=Vocabulary(cfg);d=cfg.width
        self.semantic=nn.Embedding(self.vocabulary.size,d,padding_idx=0);self.numeric=nn.Linear(cfg.numeric_fields,d,bias=False)
        self.kind_gate=nn.Embedding(64,d);self.owner_projection=nn.Linear(d,d,bias=False);self.reference_projection=nn.Linear(d,d,bias=False)
        self.child_projection=nn.Linear(d,d,bias=False);self.input_norm=nn.LayerNorm(d);self.summary=nn.Parameter(torch.randn(d)*.02)
        self.blocks=nn.ModuleList([Block(cfg) for _ in range(cfg.layers)]);self.output_norm=nn.LayerNorm(d)
        def policy_head():return nn.Sequential(nn.Linear(4*d,d),nn.GELU(),nn.Linear(d,1))
        def value_head():return nn.Sequential(nn.Linear(2*d,d),nn.GELU(),nn.Linear(d,1))
        self.combat_policy=policy_head();self.noncombat_policy=policy_head()
        self.privileged=PrivilegedCriticEncoder(cfg);self.combat_value=value_head();self.noncombat_value=value_head()
        nn.init.normal_(self.semantic.weight,std=.02);nn.init.zeros_(self.semantic.weight.data[0]);nn.init.zeros_(self.kind_gate.weight)
        for head in [self.combat_policy,self.noncombat_policy,self.combat_value,self.noncombat_value]:
            nn.init.normal_(head[-1].weight,std=.01);nn.init.zeros_(head[-1].bias)
    def encode(self,raw):
        cats,nums,valid,owner,ref,child,action=decode_records(raw,self.cfg,self.vocabulary.offsets)
        projected=self.numeric(nums)
        if raw.is_cuda and not torch.is_grad_enabled():
            x=assemble(cats,projected,self.semantic.weight,self.kind_gate.weight,self.vocabulary.offsets['kind'])
        else:
            atom=self.semantic(cats).sum(-2)/3.;kind=torch.where(valid,raw[...,0],0).long()
            kind=torch.where(torch.arange(raw.shape[1],device=raw.device)[None,:]>0,kind,0)
            x=atom+projected*(1+self.kind_gate(kind).tanh())
        messages=self.child_projection(x)*child[...,None];pooled=torch.zeros_like(x).scatter_add(1,owner[...,None].expand_as(x),messages)
        base=x+pooled;x=base+self.owner_projection(gather_rows(base,owner))*(owner>0)[...,None]
        x=x+self.reference_projection(gather_rows(base,ref))*(ref>0)[...,None]
        x=torch.where((torch.arange(raw.shape[1],device=raw.device)[None,:]==0)[...,None],self.summary,x)
        x=self.input_norm(x).to(projected.dtype)*valid[...,None]
        for block in self.blocks:x=block(x,valid)
        return self.output_norm(x),owner,ref
    def actor_from_encoded(self,raw,encoded,action_slots):
        x,owner,ref=encoded;order=torch.arange(action_slots,device=raw.device)[None,:]
        legal=order<raw[:,0,12,None];at=torch.where(legal,raw[:,0,11,None]+1+order,0).long()
        own=gather_rows(x,owner.gather(1,at));target=gather_rows(x,ref.gather(1,at))
        h=gather_rows(x,at);summary=x[:,:1].expand_as(h);composed=torch.cat([h,own,target,summary],-1)
        combat=raw[:,0,8].bool()
        logits=torch.where(combat[:,None],self.combat_policy(composed).squeeze(-1),self.noncombat_policy(composed).squeeze(-1)).float()
        return logits.masked_fill(~legal,-torch.inf),legal
    def critic(self,raw,summary,privileged):
        # Private features enter ONLY here, after the public transformer.
        p=self.privileged(privileged,summary);v=torch.cat([summary,p],-1);combat=raw[:,0,8].bool()
        def value_fp32(head,x):
            hidden=head[1](head[0](x))
            with torch.autocast(device_type=x.device.type,enabled=False):
                return F.linear(hidden.float(),head[2].weight.float(),head[2].bias.float()).squeeze(-1)
        value=torch.where(combat,value_fp32(self.combat_value,v),value_fp32(self.noncombat_value,v))
        return torch.where(raw[:,0,14].bool(),0.,value)
    def actor(self,raw,action_slots=64):
        """Deployment API has no privileged-input argument or critic call."""
        return self.actor_from_encoded(raw,self.encode(raw),action_slots)
    def forward(self,raw,privileged,action_slots=64):
        encoded=self.encode(raw);logits,legal=self.actor_from_encoded(raw,encoded,action_slots)
        return logits,self.critic(raw,encoded[0][:,0],privileged),legal
    def describe(self):
        return dict(config=asdict(self.cfg),parameters=sum(p.numel() for p in self.parameters()),components={n:sum(p.numel() for p in m.parameters()) for n,m in self.named_children()},architecture='8-layer public encoder; separate combat/noncombat policy and value heads; critic-only private fusion')


def sample_actions(logits,legal):
    # Safe for terminal/dummy padded states, whose output is ignored.
    safe=torch.where(legal.any(-1,keepdim=True),logits,torch.zeros_like(logits))
    # Gumbel-max samples the ordinary categorical policy; this is not tree search.
    u=torch.rand_like(safe).clamp_(1e-7,1-1e-7)
    action=(safe-(-u.log()).log()).argmax(-1)
    logp=safe.log_softmax(-1).gather(-1,action[:,None]).squeeze(-1)
    return torch.where(legal.any(-1),action,-1).int(),torch.where(legal.any(-1),logp,0.)

"""Wall-clock LR and decision limits. Pure Python; also used by the supervisor."""
from datetime import datetime
import time


def timestamp(value):
    d = datetime.fromisoformat(value)
    if d.tzinfo is None:
        raise ValueError('Schedule timestamps must include a UTC offset')
    return d.timestamp()


def deadline_reached(cfg, now=None):
    return (time.time() if now is None else now) >= timestamp(cfg['schedule']['stop_at'])


def next_rollout_size(cfg, collected):
    remaining = max(0, cfg['max_collected_decisions'] - collected)
    size = min(cfg['rollout_steps'], remaining)
    if size % cfg['environment']['slots']:
        raise ValueError('Decision cap must align with the native batch size')
    return size


class LRControl:
    def __init__(self, cfg, clock=time.time):
        self.cfg = cfg
        self.clock = clock
        self.frozen_multiplier = None
        s = cfg['schedule']
        self.freeze = timestamp(s['adapt_lr_until'])
        self.start, self.end = timestamp(s['anneal_start']), timestamp(s['anneal_end'])
        assert self.freeze == self.start < self.end == timestamp(s['stop_at'])
        assert 0 <= s['anneal_drop'] <= 1 and s['anneal_kind'] == 'linear'

    def snapshot(self, multiplier, warmup=1., now=None):
        now = self.clock() if now is None else now
        if now >= self.freeze and self.frozen_multiplier is None:
            self.frozen_multiplier = multiplier
        effective = multiplier if self.frozen_multiplier is None else self.frozen_multiplier
        fraction = min(1., max(0., (now - self.start) / (self.end - self.start)))
        anneal = 1. - self.cfg['schedule']['anneal_drop'] * fraction
        factor = effective * warmup * anneal
        return dict(time=now, phase='adaptive' if now < self.freeze else 'annealing' if now < self.end else 'expired',
                    multiplier=effective, frozen_multiplier=self.frozen_multiplier, warmup=warmup,
                    anneal_factor=anneal, learning_rates=[self.cfg[k] * factor for k in ['learning_rate', 'critic_learning_rate']])

    def adapt(self, multiplier, kl):
        # Observe the boundary before adapting, including a minibatch crossing it.
        self.snapshot(multiplier)
        if self.frozen_multiplier is not None:
            return self.frozen_multiplier
        if kl < self.cfg['kl_lr_low']:
            multiplier *= 1.5
        elif kl > self.cfg['kl_lr_high']:
            multiplier /= 1.5
        lo, hi = self.cfg['lr_multiplier_bounds']
        return min(hi, max(lo, multiplier))

    def state_dict(self):
        return dict(frozen_multiplier=self.frozen_multiplier)

    def load_state_dict(self, state):
        self.frozen_multiplier = state['frozen_multiplier']

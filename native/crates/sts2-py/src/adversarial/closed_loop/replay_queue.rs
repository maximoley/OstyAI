//! Bounded completed-episode learner queue. Unfinished combats never get value labels.
use super::{numeric, Result};
use std::collections::VecDeque;
#[derive(Debug)]
pub(super) struct Sample {
    pub raw: Vec<numeric::Row>,
    pub policy: Vec<f32>,
    pub value: f32,
    pub combat: u64,
    pub version: u64,
}
#[derive(Debug, Default)]
pub(super) struct Queue {
    pub samples: VecDeque<Sample>,
    pub added: u64,
    pub evicted: u64,
    pub censored: u64,
    raw: Vec<numeric::Row>,
    policy: Vec<f32>,
    values: Vec<f32>,
    ids: Vec<u64>,
    versions: Vec<u64>,
}
#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub struct ReplayDescriptor {
    pub raw: u64,
    pub policy: u64,
    pub values: u64,
    pub ids: u64,
    pub versions: u64,
    pub count: u32,
    pub rows: u32,
}
impl Queue {
    pub fn commit(&mut self, samples: Vec<Sample>, value: f32, capacity: usize) {
        for mut sample in samples {
            sample.value = value;
            self.samples.push_back(sample);
            self.added += 1;
            while self.samples.len() > capacity {
                self.samples.pop_front();
                self.evicted += 1;
            }
        }
    }
    pub fn drain(&mut self, out: &mut ReplayDescriptor, limit: usize) -> Result<usize> {
        if limit == 0 || limit > 8192 {
            return Err("invalid replay packet size".into());
        }
        let n = limit.min(self.samples.len());
        if n == 0 {
            return Ok(0);
        }
        let max = self
            .samples
            .iter()
            .take(n)
            .map(|s| s.raw.len())
            .max()
            .unwrap_or(1);
        let rows = super::CAPACITIES[super::Control::bucket(max)];
        if max > rows {
            return Err("replay row capacity exceeded".into());
        }
        self.raw.clear();
        self.raw.resize(n * rows, [0; 16]);
        self.policy.clear();
        self.policy.resize(n * rows, 0.0);
        self.values.clear();
        self.ids.clear();
        self.versions.clear();
        for i in 0..n {
            let s = self.samples.pop_front().expect("queued sample");
            self.raw[i * rows..i * rows + s.raw.len()].copy_from_slice(&s.raw);
            self.policy[i * rows..i * rows + s.policy.len()].copy_from_slice(&s.policy);
            self.values.push(s.value);
            self.ids.push(s.combat);
            self.versions.push(s.version);
        }
        *out = ReplayDescriptor {
            raw: self.raw.as_ptr() as u64,
            policy: self.policy.as_ptr() as u64,
            values: self.values.as_ptr() as u64,
            ids: self.ids.as_ptr() as u64,
            versions: self.versions.as_ptr() as u64,
            count: n as u32,
            rows: rows as u32,
        };
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_replay_samples_preserve_every_row() {
        for count in [1025, 1537, 2049, 3073, 4096] {
            let mut q = Queue::default();
            let sample = Sample {
                raw: vec![[7; 16]; count],
                policy: vec![1.0],
                value: 0.0,
                combat: 123,
                version: 7,
            };
            q.commit(vec![sample], 1.25, 8);
            let mut descriptor = ReplayDescriptor::default();
            assert_eq!(q.drain(&mut descriptor, 1).unwrap(), 1);
            assert!(descriptor.rows as usize >= count);
            assert_eq!(&q.raw[..count], vec![[7; 16]; count].as_slice());
            assert!(q.raw[count..].iter().all(|r| *r == [0; 16]));
            assert_eq!(q.ids, vec![123]);
            assert_eq!(q.values, vec![1.25]);
        }
    }

    #[test]
    fn oversized_replay_error_preserves_queued_sample() {
        let mut q = Queue::default();
        q.commit(
            vec![Sample {
                raw: vec![[7; 16]; 4097],
                policy: vec![1.0],
                value: 0.0,
                combat: 123,
                version: 7,
            }],
            1.25,
            8,
        );
        let mut descriptor = ReplayDescriptor::default();
        assert!(q.drain(&mut descriptor, 1).is_err());
        assert_eq!(q.samples.len(), 1);
    }

    #[test]
    fn bounded_queue_preserves_policy_version_and_terminal_value() {
        let mut q = Queue::default();
        let samples = (0..3)
            .map(|id| Sample {
                raw: vec![[0; 16]; 2],
                policy: vec![1.0],
                value: 0.0,
                combat: id,
                version: 9,
            })
            .collect();
        q.commit(samples, 1.7, 2);
        assert_eq!(q.evicted, 1);
        let mut d = ReplayDescriptor::default();
        assert_eq!(q.drain(&mut d, 1).unwrap(), 1);
        assert_eq!(q.ids[0], 1);
        assert_eq!(q.versions[0], 9);
        assert!((q.values[0] - 1.7).abs() < 1e-6);
        assert!((q.policy[0] - 1.0).abs() < 1e-6);
        assert_eq!(q.samples.len(), 1);
    }
}

use rand::{Rng, SeedableRng};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::{sleep_until, Instant};

#[derive(Debug, Clone, Default)]
pub struct Params {
    pub rampdown: Duration,
    pub min: Duration,
    pub max: Duration,
}

#[derive(Debug, Clone)]
pub struct Interval {
    rng: rand::rngs::SmallRng,
    start: Instant,
    rampdown: Duration,
    min: Duration,
    max: Duration,
    last_broadcast: Arc<Mutex<Option<Instant>>>,
}

impl From<Params> for Interval {
    fn from(p: Params) -> Self {
        assert!(p.min < p.max);
        Interval {
            min: p.min,
            max: p.max,
            rampdown: p.rampdown,
            rng: rand::rngs::SmallRng::from_entropy(),
            start: Instant::now(),
            last_broadcast: Arc::new(Mutex::new(None)),
        }
    }
}

impl Interval {
    pub fn now(&mut self) -> Duration {
        if self.start.elapsed() > self.rampdown {
            return self.max;
        }
        let dy = self.max - self.min;
        let dx = self.rampdown.as_secs_f32() / self.start.elapsed().as_secs_f32();
        let amplitude = self.min + dy.mul_f32(dx);

        let rand = self.rng.gen_range(0.9..1.1);
        amplitude.mul_f32(rand)
    }
    pub async fn sleep_till_next(&mut self) {
        sleep_until(self.next()).await;
        *self.last_broadcast.lock().unwrap() = Some(Instant::now());
    }
    pub fn next(&mut self) -> Instant {
        let last = *self.last_broadcast.lock().unwrap();
        match last {
            Some(last) => last + self.now(),
            None => Instant::now(),
        }
    }
}

pub trait Until {
    fn until(&self) -> Duration;
}

impl Until for tokio::time::Instant {
    fn until(&self) -> Duration {
        self.saturating_duration_since(Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::sleep_until;

    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[tokio::test]
    async fn test_interval() {
        let call_next = tokio::time::Instant::now();
        let mut interval: Interval = Params {
            min: Duration::from_secs(0),
            max: Duration::from_secs(10),
            rampdown: Duration::from_secs(1),
        }.into();

        for _i in 0..10 {
            let call_next = call_next + Duration::from_secs_f32(0.1);
            sleep_until(call_next).await;
            assert_eq!(interval.now(), Duration::from_secs(0))
        }
    }
}

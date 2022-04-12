use rand::{Rng, SeedableRng};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::{sleep_until, Instant};

#[derive(Debug, Clone)]
pub struct Params {
    pub rampdown: Duration,
    pub min: Duration,
    pub max: Duration,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            rampdown: Duration::from_secs(10),
            min: Duration::from_millis(100),
            max: Duration::from_secs(1),
        }
    }
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
        dbg!(p.min, p.max);
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
        let dx = self.rampdown;
        let slope = dy.as_secs_f32() / dx.as_secs_f32();
        let x = self.start.elapsed();
        let rand = self.rng.gen_range(0.9..1.1);
        self.min + x.mul_f32(slope).mul_f32(rand)
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
    use more_asserts::*;
    use tokio::time::sleep_until;

    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[tokio::test]
    async fn test_interval() {
        let mut call_next = tokio::time::Instant::now();
        let mut interval: Interval = Params {
            min: Duration::from_secs(0),
            max: Duration::from_secs(1),
            rampdown: Duration::from_secs(1),
        }
        .into();

        for i in 1..=10 {
            call_next = call_next + Duration::from_secs_f32(0.1);
            sleep_until(call_next).await;
            let correct = Duration::from_secs_f32(0.1 * (i as f32)).as_millis();
            assert_lt!(u128::abs_diff(interval.now().as_millis(), correct), i * 11);
        }
    }
}

use std::{
    fmt,
    ops::Sub,
    time::{
        Duration,
        SystemTime,
    },
};

use anyhow::{
    anyhow,
    Context,
};
use derive_more::FromStr;
use serde_json::json;

/// Database transaction timestamp.
/// This is unique across all transactions.
/// Units are nanoseconds since epoch.
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd, Ord, Eq, FromStr, Hash, Default)]
pub struct Timestamp(u64);

impl Timestamp {
    // Some SQL and serialization don't support timestamps > i64::MAX,
    // which is fine to use as an upper bound because real timestamps aren't that
    // high.
    pub const MAX: Self = Self(i64::MAX as u64);
    pub const MIN: Self = Self(0);

    #[inline]
    pub fn succ_opt(&self) -> Option<Self> {
        if *self >= Self::MAX {
            None
        } else {
            Some(Self(self.0 + 1))
        }
    }

    pub fn succ(&self) -> anyhow::Result<Self> {
        self.succ_opt()
            .ok_or_else(|| anyhow!("timestamp {self} already at max"))
    }

    #[inline]
    pub fn pred_opt(&self) -> Option<Self> {
        if *self <= Self::MIN {
            None
        } else {
            Some(Self(self.0 - 1))
        }
    }

    pub fn pred(&self) -> anyhow::Result<Self> {
        self.pred_opt()
            .ok_or_else(|| anyhow!("timestamp {self} already at min"))
    }

    pub fn add(&self, duration: Duration) -> anyhow::Result<Self> {
        let nanos = self
            .0
            .checked_add(duration.as_nanos() as u64)
            .with_context(|| format!("timestamp {self} + {duration:?} overflow u64"))?;

        anyhow::ensure!(
            nanos <= u64::from(Self::MAX),
            "timestamp {self} + {duration:?} overflow i64"
        );

        Ok(Self(nanos))
    }

    pub fn sub(&self, duration: Duration) -> anyhow::Result<Self> {
        let nanos = duration.as_nanos() as u64;
        if self.0 <= nanos {
            anyhow::bail!("timestamp {self} already greater than {duration:?}");
        }
        Ok(Self(self.0 - nanos))
    }

    // This is similar to `self - base` but it works if `self` is before `base`.
    // Since Duration is always positive, `self - base` can overflow.
    pub fn secs_since_f64(self, base: Timestamp) -> f64 {
        if self >= base {
            (self - base).as_secs_f64()
        } else {
            -(base - self).as_secs_f64()
        }
    }

    pub fn size(&self) -> usize {
        8
    }

}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Timestamp> for u64 {
    fn from(ts: Timestamp) -> Self {
        ts.0
    }
}

impl From<Timestamp> for i64 {
    fn from(ts: Timestamp) -> Self {
        // This cast is safe because Timestamp checks bounds on construction.
        ts.0 as i64
    }
}

impl TryFrom<i64> for Timestamp {
    type Error = anyhow::Error;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Ok(Timestamp(u64::try_from(value)?))
    }
}

impl TryFrom<u64> for Timestamp {
    type Error = anyhow::Error;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value > Self::MAX.0 {
            anyhow::bail!("ts {value} too large");
        }
        Ok(Timestamp(value))
    }
}

impl TryFrom<SystemTime> for Timestamp {
    type Error = anyhow::Error;

    fn try_from(value: SystemTime) -> Result<Self, Self::Error> {
        let system_ns: u64 = value
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("SystemTime before 1970")?
            .as_nanos()
            .try_into()
            .context("SystemTime past 2262")?;
        Self::try_from(system_ns)
    }
}

impl From<Timestamp> for SystemTime {
    fn from(ts: Timestamp) -> Self {
        SystemTime::UNIX_EPOCH + Duration::from_nanos(ts.0)
    }
}

impl TryFrom<serde_json::Value> for Timestamp {
    type Error = anyhow::Error;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        let ts = value
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("value is not timestamp"))?;
        Timestamp::try_from(ts)
    }
}

impl From<Timestamp> for serde_json::Value {
    fn from(ts: Timestamp) -> Self {
        json!(i64::from(ts))
    }
}

impl Sub for Timestamp {
    type Output = Duration;

    fn sub(self, rhs: Self) -> Self::Output {
        Duration::from_nanos(self.0 - rhs.0)
    }
}

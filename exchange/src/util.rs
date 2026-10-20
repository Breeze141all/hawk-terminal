use serde::{Deserialize, Serialize};

pub type ContractSize = Power10<-4, 6>;
pub type MinTicksize = Power10<-8, 2>;
pub type MinQtySize = Power10<-6, 8>;

#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
pub struct Power10<const MIN: i8, const MAX: i8> {
    pub power: i8,
}

impl<const MIN: i8, const MAX: i8> Power10<MIN, MAX> {
    #[inline]
    pub fn new(power: i8) -> Self {
        Self {
            power: power.clamp(MIN, MAX),
        }
    }

    #[inline]
    pub fn as_f32(self) -> f32 {
        10f32.powi(self.power as i32)
    }
}

impl<const MIN: i8, const MAX: i8> From<Power10<MIN, MAX>> for f32 {
    fn from(v: Power10<MIN, MAX>) -> Self {
        v.as_f32()
    }
}

impl<const MIN: i8, const MAX: i8> From<f32> for Power10<MIN, MAX> {
    fn from(value: f32) -> Self {
        if value <= 0.0 {
            return Self { power: 0 };
        }
        let log10 = value.abs().log10();
        let rounded = log10.round() as i8;
        let power = rounded.clamp(MIN, MAX);
        Self { power }
    }
}

impl<const MIN: i8, const MAX: i8> serde::Serialize for Power10<MIN, MAX> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // serialize as a plain numeric (e.g. 0.1, 1, 10)
        let v: f32 = (*self).into();
        serializer.serialize_f32(v)
    }
}

impl<'de, const MIN: i8, const MAX: i8> serde::Deserialize<'de> for Power10<MIN, MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = f32::deserialize(deserializer)?;
        Ok(Self::from(v))
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct PriceStep {
    /// step size in atomic units (10^-PRICE_SCALE)
    pub units: i64,
}

impl PriceStep {
    /// Decimal places needed to represent this step without trailing zeros.
    pub fn decimal_places(self) -> usize {
        let mut units = self.units.unsigned_abs();
        if units == 0 {
            return 0;
        }

        let mut trailing_zeros = 0u32;
        let max = Price::PRICE_SCALE as u32;
        while trailing_zeros < max && units.is_multiple_of(10) {
            units /= 10;
            trailing_zeros += 1;
        }

        (max - trailing_zeros) as usize
    }

    /// String form for UI labels, trimming trailing fractional zeros.
    pub fn to_ui_string(self) -> String {
        let mut out = String::with_capacity(24);

        if self.units < 0 {
            out.push('-');
        }

        let abs_u = self.units.unsigned_abs();
        let scale = 10u64.pow(Price::PRICE_SCALE as u32);
        let int_part = abs_u / scale;
        out.push_str(&int_part.to_string());

        let frac_raw = abs_u % scale;
        if frac_raw == 0 {
            return out;
        }

        let mut frac = format!("{:0width$}", frac_raw, width = Price::PRICE_SCALE as usize);
        while frac.ends_with('0') {
            frac.pop();
        }

        out.push('.');
        out.push_str(&frac);
        out
    }

    /// Lossy: f32 step for UI
    pub fn to_f32_lossy(self) -> f32 {
        let scale = 10f32.powi(Price::PRICE_SCALE);
        (self.units as f32) / scale
    }

    /// f64 step for UI
    pub fn to_f64_lossy(self) -> f64 {
        let scale = 10f64.powi(Price::PRICE_SCALE);
        (self.units as f64) / scale
    }

    /// Lossy: from f32 step (rounds to nearest atomic unit)
    pub fn from_f32_lossy(step: f32) -> Self {
        assert!(step > 0.0, "step must be > 0");
        let scale = 10f32.powi(Price::PRICE_SCALE);
        let units = (step * scale).round() as i64;
        assert!(units > 0, "step too small at given PRICE_SCALE");
        Self { units }
    }

    pub fn from_f32(step: f32) -> Self {
        Self::from_f32_lossy(step)
    }
}

impl<const MIN: i8, const MAX: i8> From<Power10<MIN, MAX>> for PriceStep {
    fn from(v: Power10<MIN, MAX>) -> Self {
        let exp = Price::PRICE_SCALE + i32::from(v.power);
        assert!(exp >= 0, "invalid Power10 to PriceStep exponent");

        Self {
            units: 10i64
                .checked_pow(exp as u32)
                .expect("Power10 to PriceStep overflowed"),
        }
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(transparent)]
pub struct UnixMs(pub u64);

impl UnixMs {
    pub const ZERO: Self = Self(0);

    #[inline]
    pub const fn new(ms: u64) -> Self {
        Self(ms)
    }

    #[inline]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    #[inline]
    pub const fn from_millis(ms: u64) -> Self {
        Self(ms)
    }

    #[inline]
    pub fn now() -> Self {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self(ms)
    }

    #[inline]
    pub fn as_datetime_utc(self) -> Option<chrono::DateTime<chrono::Utc>> {
        let millis = i64::try_from(self.0).ok()?;
        chrono::DateTime::from_timestamp_millis(millis)
    }

    #[inline]
    pub const fn saturating_add(self, delta_ms: u64) -> Self {
        Self(self.0.saturating_add(delta_ms))
    }

    #[inline]
    pub const fn saturating_sub(self, delta_ms: u64) -> Self {
        Self(self.0.saturating_sub(delta_ms))
    }

    #[inline]
    pub const fn checked_add(self, delta_ms: u64) -> Option<Self> {
        match self.0.checked_add(delta_ms) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    #[inline]
    pub const fn checked_sub(self, delta_ms: u64) -> Option<Self> {
        match self.0.checked_sub(delta_ms) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }
}

impl std::ops::Add for UnixMs {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        self.saturating_add(rhs.0)
    }
}

impl std::ops::Sub for UnixMs {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        self.saturating_sub(rhs.0)
    }
}

impl From<u64> for UnixMs {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<UnixMs> for u64 {
    fn from(v: UnixMs) -> Self {
        v.0
    }
}

impl std::ops::Deref for UnixMs {
    type Target = u64;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for UnixMs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Fixed atomic unit scale: 10^-PRICE_SCALE is the smallest stored fraction.
/// MinTicksize has range [-8, 2], e.g. PRICE_SCALE = 8 to represent 10^-8 atomic units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Deserialize, Serialize)]
pub struct Price {
    /// number of atomic units (atomic unit = 10^-PRICE_SCALE)
    pub units: i64,
}

impl Price {
    pub const ZERO: Self = Self { units: 0 };
    /// number of decimal places of the atomic unit (10^-8)
    pub const PRICE_SCALE: i32 = 8;

    #[inline]
    pub fn to_string<const MIN: i8, const MAX: i8>(self, precision: Power10<MIN, MAX>) -> String {
        let mut out = String::with_capacity(24);
        self.fmt_into(precision, &mut out).unwrap();
        out
    }

    #[inline]
    pub fn fmt_into<const MIN: i8, const MAX: i8, W: core::fmt::Write>(
        self,
        precision: Power10<MIN, MAX>,
        out: &mut W,
    ) -> core::fmt::Result {
        let scale_u = Self::PRICE_SCALE as u32;

        // number of atomic units for the given decade step: 10^(PRICE_SCALE + power)
        let exp = (Self::PRICE_SCALE + precision.power as i32) as u32;
        debug_assert!(Self::PRICE_SCALE + precision.power as i32 >= 0);
        let unit = 10i64
            .checked_pow(exp)
            .expect("Price::to_string unit overflow");

        let u = self.units;
        let half = unit / 2;
        let rounded_units = if u >= 0 {
            ((u + half).div_euclid(unit)) * unit
        } else {
            ((u - half).div_euclid(unit)) * unit
        };

        let decimals: u32 = if precision.power < 0 {
            ((-precision.power) as u32).min(scale_u)
        } else {
            0
        };

        if rounded_units < 0 {
            core::fmt::Write::write_char(out, '-')?;
        }
        let abs_u = (rounded_units as i128).unsigned_abs();

        let scale_pow = 10u128.pow(scale_u);
        let int_part = abs_u / scale_pow;
        write!(out, "{}", int_part)?;

        if decimals == 0 {
            return Ok(());
        }

        let frac_div = 10u128.pow(scale_u - decimals);
        let frac_part = (abs_u % scale_pow) / frac_div;
        write!(out, ".{:0width$}", frac_part, width = decimals as usize)
    }

    /// Lossy: convert price to f32, may lose precision if going beyond `PRICE_SCALE`
    pub fn to_f32_lossy(self) -> f32 {
        let scale = 10f32.powi(Self::PRICE_SCALE);
        (self.units as f32) / scale
    }

    /// Lossy: create Price from f32 (rounds to nearest atomic unit)
    pub fn from_f32_lossy(v: f32) -> Self {
        let scale = 10f32.powi(Self::PRICE_SCALE);
        let u = (v * scale).round() as i64;
        Self { units: u }
    }

    pub fn from_f32(v: f32) -> Self {
        Self::from_f32_lossy(v)
    }

    #[inline]
    pub fn from_f64(v: f64) -> Self {
        let scale = 10f64.powi(Self::PRICE_SCALE);
        let u = (v * scale).round() as i64;
        Self { units: u }
    }

    pub fn to_f32(self) -> f32 {
        self.to_f32_lossy()
    }

    #[inline]
    pub fn to_f64(self) -> f64 {
        (self.units as f64) / 10f64.powi(Self::PRICE_SCALE)
    }

    pub fn round_to_step(self, step: PriceStep) -> Self {
        let unit = step.units;
        if unit <= 1 {
            return self;
        }
        let half = unit / 2;
        let rounded = ((self.units + half).div_euclid(unit)) * unit;
        Self { units: rounded }
    }

    /// Floor to multiple of an arbitrary step
    fn floor_to_step(self, step: PriceStep) -> Self {
        let unit = step.units;
        if unit <= 1 {
            return self;
        }
        let floored = (self.units.div_euclid(unit)) * unit;
        Self { units: floored }
    }

    /// Ceil to multiple of an arbitrary step
    fn ceil_to_step(self, step: PriceStep) -> Self {
        let unit = step.units;
        if unit <= 1 {
            return self;
        }
        let added = self.units.checked_add(unit - 1).unwrap_or_else(|| {
            if self.units.is_negative() {
                i64::MIN
            } else {
                i64::MAX
            }
        });

        let ceiled = (added.div_euclid(unit)) * unit;
        Self { units: ceiled }
    }

    /// Group with arbitrary step (e.g. sells floor, buys ceil)
    pub fn round_to_side_step(self, is_sell_or_bid: bool, step: PriceStep) -> Self {
        if is_sell_or_bid {
            self.floor_to_step(step)
        } else {
            self.ceil_to_step(step)
        }
    }

    /// Difference of two prices, saturating instead of panicking on overflow
    pub fn saturating_sub(self, rhs: Self) -> Self {
        Self {
            units: self.units.saturating_sub(rhs.units),
        }
    }

    /// Normalized position of `self` within `[low, high]`: `0.0` at `low`,
    /// `1.0` at `high`. Returns `0.0` for an empty range.
    pub fn ratio_in_range(self, low: Self, high: Self) -> f64 {
        let range = high.units.saturating_sub(low.units);
        if range == 0 {
            0.0
        } else {
            self.units.saturating_sub(low.units) as f64 / range as f64
        }
    }

    /// Create Price from raw atomic units (no rounding) — internal only
    pub fn from_units(units: i64) -> Self {
        Self { units }
    }

    /// Returns the atomic-unit count that corresponds to one min tick (min_tick / atomic_unit)
    fn min_tick_units(min_tick: MinTicksize) -> i64 {
        let exp = Self::PRICE_SCALE + (min_tick.power as i32);
        assert!(exp >= 0, "PRICE_SCALE must be >= -min_tick.power");
        10i64
            .checked_pow(exp as u32)
            .expect("min_tick_units overflowed")
    }

    /// Round this Price to the nearest multiple of the provided min_ticksize
    pub fn round_to_min_tick(self, min_tick: MinTicksize) -> Self {
        let unit = Self::min_tick_units(min_tick);
        if unit <= 1 {
            return self;
        }
        let half = unit / 2;
        let rounded = ((self.units + half).div_euclid(unit)) * unit;
        Self { units: rounded }
    }

    pub fn add_steps(self, steps: i64, step: PriceStep) -> Self {
        Self::from_units(
            self.units
                .checked_add(steps.saturating_mul(step.units))
                .expect("add_steps overflowed"),
        )
    }

    /// Number of step increments between low..=high (inclusive), or None if invalid.
    pub fn steps_between_inclusive(low: Price, high: Price, step: PriceStep) -> Option<usize> {
        if high.units < low.units || step.units <= 0 {
            return None;
        }
        let span = high.units.checked_sub(low.units)?;
        Some((span / step.units) as usize + 1)
    }
}

impl std::ops::Add for Price {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            units: self
                .units
                .checked_add(rhs.units)
                .expect("Price add overflowed"),
        }
    }
}

impl std::ops::Div<i64> for Price {
    type Output = Self;

    fn div(self, rhs: i64) -> Self::Output {
        Self {
            units: self.units.div_euclid(rhs),
        }
    }
}

impl std::ops::Sub for Price {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            units: self
                .units
                .checked_sub(rhs.units)
                .expect("Price sub overflowed"),
        }
    }
}

#[cfg(test)]
mod manual_printouts {
    use super::*;

    #[test]
    fn show_min_tick_rounding() {
        let orig: f32 = 0.000051;
        let p = Price::from_f32_lossy(orig);
        let back = p.to_f32_lossy();

        let scale = 10f32.powi(Price::PRICE_SCALE);
        let expected_units = (orig * scale).round() as i64;
        let expected_back = (expected_units as f32) / scale;

        println!("orig (f32)        = {:0.9}", orig);
        println!("orig bits         = 0x{:08x}", orig.to_bits());
        println!("price units       = {}", p.units);
        println!("expected units    = {}", expected_units);
        println!("back (from units) = {:0.9}", back);
        println!("expected back     = {:0.9}", expected_back);
        println!("orig - back       = {:+.9e}", orig - back);
        println!("back == expected  = {}", back == expected_back);
    }
}

use rand::Rng;
use serde_json::value::Number as JsonNumber;

use super::OpProvider;

#[convex_macro::op]
pub fn op_random<P: OpProvider>(provider: &mut P) -> anyhow::Result<JsonNumber> {
    let rng = provider.rng()?;
    let n =
        JsonNumber::from_f64(rng.random()).expect("f64's distribution returned a NaN or infinity?");
    Ok(n)
}

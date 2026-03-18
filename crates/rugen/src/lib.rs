use std::collections::BTreeMap;

pub use rune;

use rand::{
    RngExt,
    seq::{IndexedRandom, WeightError},
};
use rune::{
    Any, ContextError, FromValue, Module, Value,
    alloc::{self, Result, String as RuneString},
    runtime::{Object, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RuntimeError},
};

#[cfg(feature = "fmt")]
use rune::{Diagnostics, Source, Sources};

#[cfg(feature = "fmt")]
pub fn format_rune_script<P: AsRef<std::path::Path>>(
    script: P,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut sources = Sources::new();

    sources.insert(match Source::from_path(&script) {
        Ok(source) => source,
        Err(error) => return Err(Box::new(error)),
    })?;

    let mut diagnostics = Diagnostics::new();

    let options = rune::Options::default();

    let build = rune::fmt::prepare(&mut sources)
        .with_options(&options)
        .with_diagnostics(&mut diagnostics);

    let result = build.format();

    if !diagnostics.is_empty() {
        let mut writer =
            rune::termcolor::StandardStream::stdout(rune::termcolor::ColorChoice::Always);
        diagnostics.emit(&mut writer, &sources)?;
    }

    let formatted = result?;

    let formatted = &formatted.first().unwrap().1;
    std::fs::write(&script, formatted)?;
    Ok(())
}

#[derive(Any, thiserror::Error, Debug)]
pub enum RuGenError {
    #[error("Invalid range start")]
    InvalidRangeStart,
    #[error("Invalid range end")]
    InvalidRangeEnd,
    #[error("Unsupported type")]
    UnsupportedType,
    #[error("Min and max must be of the same type")]
    MinMaxTypeMismatch,
    #[error("Invalid probability: {0}! Value must be between 0 and 1 inclusive")]
    InvalidProbability(f64),
    #[error("Vec must have at least one value to choose from")]
    NoValueToChooseFrom,
    #[error("Count must be non-negative")]
    CountMustBeNonNegative,
    #[error("RuntimeError: {0}")]
    RuntimeError(#[from] RuntimeError),
    #[error("alloc::Error: {0}")]
    AllocError(#[from] alloc::Error),
    #[error("WeightError: {0}")]
    WeightError(#[from] WeightError),
}

#[derive(Any, Debug)]
pub enum DataDescription {
    Bool,
    Just(Value),
    UInt {
        min: u64,
        max: u64,
        inclusive: bool,
    },
    Int {
        min: i64,
        max: i64,
        inclusive: bool,
    },
    Char {
        min: char,
        max: char,
        inclusive: bool,
    },
    Float {
        min: f64,
        max: f64,
        inclusive: bool,
    },
    String {
        len: Box<DataDescription>,
    },
    Choice(Vec<DataDescription>),
    Weighted(Vec<(u32, DataDescription)>),
    FixedLengthArray {
        count: i64,
        value: Box<DataDescription>,
    },
    VariableLengthArray {
        count: Box<DataDescription>,
        value: Box<DataDescription>,
    },
    Object(BTreeMap<String, DataDescription>),
    Optional {
        p: f64,
        value: Box<DataDescription>,
    },
    Vec(Vec<DataDescription>),
}

impl DataDescription {
    pub fn evaluate(&self) -> Result<Value, RuGenError> {
        let mut rng = rand::rng();
        match self {
            DataDescription::Just(v) => Ok(v.clone()),
            DataDescription::String { len } => {
                let s: String = rng
                    .sample_iter(rand::distr::Alphanumeric)
                    .take(len.evaluate()?.as_usize()?)
                    .map(char::from)
                    .collect();
                Ok(rune::to_value(s)?)
            }
            DataDescription::Choice(values) => {
                let mut rng = rand::rng();
                values.choose(&mut rng).map_or_else(
                    || Err(RuGenError::NoValueToChooseFrom),
                    DataDescription::evaluate,
                )
            }
            DataDescription::VariableLengthArray {
                count,
                value: value_description,
            } => Ok(rune::to_value(
                (0..count.evaluate()?.as_usize()?)
                    .map(|_| value_description.evaluate())
                    .collect::<Result<Vec<Value>, RuGenError>>()?,
            )?),
            DataDescription::FixedLengthArray {
                count,
                value: value_description,
            } => Ok(rune::to_value(
                (0..*count)
                    .map(|_| value_description.evaluate())
                    .collect::<Result<Vec<Value>, RuGenError>>()?,
            )?),
            DataDescription::Object(obj) => {
                let mut new_obj = Object::new();
                for (k, v) in obj {
                    let mut new_str = RuneString::new();
                    new_str.try_push_str(k)?;
                    new_obj.insert(new_str, v.evaluate()?)?;
                }
                Ok(rune::to_value(new_obj)?)
            }
            DataDescription::Optional { p, value } => {
                let mut rng = rand::rng();
                Ok(rune::to_value(
                    (rng.random::<f64>() < *p)
                        .then(|| value.evaluate())
                        .transpose()?,
                )?)
            }
            DataDescription::Vec(values) => {
                let mut v = Vec::new();
                for desc in values {
                    v.push(desc.evaluate()?);
                }
                Ok(rune::to_value(v)?)
            }
            DataDescription::Bool => Ok(rune::to_value(rng.random::<bool>())?),
            DataDescription::UInt {
                min,
                max,
                inclusive,
            } => Ok(rune::to_value(if *inclusive {
                rng.random_range(*min..=*max)
            } else {
                rng.random_range(*min..*max)
            })?),
            DataDescription::Int {
                min,
                max,
                inclusive,
            } => Ok(rune::to_value(if *inclusive {
                rng.random_range(*min..=*max)
            } else {
                rng.random_range(*min..*max)
            })?),
            DataDescription::Char {
                min,
                max,
                inclusive,
            } => Ok(rune::to_value(if *inclusive {
                rng.random_range(*min..=*max)
            } else {
                rng.random_range(*min..*max)
            })?),
            DataDescription::Float {
                min,
                max,
                inclusive,
            } => Ok(rune::to_value(if *inclusive {
                rng.random_range(*min..=*max)
            } else {
                rng.random_range(*min..*max)
            })?),
            DataDescription::Weighted(values) => {
                let (_, v) = values.choose_weighted(&mut rng, |v| v.0)?;
                Ok(rune::to_value(v.evaluate()?)?)
            }
        }
    }
}

pub fn checked_from_value<T: FromValue>(value: &Value) -> Result<T, RuGenError> {
    if let Ok(v) = rune::from_value::<Result<Value, RuGenError>>(value) {
        Ok(rune::from_value(v?)?)
    } else {
        Ok(rune::from_value(value.to_owned())?)
    }
}

#[rune::function]
fn bool() -> DataDescription {
    DataDescription::Bool
}

fn range_impl(min: &Value, max: &Value, inclusive: bool) -> Result<DataDescription, RuGenError> {
    if min.type_info() != max.type_info() {
        return Err(RuGenError::MinMaxTypeMismatch);
    }
    match min {
        min if min.as_integer::<u64>().is_ok() => {
            let min = min
                .as_integer::<u64>()
                .map_err(|_| RuGenError::InvalidRangeStart)?;
            let max = max
                .as_integer::<u64>()
                .map_err(|_| RuGenError::InvalidRangeEnd)?;

            Ok(DataDescription::UInt {
                min,
                max,
                inclusive,
            })
        }
        min if min.as_integer::<i64>().is_ok() => {
            let min = min
                .as_integer::<i64>()
                .map_err(|_| RuGenError::InvalidRangeStart)?;
            let max = max
                .as_integer::<i64>()
                .map_err(|_| RuGenError::InvalidRangeEnd)?;
            Ok(DataDescription::Int {
                min,
                max,
                inclusive,
            })
        }
        min if min.as_float().is_ok() => {
            let min = min.as_float().map_err(|_| RuGenError::InvalidRangeStart)?;
            let max = max.as_float().map_err(|_| RuGenError::InvalidRangeEnd)?;
            Ok(DataDescription::Float {
                min,
                max,
                inclusive,
            })
        }
        min if min.as_char().is_ok() => {
            let min = min.as_char().map_err(|_| RuGenError::InvalidRangeStart)?;
            let max = max.as_char().map_err(|_| RuGenError::InvalidRangeEnd)?;
            Ok(DataDescription::Char {
                min,
                max,
                inclusive,
            })
        }
        _ => Err(RuGenError::UnsupportedType),
    }
}

#[rune::function]
#[expect(clippy::needless_pass_by_value)]
fn range(min: Value, max: Value) -> Result<DataDescription, RuGenError> {
    range_impl(&min, &max, false)
}

#[rune::function]
#[expect(clippy::needless_pass_by_value)]
fn range_inclusive(min: Value, max: Value) -> Result<DataDescription, RuGenError> {
    range_impl(&min, &max, true)
}

fn value_min(value: &Value) -> Option<Value> {
    if value.as_integer::<i64>().is_ok() {
        Some(rune::to_value(i64::MIN).expect("Failed to convert i64::MIN to Value"))
    } else if value.as_integer::<u64>().is_ok() {
        Some(rune::to_value(u64::MIN).expect("Failed to convert u64::MIN to Value"))
    } else if value.as_float().is_ok() {
        Some(rune::to_value(f64::MIN).expect("Failed to convert f64::MIN to Value"))
    } else if value.as_char().is_ok() {
        Some(rune::to_value(char::MIN).expect("Failed to convert char::MIN to Value"))
    } else {
        None
    }
}

fn value_max(value: &Value) -> Option<Value> {
    if value.as_integer::<i64>().is_ok() {
        Some(rune::to_value(i64::MAX).expect("Failed to convert i64::MAX to Value"))
    } else if value.as_integer::<u64>().is_ok() {
        Some(rune::to_value(u64::MAX).expect("Failed to convert u64::MAX to Value"))
    } else if value.as_float().is_ok() {
        Some(rune::to_value(f64::MAX).expect("Failed to convert f64::MAX to Value"))
    } else if value.as_char().is_ok() {
        Some(rune::to_value(char::MAX).expect("Failed to convert char::MAX to Value"))
    } else {
        None
    }
}

impl TryFrom<&Value> for DataDescription {
    type Error = RuGenError;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        let value = checked_from_value(value)?;
        if let Ok(desc) = rune::from_value::<DataDescription>(&value) {
            Ok(desc)
        } else if let Ok(obj) = rune::from_value::<Object>(&value) {
            Ok(DataDescription::Object(
                obj.into_iter()
                    .map(|(k, v)| v.try_into().map(|v| (k.as_str().to_string(), v)))
                    .collect::<Result<_, _>>()?,
            ))
        } else if let Ok(range) = rune::from_value::<Range>(&value) {
            range_impl(&range.start, &range.end, false)
        } else if let Ok(range) = rune::from_value::<RangeInclusive>(&value) {
            range_impl(&range.start, &range.end, true)
        } else if let Ok(range) = rune::from_value::<RangeFrom>(&value) {
            let max = value_max(&range.start).ok_or(RuGenError::UnsupportedType)?;
            range_impl(&range.start, &max, true)
        } else if let Ok(range) = rune::from_value::<RangeTo>(&value) {
            let min = value_min(&range.end).ok_or(RuGenError::UnsupportedType)?;
            range_impl(&min, &range.end, false)
        } else if rune::from_value::<RangeFull>(&value).is_ok() {
            Err(RuGenError::UnsupportedType)
        } else if let Ok(s) = rune::from_value::<Vec<Value>>(&value) {
            Ok(DataDescription::Vec(
                s.into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?,
            ))
        } else {
            Ok(DataDescription::Just(value))
        }
    }
}

impl TryFrom<Value> for DataDescription {
    type Error = RuGenError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        DataDescription::try_from(&value)
    }
}

#[rune::function]
fn string(len: Value) -> Result<DataDescription, RuGenError> {
    Ok(DataDescription::String {
        len: Box::new(len.try_into()?),
    })
}

#[rune::function]
fn choose(values: Vec<Value>) -> Result<DataDescription, RuGenError> {
    if values.is_empty() {
        return Err(RuGenError::NoValueToChooseFrom);
    }
    Ok(DataDescription::Choice(
        values
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?,
    ))
}

#[rune::function(instance)]
fn values(count: i64, value_description: Value) -> Result<DataDescription, RuGenError> {
    if count < 0 {
        return Err(RuGenError::CountMustBeNonNegative);
    }
    Ok(DataDescription::FixedLengthArray {
        count,
        value: Box::new(value_description.try_into()?),
    })
}

#[rune::function(path = values)]
fn variable_values(count: Value, value: Value) -> Result<DataDescription, RuGenError> {
    Ok(DataDescription::VariableLengthArray {
        count: Box::new(count.try_into()?),
        value: Box::new(value.try_into()?),
    })
}

#[rune::function]
fn weighted(values: Vec<(u32, Value)>) -> Result<DataDescription, RuGenError> {
    if values.is_empty() {
        return Err(RuGenError::NoValueToChooseFrom);
    }
    Ok(DataDescription::Weighted(
        values
            .into_iter()
            .map(|(w, v)| v.try_into().map(|v| (w, v)))
            .collect::<Result<_, _>>()?,
    ))
}

#[rune::function]
fn optional(p: Value, value: Value) -> Result<DataDescription, RuGenError> {
    let p = checked_from_value(&p)?;
    if !(0.0..=1.0).contains(&p) {
        return Err(RuGenError::InvalidProbability(p));
    }
    Ok(DataDescription::Optional {
        p,
        value: Box::new(value.try_into()?),
    })
}

#[rune::function]
fn describe(this: Value) -> Result<DataDescription, RuGenError> {
    this.try_into()
}

pub fn module() -> Result<Module, ContextError> {
    let mut m = Module::with_item(["rugen"])?;
    m.ty::<DataDescription>()?;
    m.function_meta(describe)?;
    m.function_meta(bool)?;
    m.function_meta(string)?;
    m.function_meta(optional)?;
    m.function_meta(range)?;
    m.function_meta(range_inclusive)?;
    m.function_meta(choose)?;
    m.function_meta(weighted)?;
    m.function_meta(values)?;
    m.function_meta(variable_values)?;
    Ok(m)
}

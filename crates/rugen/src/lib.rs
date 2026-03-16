use std::collections::BTreeMap;

use rand::{
    RngExt,
    seq::{IndexedRandom, WeightError},
};
use rune::{
    Any, ContextError, Module, Value,
    alloc::{self, Result, String as RuneString},
    runtime::{Object, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RuntimeError},
};

#[derive(Any, thiserror::Error, Debug)]
pub enum RuGenError {
    #[error("Invalid range start")]
    InvalidRangeStart,
    #[error("Invalid range end")]
    InvalidRangeEnd,
    #[error("Unsupported type")]
    UnsupportedType,
    #[error("Invalid probability: {0}! Value must be between 0 and 1 inclusive")]
    InvalidProbability(f64),
    #[error("Vec must have at least one item to choose from")]
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
    OneOf(Vec<DataDescription>),
    Weighted(Vec<(u32, DataDescription)>),
    Array {
        len: Box<DataDescription>,
        item: Box<DataDescription>,
    },
    Object(BTreeMap<String, DataDescription>),
    Optional {
        p: f64,
        item: Box<DataDescription>,
    },
    Tuple(Vec<DataDescription>),
}

#[rune::function]
fn just(value: Value) -> DataDescription {
    DataDescription::Just(value)
}

#[rune::function]
fn bool() -> DataDescription {
    DataDescription::Bool
}

fn range_impl(min: &Value, max: &Value, inclusive: bool) -> Result<DataDescription, RuGenError> {
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
        if let Ok(v) = rune::from_value::<Result<Value, RuGenError>>(value) {
            Ok(v?.try_into()?)
        } else if let Ok(desc) = rune::from_value::<DataDescription>(value) {
            Ok(desc)
        } else if let Ok(obj) = rune::from_value::<Object>(value) {
            Ok(DataDescription::Object(
                obj.into_iter()
                    .map(|(k, v)| v.try_into().map(|v| (k.as_str().to_string(), v)))
                    .collect::<Result<_, _>>()?,
            ))
        } else if let Ok(range) = rune::from_value::<Range>(value) {
            range_impl(&range.start, &range.end, false)
        } else if let Ok(range) = rune::from_value::<RangeInclusive>(value) {
            range_impl(&range.start, &range.end, true)
        } else if let Ok(range) = rune::from_value::<RangeFrom>(value) {
            let max = value_max(&range.start).ok_or(RuGenError::UnsupportedType)?;
            range_impl(&range.start, &max, true)
        } else if let Ok(range) = rune::from_value::<RangeTo>(value) {
            let min = value_min(&range.end).ok_or(RuGenError::UnsupportedType)?;
            range_impl(&min, &range.end, false)
        } else if rune::from_value::<RangeFull>(value).is_ok() {
            Err(RuGenError::UnsupportedType)
        } else if let Ok(s) = rune::from_value::<Vec<Value>>(value) {
            Ok(DataDescription::Tuple(
                s.into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()?,
            ))
        } else {
            Ok(DataDescription::Just(value.to_owned()))
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

#[rune::function(path = choose)]
fn choose_one(values: Vec<Value>) -> Result<DataDescription, RuGenError> {
    if values.is_empty() {
        return Err(RuGenError::NoValueToChooseFrom);
    }
    Ok(DataDescription::OneOf(
        values
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?,
    ))
}

#[rune::function(instance)]
fn choose(values: Vec<Value>) -> Result<DataDescription, RuGenError> {
    if values.is_empty() {
        return Err(RuGenError::NoValueToChooseFrom);
    }
    Ok(DataDescription::OneOf(
        values
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?,
    ))
}

#[rune::function(instance)]
fn values(count: i64, value: Value) -> Result<DataDescription, RuGenError> {
    if count < 0 {
        return Err(RuGenError::CountMustBeNonNegative);
    }
    let count = rune::to_value(count)?;
    Ok(DataDescription::Array {
        len: Box::new(DataDescription::Just(count)),
        item: Box::new(value.try_into()?),
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
fn array(len: Value, item: Value) -> Result<DataDescription, RuGenError> {
    if let Ok(v) = len.as_integer::<i64>()
        && v < 0
    {
        return Err(RuGenError::CountMustBeNonNegative);
    }
    Ok(DataDescription::Array {
        len: Box::new(len.try_into()?),
        item: Box::new(item.try_into()?),
    })
}

fn object_impl(obj: &Object) -> Result<DataDescription, RuGenError> {
    Ok(DataDescription::Object(
        obj.into_iter()
            .map(|(k, v)| v.try_into().map(|v| (k.as_str().to_string(), v)))
            .collect::<Result<_, _>>()?,
    ))
}

#[rune::function]
fn object(obj: &Object) -> Result<DataDescription, RuGenError> {
    object_impl(obj)
}

#[rune::function]
fn optional(p: Value, item: Value) -> Result<DataDescription, RuGenError> {
    let p = rune::from_value(p)?;
    if !(0.0..=1.0).contains(&p) {
        return Err(RuGenError::InvalidProbability(p));
    }
    Ok(DataDescription::Optional {
        p,
        item: Box::new(item.try_into()?),
    })
}

#[rune::function]
fn tuple(items: Vec<Value>) -> Result<DataDescription, RuGenError> {
    Ok(DataDescription::Tuple(
        items
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()?,
    ))
}

pub fn generate(this: &DataDescription) -> Result<Value, RuGenError> {
    let mut rng = rand::rng();
    match this {
        DataDescription::Just(v) => Ok(v.clone()),
        DataDescription::String { len } => {
            let s: String = rng
                .sample_iter(rand::distr::Alphanumeric)
                .take(generate(len)?.as_usize()?)
                .map(char::from)
                .collect();
            Ok(rune::to_value(s)?)
        }
        DataDescription::OneOf(values) => {
            let mut rng = rand::rng();
            values
                .choose(&mut rng)
                .map_or_else(|| Err(RuGenError::NoValueToChooseFrom), generate)
        }
        DataDescription::Array { len, item } => Ok(rune::to_value(
            (0..generate(len)?.as_usize()?)
                .map(|_| generate(item))
                .collect::<Result<Vec<Value>, RuGenError>>()?,
        )?),
        DataDescription::Object(obj) => {
            let mut new_obj = Object::new();
            for (k, v) in obj {
                let mut new_str = RuneString::new();
                new_str.try_push_str(k)?;
                new_obj.insert(new_str, generate(v)?)?;
            }
            Ok(rune::to_value(new_obj)?)
        }
        DataDescription::Optional { p, item } => {
            let mut rng = rand::rng();
            Ok(rune::to_value(
                (rng.random::<f64>() < *p)
                    .then(|| generate(item))
                    .transpose()?,
            )?)
        }
        DataDescription::Tuple(values) => {
            let mut v = Vec::new();
            for desc in values {
                v.push(generate(desc)?);
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
        DataDescription::Weighted(items) => {
            let (_, v) = items.choose_weighted(&mut rng, |v| v.0)?;
            Ok(rune::to_value(generate(v))?)
        }
    }
}

#[rune::function]
fn describe(this: Value) -> Result<Value, RuGenError> {
    let desc: DataDescription = this.try_into()?;
    Ok(rune::to_value(desc)?)
}

#[rune::function(instance, path = to_description)]
fn describe_object(this: &Object) -> Result<Value, RuGenError> {
    let desc = object_impl(this)?;
    Ok(rune::to_value(desc)?)
}

#[rune::function(instance, path = to_description)]
fn describe_vec(this: Vec<Value>) -> Result<Value, RuGenError> {
    let desc = DataDescription::Array {
        len: Box::new(DataDescription::Just(rune::to_value(this.len())?)),
        item: Box::new(DataDescription::Tuple(
            this.into_iter()
                .map(DataDescription::try_from)
                .collect::<Result<_, _>>()?,
        )),
    };
    Ok(rune::to_value(desc)?)
}

#[rune::function(instance, path = generate)]
fn generate_data(this: &DataDescription) -> Result<Value, RuGenError> {
    generate(this)
}

pub fn module() -> Result<Module, ContextError> {
    let mut m = Module::with_item(["rugen"])?;
    m.ty::<DataDescription>()?;
    m.function_meta(range)?;
    m.function_meta(range_inclusive)?;
    m.function_meta(just)?;
    m.function_meta(bool)?;
    m.function_meta(string)?;
    m.function_meta(choose_one)?;
    m.function_meta(choose)?;
    m.function_meta(weighted)?;
    m.function_meta(array)?;
    m.function_meta(object)?;
    m.function_meta(optional)?;
    m.function_meta(tuple)?;
    m.function_meta(values)?;
    m.function_meta(describe_object)?;
    m.function_meta(describe_vec)?;
    m.function_meta(describe)?;
    m.function_meta(generate_data)?;
    Ok(m)
}

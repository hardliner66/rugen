use std::sync::Arc;

use rand::{RngExt, seq::IndexedRandom};
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
    #[cfg(not(feature = "allow_empty"))]
    #[error("Vec must have at least one item to choose from")]
    NoValueToChooseFrom,
    #[cfg(not(feature = "allow_empty"))]
    #[error("Count must be non-negative")]
    CountMustBeNonNegative,
    #[error("RuntimeError: {0}")]
    RuntimeError(Arc<RuntimeError>),
    #[error("alloc::Error: {0}")]
    AllocError(#[from] alloc::Error),
}

impl From<RuntimeError> for RuGenError {
    fn from(e: RuntimeError) -> Self {
        RuGenError::RuntimeError(Arc::new(e))
    }
}

impl Clone for RuGenError {
    fn clone(&self) -> Self {
        match self {
            RuGenError::InvalidRangeStart => RuGenError::InvalidRangeStart,
            RuGenError::InvalidRangeEnd => RuGenError::InvalidRangeEnd,
            RuGenError::UnsupportedType => RuGenError::UnsupportedType,
            #[cfg(not(feature = "allow_empty"))]
            RuGenError::NoValueToChooseFrom => RuGenError::NoValueToChooseFrom,
            #[cfg(not(feature = "allow_empty"))]
            RuGenError::CountMustBeNonNegative => RuGenError::CountMustBeNonNegative,
            RuGenError::RuntimeError(e) => RuGenError::RuntimeError(e.clone()),
            RuGenError::AllocError(e) => RuGenError::AllocError(*e),
        }
    }
}

#[derive(Any, Debug)]
pub enum DataDescription {
    Bool,
    Just(Value),
    UInt {
        min: u64,
        max: u64,
    },
    Int {
        min: i64,
        max: i64,
    },
    Char {
        min: char,
        max: char,
    },
    Float {
        min: f64,
        max: f64,
    },
    String {
        len: Box<DataDescription>,
        min: char,
        max: char,
    },
    AlphaNumeric {
        len: Box<DataDescription>,
    },
    OneOf(Vec<DataDescription>),
    Weighted(Vec<(u32, DataDescription)>),
    Array {
        len: Box<DataDescription>,
        item: Box<DataDescription>,
    },
    Object(Object),
    Optional {
        p: Box<DataDescription>,
        item: Box<DataDescription>,
    },
    Tuple(Vec<DataDescription>),
    Error(RuGenError),
}

impl DataDescription {
    pub fn generate(self) -> Result<Value, RuGenError> {
        generate_impl(self)
    }
}

impl Clone for DataDescription {
    fn clone(&self) -> Self {
        match self {
            DataDescription::Bool => DataDescription::Bool,
            DataDescription::Just(v) => DataDescription::Just(v.clone()),
            DataDescription::UInt { min, max } => DataDescription::UInt {
                min: *min,
                max: *max,
            },
            DataDescription::Int { min, max } => DataDescription::Int {
                min: *min,
                max: *max,
            },
            DataDescription::Char { min, max } => DataDescription::Char {
                min: *min,
                max: *max,
            },
            DataDescription::Float { min, max } => DataDescription::Float {
                min: *min,
                max: *max,
            },
            DataDescription::String { len, min, max } => DataDescription::String {
                len: Box::new((**len).clone()),
                min: *min,
                max: *max,
            },
            DataDescription::AlphaNumeric { len } => DataDescription::AlphaNumeric {
                len: Box::new((**len).clone()),
            },
            DataDescription::OneOf(values) => DataDescription::OneOf(values.clone()),
            DataDescription::Weighted(items) => {
                DataDescription::Weighted(items.iter().map(|(w, v)| (*w, v.clone())).collect())
            }
            DataDescription::Array { len, item } => DataDescription::Array {
                len: Box::new((**len).clone()),
                item: Box::new((**item).clone()),
            },
            DataDescription::Object(obj) => {
                let mut new_obj = Object::new();
                for (k, v) in obj {
                    match (clone_rune_string(k), generate_impl(v.into())) {
                        (Ok(k), Ok(v)) => {
                            if let Err(e) = new_obj.insert(k, v) {
                                return DataDescription::Error(RuGenError::AllocError(e));
                            }
                        }
                        (Err(e), _) => {
                            return DataDescription::Error(RuGenError::RuntimeError(Arc::new(e)));
                        }
                        (_, Err(e)) => {
                            return DataDescription::Error(e);
                        }
                    }
                }
                DataDescription::Object(new_obj)
            }
            DataDescription::Optional { p, item } => DataDescription::Optional {
                p: Box::new((**p).clone()),
                item: Box::new((**item).clone()),
            },
            DataDescription::Tuple(values) => DataDescription::Tuple(values.clone()),
            DataDescription::Error(e) => DataDescription::Error(e.clone()),
        }
    }
}

impl Default for DataDescription {
    fn default() -> Self {
        DataDescription::Error(RuGenError::UnsupportedType)
    }
}

#[rune::function]
fn just(value: Value) -> DataDescription {
    DataDescription::Just(value)
}

#[rune::function]
fn literal(value: Value) -> DataDescription {
    DataDescription::Just(value)
}

#[rune::function]
fn bool() -> DataDescription {
    DataDescription::Bool
}

fn range_impl(min: &Value, max: &Value) -> DataDescription {
    match min {
        min if min.as_integer::<u64>().is_ok() => {
            let Ok(min) = min.as_integer::<u64>() else {
                return DataDescription::Error(RuGenError::InvalidRangeStart);
            };
            let Ok(max) = max.as_integer::<u64>() else {
                return DataDescription::Error(RuGenError::InvalidRangeEnd);
            };
            DataDescription::UInt { min, max }
        }
        min if min.as_integer::<i64>().is_ok() => {
            let Ok(min) = min.as_integer::<i64>() else {
                return DataDescription::Error(RuGenError::InvalidRangeStart);
            };
            let Ok(max) = max.as_integer::<i64>() else {
                return DataDescription::Error(RuGenError::InvalidRangeEnd);
            };
            DataDescription::Int { min, max }
        }
        min if min.as_float().is_ok() => {
            let Ok(min) = min.as_float() else {
                return DataDescription::Error(RuGenError::InvalidRangeStart);
            };
            let Ok(max) = max.as_float() else {
                return DataDescription::Error(RuGenError::InvalidRangeEnd);
            };
            DataDescription::Float { min, max }
        }
        min if min.as_char().is_ok() => {
            let Ok(min) = min.as_char() else {
                return DataDescription::Error(RuGenError::InvalidRangeStart);
            };
            let Ok(max) = max.as_char() else {
                return DataDescription::Error(RuGenError::InvalidRangeEnd);
            };
            DataDescription::Char { min, max }
        }
        _ => DataDescription::Error(RuGenError::UnsupportedType),
    }
}

#[rune::function]
#[expect(clippy::needless_pass_by_value)]
fn range(min: Value, max: Value) -> DataDescription {
    range_impl(&min, &max)
}

#[rune::function]
fn char(min: char, max: char) -> DataDescription {
    DataDescription::Char { min, max }
}

#[rune::function]
fn uint(min: u64, max: u64) -> DataDescription {
    DataDescription::UInt { min, max }
}

#[rune::function]
fn int(min: i64, max: i64) -> DataDescription {
    DataDescription::Int { min, max }
}

#[rune::function]
fn float(min: f64, max: f64) -> DataDescription {
    DataDescription::Float { min, max }
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

impl From<&Value> for DataDescription {
    fn from(value: &Value) -> Self {
        if let Ok(desc) = rune::from_value::<DataDescription>(value) {
            desc
        } else if let Ok(obj) = rune::from_value::<Object>(value) {
            DataDescription::Object(obj)
        } else if let Ok(range) = rune::from_value::<Range>(value) {
            range_impl(&range.start, &range.end)
        } else if let Ok(range) = rune::from_value::<RangeInclusive>(value) {
            range_impl(&range.start, &range.end)
        } else if let Ok(range) = rune::from_value::<RangeFrom>(value) {
            let Some(max) = value_max(&range.start) else {
                return DataDescription::Error(RuGenError::UnsupportedType);
            };
            range_impl(&range.start, &max)
        } else if let Ok(range) = rune::from_value::<RangeTo>(value) {
            let Some(min) = value_min(&range.end) else {
                return DataDescription::Error(RuGenError::UnsupportedType);
            };
            range_impl(&min, &range.end)
        } else if rune::from_value::<RangeFull>(value).is_ok() {
            DataDescription::Error(RuGenError::UnsupportedType)
        } else if let Ok(s) = rune::from_value::<Vec<Value>>(value) {
            DataDescription::Tuple(s.into_iter().map(Into::into).collect())
        } else {
            DataDescription::Just(value.to_owned())
        }
    }
}

impl From<Value> for DataDescription {
    fn from(value: Value) -> Self {
        DataDescription::from(&value)
    }
}

#[rune::function]
fn alphanumeric(len: Value) -> DataDescription {
    DataDescription::AlphaNumeric {
        len: Box::new(len.into()),
    }
}

#[rune::function]
fn string(len: Value, min: char, max: char) -> DataDescription {
    DataDescription::String {
        len: Box::new(len.into()),
        min,
        max,
    }
}

#[rune::function]
fn one_of(values: Vec<Value>) -> DataDescription {
    DataDescription::OneOf(values.into_iter().map(Into::into).collect())
}

#[rune::function(path = choose)]
fn choose_one(values: Vec<Value>) -> DataDescription {
    DataDescription::OneOf(values.into_iter().map(Into::into).collect())
}

#[cfg(not(feature = "allow_empty"))]
#[rune::function(instance)]
fn values(count: i64, value: Value) -> DataDescription {
    let Ok(count) = rune::to_value(count) else {
        return DataDescription::Error(RuGenError::CountMustBeNonNegative);
    };
    DataDescription::Array {
        len: Box::new(DataDescription::Just(count)),
        item: Box::new(value.into()),
    }
}

#[cfg(feature = "allow_empty")]
#[rune::function(instance)]
fn values(count: i64, value: Value) -> DataDescription {
    let Ok(count) = rune::to_value(count) else {
        return DataDescription::Array {
            len: Box::new(DataDescription::Just(0)),
            item: Box::new(DataDescription::Just(Value::Unit)),
        };
    };
    DataDescription::Array {
        len: Box::new(DataDescription::Just(count)),
        item: Box::new(value.into()),
    }
}

#[rune::function(instance)]
fn choose(values: Vec<Value>) -> DataDescription {
    DataDescription::OneOf(values.into_iter().map(Into::into).collect())
}

#[rune::function(instance)]
fn pick(values: Vec<Value>) -> DataDescription {
    DataDescription::OneOf(values.into_iter().map(Into::into).collect())
}

#[rune::function(path = pick)]
fn pick_one(values: Vec<Value>) -> DataDescription {
    DataDescription::OneOf(values.into_iter().map(Into::into).collect())
}

#[rune::function]
fn weighted(values: Vec<(u32, Value)>) -> DataDescription {
    DataDescription::Weighted(values.into_iter().map(|(w, v)| (w, v.into())).collect())
}

#[rune::function]
fn array(len: Value, item: Value) -> DataDescription {
    DataDescription::Array {
        len: Box::new(len.into()),
        item: Box::new(item.into()),
    }
}

#[rune::function]
fn object(obj: Object) -> DataDescription {
    DataDescription::Object(obj)
}

#[rune::function]
fn optional(p: Value, item: Value) -> DataDescription {
    DataDescription::Optional {
        p: Box::new(p.into()),
        item: Box::new(item.into()),
    }
}

#[rune::function]
fn tuple(items: Vec<Value>) -> DataDescription {
    DataDescription::Tuple(items.into_iter().map(Into::into).collect())
}

fn clone_rune_string(s: &RuneString) -> Result<RuneString, RuntimeError> {
    let mut new_str = RuneString::new();
    new_str
        .try_push_str(s.as_str())
        .map_err(|e| RuntimeError::panic(e.to_string()))?;
    Ok(new_str)
}

fn generate_impl(this: DataDescription) -> Result<Value, RuGenError> {
    let mut rng = rand::rng();
    match this {
        DataDescription::Error(e) => Err(e),
        DataDescription::Just(v) => Ok(v.clone()),
        DataDescription::AlphaNumeric { len } => {
            let s: String = rng
                .sample_iter(rand::distr::Alphanumeric)
                .take(generate_impl(*len)?.as_usize()?)
                .map(char::from)
                .collect();
            Ok(rune::to_value(s)?)
        }
        DataDescription::String { len, min, max } => {
            let s: String = (0..generate_impl(*len)?.as_usize()?)
                .map(|_| rng.random_range(min..max))
                .collect();
            Ok(rune::to_value(s)?)
        }
        DataDescription::OneOf(values) => {
            let mut rng = rand::rng();
            values.choose(&mut rng).map_or_else(
                || Err(RuGenError::NoValueToChooseFrom),
                |v| generate_impl(v.clone()),
            )
        }
        DataDescription::Array { len, item } => Ok(rune::to_value(
            (0..generate_impl(*len)?.as_usize()?)
                .map(|_| generate_impl(*item.clone()))
                .collect::<Result<Vec<Value>, RuGenError>>()?,
        )?),
        DataDescription::Object(obj) => {
            let mut new_obj = Object::new();
            for (k, v) in &obj {
                new_obj.insert(clone_rune_string(k)?, generate_impl(v.into())?)?;
            }
            Ok(rune::to_value(new_obj)?)
        }
        DataDescription::Optional { p, item } => {
            let mut rng = rand::rng();
            Ok(rune::to_value(
                (rng.random::<f64>() < generate_impl(*p)?.as_float()?)
                    .then(|| generate_impl(*item))
                    .transpose()?,
            )?)
        }
        DataDescription::Tuple(values) => {
            let mut v = Vec::new();
            for desc in values {
                v.push(generate_impl(desc)?);
            }
            Ok(rune::to_value(v)?)
        }
        DataDescription::Bool => Ok(rune::to_value(rng.random::<bool>())?),
        DataDescription::UInt { min, max } => Ok(rune::to_value(rng.random_range(min..max))?),
        DataDescription::Int { min, max } => Ok(rune::to_value(rng.random_range(min..max))?),
        DataDescription::Char { min, max } => Ok(rune::to_value(rng.random_range(min..max))?),
        DataDescription::Float { min, max } => Ok(rune::to_value(rng.random_range(min..max))?),
        DataDescription::Weighted(items) => Ok(rune::to_value(
            items.choose_weighted(&mut rng, |v| v.0).map_or_else(
                |_| Err(RuGenError::NoValueToChooseFrom),
                |(_, v)| generate_impl(v.clone()),
            )?,
        )?),
    }
}

#[rune::function]
fn describe(this: Value) -> Result<Value, RuGenError> {
    let desc: DataDescription = this.into();
    generate_impl(desc)
}

#[rune::function(instance, path = to_description)]
fn generate_object(this: Object) -> Result<Value, RuGenError> {
    generate_impl(DataDescription::Object(this))
}

#[rune::function(instance, path = to_description)]
fn generate_vec(this: Vec<Value>) -> Result<Vec<Value>, RuGenError> {
    this.into_iter()
        .map(|v| generate_impl(DataDescription::from(v)))
        .collect::<Result<Vec<Value>, RuGenError>>()
}

#[rune::function(instance, path = to_description)]
fn generate(this: DataDescription) -> Result<Value, RuGenError> {
    generate_impl(this)
}

pub fn module() -> Result<Module, ContextError> {
    let mut m = Module::with_item(["rugen"])?;
    m.ty::<DataDescription>()?;
    m.function_meta(range)?;
    m.function_meta(just)?;
    m.function_meta(literal)?;
    m.function_meta(bool)?;
    m.function_meta(char)?;
    m.function_meta(uint)?;
    m.function_meta(int)?;
    m.function_meta(float)?;
    m.function_meta(alphanumeric)?;
    m.function_meta(string)?;
    m.function_meta(one_of)?;
    m.function_meta(choose_one)?;
    m.function_meta(choose)?;
    m.function_meta(pick_one)?;
    m.function_meta(pick)?;
    m.function_meta(weighted)?;
    m.function_meta(array)?;
    m.function_meta(object)?;
    m.function_meta(optional)?;
    m.function_meta(tuple)?;
    m.function_meta(generate)?;
    m.function_meta(generate_object)?;
    m.function_meta(generate_vec)?;
    m.function_meta(describe)?;
    m.function_meta(values)?;
    Ok(m)
}

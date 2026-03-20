use std::{any::type_name, collections::BTreeMap, path::Path};

pub use rune;

use rand::{
    RngExt,
    seq::{IndexedRandom, WeightError},
};
use rune::{
    Any, ContextError, FromValue, Module, ToConstValue, Value,
    alloc::{self, Result, String as RuneString},
    runtime::{
        Object, Protocol, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RuntimeError,
    },
};

#[cfg(feature = "fmt")]
use rune::{Diagnostics, Source, Sources};

#[cfg(feature = "fmt")]
pub fn format_rune_script(script: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut sources = Sources::new();

    sources.insert(match Source::from_path(script) {
        Ok(source) => source,
        Err(error) => return Err(Box::new(error)),
    })?;

    let mut diagnostics = Diagnostics::new();

    let options = rune::Options::default();

    let build = rune::fmt::prepare(&sources)
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
    std::fs::write(script, formatted)?;
    Ok(())
}

#[derive(Any, thiserror::Error, Debug)]
pub enum DescriptionError {
    #[error("Invalid range start {0}")]
    InvalidRangeStart(String),
    #[error("Invalid range end {0}")]
    InvalidRangeEnd(String),
    #[error("Unsupported type {0}")]
    UnsupportedType(String),
    #[error("Vec must have at least one value to choose from {0}")]
    NoValueToChooseFrom(String),
    #[error("Min and max of range must be of the same type {0}")]
    MinMaxTypeMismatch(String),
    #[error("Invalid probability: {0}! Value must be between 0 and 1 inclusive {1}")]
    InvalidProbability(String, f64),
    #[error("Count must be non-negative {0}")]
    CountMustBeNonNegative(String),
    #[error("Could not convert value to expected type: {0} {1}")]
    ConversionError(String, String),
}

#[derive(Any, thiserror::Error, Debug)]
pub enum EvaluationError {
    #[error("Vec must have at least one value to choose from")]
    NoValueToChooseFrom,
    #[error("RuntimeError: {0} {1}")]
    RuntimeError(RuntimeError, String),
    #[error("alloc::Error: {0} {1}")]
    AllocError(alloc::Error, String),
    #[error("WeightError: {0} {1}")]
    WeightError(WeightError, String),
}

#[derive(Any, thiserror::Error, Debug)]
pub enum RuGenError {
    #[error("{0}")]
    DescriptionError(#[from] DescriptionError),
    #[error("{0}")]
    EvaluationError(#[from] EvaluationError),
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

#[derive(Any)]
enum Marker {
    Bool,
    Range {
        min: Value,
        max: Value,
        inclusive: bool,
    },
    String {
        len: Value,
    },
    Choice(Vec<Value>),
    Weighted(Vec<(u32, Value)>),
    FixedLengthArray {
        count: i64,
        value: Value,
    },
    VariableLengthArray {
        count: Value,
        value: Value,
    },
    Optional {
        p: Value,
        value: Value,
    },
}

#[expect(clippy::too_many_lines)]
fn evaluate_inner(
    description: &DataDescription,
    mut path: PathHelper,
) -> Result<Value, EvaluationError> {
    let mut rng = rand::rng();
    match description {
        DataDescription::Just(v) => Ok(v.clone()),
        DataDescription::String { len } => {
            let s: String = rng
                .sample_iter(rand::distr::Alphanumeric)
                .take(
                    evaluate_inner(len, path.join("<len>".to_string()))?
                        .as_usize()
                        .map_err(|e| {
                            EvaluationError::RuntimeError(e, path_to_location_string(&path))
                        })?,
                )
                .map(char::from)
                .collect();
            Ok(rune::to_value(s)
                .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?)
        }
        DataDescription::Choice(values) => {
            if values.is_empty() {
                return Err(EvaluationError::NoValueToChooseFrom);
            }
            let mut rng = rand::rng();
            let i = rng.random_range(0..values.len());
            evaluate_inner(&values[i], path.join(i))
        }
        DataDescription::VariableLengthArray { count, value } => Ok(rune::to_value(
            (0..evaluate_inner(count, path.join("<count>".to_string()))?
                .as_usize()
                .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?)
                .map(|i| evaluate_inner(value, path.join(i)))
                .collect::<Result<Vec<Value>, EvaluationError>>()?,
        )
        .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?),
        #[expect(clippy::cast_sign_loss)]
        #[expect(clippy::cast_possible_truncation)]
        DataDescription::FixedLengthArray { count, value } => Ok(rune::to_value(
            (0..*count)
                .map(|i| evaluate_inner(value, path.join(i as usize)))
                .collect::<Result<Vec<Value>, EvaluationError>>()?,
        )
        .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?),
        DataDescription::Object(obj) => {
            let mut new_obj = Object::new();
            for (k, v) in obj {
                let mut new_str = RuneString::new();
                new_str
                    .try_push_str(k)
                    .map_err(|e| EvaluationError::AllocError(e, path_to_location_string(&path)))?;
                new_obj
                    .insert(
                        new_str,
                        evaluate_inner(v, path.join(k.as_str().to_string()))?,
                    )
                    .map_err(|e| EvaluationError::AllocError(e, path_to_location_string(&path)))?;
            }
            Ok(rune::to_value(new_obj)
                .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?)
        }
        DataDescription::Optional { p, value } => {
            let mut rng = rand::rng();
            Ok(rune::to_value(
                (rng.random::<f64>() < *p)
                    .then(|| evaluate_inner(value, path.join("?".to_string())))
                    .transpose()?,
            )
            .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?)
        }
        DataDescription::Vec(values) => {
            let mut v = Vec::new();
            for (i, desc) in values.iter().enumerate() {
                v.push(evaluate_inner(desc, path.join(i))?);
            }
            Ok(rune::to_value(v)
                .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?)
        }
        DataDescription::Bool => Ok(rune::to_value(rng.random::<bool>())
            .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?),
        DataDescription::UInt {
            min,
            max,
            inclusive,
        } => Ok(rune::to_value(if *inclusive {
            rng.random_range(*min..=*max)
        } else {
            rng.random_range(*min..*max)
        })
        .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?),
        DataDescription::Int {
            min,
            max,
            inclusive,
        } => Ok(rune::to_value(if *inclusive {
            rng.random_range(*min..=*max)
        } else {
            rng.random_range(*min..*max)
        })
        .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?),
        DataDescription::Char {
            min,
            max,
            inclusive,
        } => Ok(rune::to_value(if *inclusive {
            rng.random_range(*min..=*max)
        } else {
            rng.random_range(*min..*max)
        })
        .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?),
        DataDescription::Float {
            min,
            max,
            inclusive,
        } => Ok(rune::to_value(if *inclusive {
            rng.random_range(*min..=*max)
        } else {
            rng.random_range(*min..*max)
        })
        .map_err(|e| EvaluationError::RuntimeError(e, path_to_location_string(&path)))?),
        DataDescription::Weighted(values) => {
            let indexed = values.iter().enumerate().collect::<Vec<_>>();
            let (i, _) = indexed
                .choose_weighted(&mut rng, |v| v.1.0)
                .map_err(|e| EvaluationError::WeightError(e, path_to_location_string(&path)))?;
            Ok(
                rune::to_value(evaluate_inner(&values[*i].1, path.join(*i))?).map_err(|e| {
                    EvaluationError::RuntimeError(e, path_to_location_string(&path))
                })?,
            )
        }
    }
}

pub fn evaluate(description: &DataDescription) -> Result<Value, EvaluationError> {
    let path = PathHelper {
        root: true,
        inner: &mut Vec::new(),
    };
    evaluate_inner(description, path)
}

pub fn checked_from_value<T: FromValue>(value: &Value) -> Result<T, DescriptionError> {
    checked_from_value_inner(
        value,
        &PathHelper {
            root: true,
            inner: &mut Vec::new(),
        },
    )
}

fn checked_from_value_inner<T: FromValue>(
    value: &Value,
    path: &PathHelper,
) -> Result<T, DescriptionError> {
    let res = if let Ok(v) = rune::from_value::<Result<Value, DescriptionError>>(value) {
        rune::from_value(v?)
    } else {
        rune::from_value(value.to_owned())
    };
    res.map_err(|_| {
        DescriptionError::ConversionError(
            type_name::<T>().to_owned(),
            path_to_location_string(path),
        )
    })
}

fn path_to_location_string(path: &PathHelper) -> String {
    format!(
        "(path: /{})",
        path.inner
            .iter()
            .map(|s| match s {
                Segment::USize(i) => format!("@~[{i}]~@"),
                Segment::Key(k) => k.clone(),
            })
            .collect::<Vec<_>>()
            .join("/")
            .replace("/@~[", "[")
            .replace("]~@/", "]/")
            .replace("]~@", "]")
    )
}

fn range_impl(
    min: &Value,
    max: &Value,
    inclusive: bool,
    path: &PathHelper,
) -> Result<DataDescription, DescriptionError> {
    if min.type_info() != max.type_info() {
        return Err(DescriptionError::MinMaxTypeMismatch(
            path_to_location_string(path),
        ));
    }
    match min {
        min if min.as_integer::<u64>().is_ok() => {
            let min = min
                .as_integer::<u64>()
                .map_err(|_| DescriptionError::InvalidRangeStart(path_to_location_string(path)))?;
            let max = max
                .as_integer::<u64>()
                .map_err(|_| DescriptionError::InvalidRangeEnd(path_to_location_string(path)))?;

            Ok(DataDescription::UInt {
                min,
                max,
                inclusive,
            })
        }
        min if min.as_integer::<i64>().is_ok() => {
            let min = min
                .as_integer::<i64>()
                .map_err(|_| DescriptionError::InvalidRangeStart(path_to_location_string(path)))?;
            let max = max
                .as_integer::<i64>()
                .map_err(|_| DescriptionError::InvalidRangeEnd(path_to_location_string(path)))?;
            Ok(DataDescription::Int {
                min,
                max,
                inclusive,
            })
        }
        min if min.as_float().is_ok() => {
            let min = min
                .as_float()
                .map_err(|_| DescriptionError::InvalidRangeStart(path_to_location_string(path)))?;
            let max = max
                .as_float()
                .map_err(|_| DescriptionError::InvalidRangeEnd(path_to_location_string(path)))?;
            Ok(DataDescription::Float {
                min,
                max,
                inclusive,
            })
        }
        min if min.as_char().is_ok() => {
            let min = min
                .as_char()
                .map_err(|_| DescriptionError::InvalidRangeStart(path_to_location_string(path)))?;
            let max = max
                .as_char()
                .map_err(|_| DescriptionError::InvalidRangeEnd(path_to_location_string(path)))?;
            Ok(DataDescription::Char {
                min,
                max,
                inclusive,
            })
        }
        _ => Err(DescriptionError::UnsupportedType(path_to_location_string(
            path,
        ))),
    }
}

#[rune::function]
fn range(min: Value, max: Value) -> Marker {
    Marker::Range {
        min,
        max,
        inclusive: false,
    }
}

#[rune::function]
fn range_inclusive(min: Value, max: Value) -> Marker {
    Marker::Range {
        min,
        max,
        inclusive: true,
    }
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

fn try_build_from_marker_inner(
    desc: &Marker,
    path: &mut PathHelper,
) -> Result<DataDescription, DescriptionError> {
    match desc {
        Marker::Bool => Ok(DataDescription::Bool),
        Marker::String { len } => Ok(DataDescription::String {
            len: Box::new(try_build_description_inner(len, path.join("<len>"))?),
        }),
        Marker::Range {
            min,
            max,
            inclusive,
        } => range_impl(min, max, *inclusive, path),
        Marker::Choice(values) => {
            if values.is_empty() {
                Err(DescriptionError::NoValueToChooseFrom(
                    path_to_location_string(path),
                ))
            } else {
                Ok(DataDescription::Choice(
                    values
                        .iter()
                        .enumerate()
                        .map(|(i, v)| try_build_description_inner(v, path.join(i)))
                        .collect::<Result<Vec<DataDescription>, DescriptionError>>()?,
                ))
            }
        }
        Marker::Weighted(values) => Ok(DataDescription::Weighted(
            values
                .iter()
                .enumerate()
                .map(|(i, (w, v))| try_build_description_inner(v, path.join(i)).map(|v| (*w, v)))
                .collect::<Result<Vec<(u32, DataDescription)>, DescriptionError>>()?,
        )),
        Marker::FixedLengthArray { count, value } => Ok(DataDescription::FixedLengthArray {
            count: *count,
            value: Box::new(try_build_description_inner(
                value,
                path.join("<value>".to_string()),
            )?),
        }),
        Marker::VariableLengthArray { count, value } => Ok(DataDescription::VariableLengthArray {
            count: Box::new(try_build_description_inner(
                count,
                path.join("<count>".to_string()),
            )?),
            value: Box::new(try_build_description_inner(
                value,
                path.join("<value>".to_string()),
            )?),
        }),
        Marker::Optional { p, value } => {
            let p = checked_from_value_inner(p, path)?;
            if !(0.0..=1.0).contains(&p) {
                return Err(DescriptionError::InvalidProbability(
                    path_to_location_string(path),
                    p,
                ));
            }
            Ok(DataDescription::Optional {
                p,
                value: Box::new(try_build_description_inner(
                    value,
                    path.join("<value>".to_string()),
                )?),
            })
        }
    }
}

fn try_build_description_inner(
    value: &Value,
    mut path: PathHelper,
) -> Result<DataDescription, DescriptionError> {
    let value = checked_from_value_inner(value, &path)?;
    if let Ok(desc) = rune::from_value::<DataDescription>(&value) {
        Ok(desc)
    } else if let Ok(desc) = rune::from_value::<Result<Marker, DescriptionError>>(&value) {
        Ok(try_build_description_inner(
            &rune::to_value(desc?).expect("Marker should always be able to convert to value"),
            path,
        )?)
    } else if let Ok(desc) = rune::from_value::<Marker>(&value) {
        try_build_from_marker_inner(&desc, &mut path)
    } else if let Ok(obj) = rune::from_value::<Object>(&value) {
        Ok(DataDescription::Object(
            obj.into_iter()
                .map(|(k, v)| {
                    try_build_description_inner(&v, path.join(k.as_str().to_string()))
                        .map(|v| (k.as_str().to_string(), v))
                })
                .collect::<Result<_, _>>()?,
        ))
    } else if let Ok(range) = rune::from_value::<Range>(&value) {
        range_impl(&range.start, &range.end, false, &path)
    } else if let Ok(range) = rune::from_value::<RangeInclusive>(&value) {
        range_impl(&range.start, &range.end, true, &path)
    } else if let Ok(range) = rune::from_value::<RangeFrom>(&value) {
        let max = value_max(&range.start).ok_or(DescriptionError::UnsupportedType(
            path_to_location_string(&path),
        ))?;
        range_impl(&range.start, &max, true, &path)
    } else if let Ok(range) = rune::from_value::<RangeTo>(&value) {
        let min = value_min(&range.end).ok_or(DescriptionError::UnsupportedType(
            path_to_location_string(&path),
        ))?;
        range_impl(&min, &range.end, false, &path)
    } else if rune::from_value::<RangeFull>(&value).is_ok() {
        Err(DescriptionError::UnsupportedType(path_to_location_string(
            &path,
        )))
    } else if let Ok(s) = rune::from_value::<Vec<Value>>(&value) {
        Ok(DataDescription::Vec(
            s.into_iter()
                .enumerate()
                .map(|(i, v)| try_build_description_inner(&v, path.join(i)))
                .collect::<Result<_, _>>()?,
        ))
    } else {
        Ok(DataDescription::Just(value))
    }
}

pub fn try_build_description(value: &Value) -> Result<DataDescription, DescriptionError> {
    try_build_description_inner(
        value,
        PathHelper {
            inner: &mut Vec::new(),
            root: true,
        },
    )
}

#[rune::function]
fn bool() -> Marker {
    Marker::Bool
}

#[rune::function]
fn string(len: Value) -> Marker {
    Marker::String { len }
}

#[rune::function]
fn choose(values: Vec<Value>) -> Marker {
    Marker::Choice(values)
}

#[rune::function(instance)]
fn values(count: i64, value: Value) -> Marker {
    Marker::FixedLengthArray { count, value }
}

#[rune::function(path = values)]
fn variable_values(count: Value, value: Value) -> Marker {
    Marker::VariableLengthArray { count, value }
}

#[rune::function]
fn weighted(values: Vec<(u32, Value)>) -> Marker {
    Marker::Weighted(values)
}

#[rune::function]
fn optional(p: Value, value: Value) -> Marker {
    Marker::Optional { p, value }
}

#[rune::function]
#[expect(clippy::needless_pass_by_value)]
fn describe(this: Value) -> Result<DataDescription, DescriptionError> {
    try_build_description(&this)
}

fn bit_or(left: Object, right: Value) -> Marker {
    let left = rune::to_value(left).unwrap();
    if let Ok(mut right_vec) = rune::from_value::<Marker>(&right) {
        match &mut right_vec {
            Marker::Choice(values) => {
                values.push(left);
                Marker::Choice(values.clone())
            }
            _ => Marker::Choice(vec![left, right]),
        }
    } else {
        Marker::Choice(vec![left, right])
    }
}

fn bit_or_marker(left: Marker, right: Value) -> Marker {
    match left {
        Marker::Choice(mut values) => {
            values.push(right);
            Marker::Choice(values)
        }
        _ => Marker::Choice(vec![rune::to_value(left).unwrap(), right]),
    }
}

fn mul_range(value: Range, count: Value) -> Marker {
    let value = rune::to_value(value).expect("Range should always be convertible to Value");
    if let Ok(i) = rune::from_value::<i64>(&value) {
        Marker::FixedLengthArray {
            count: i,
            value: rune::to_value(i).expect("i64 should always be convertible to Value"),
        }
    } else {
        Marker::VariableLengthArray { count, value }
    }
}

fn mul_alpha(_: Alpha, len: Value) -> Marker {
    Marker::String { len }
}

#[derive(Any, ToConstValue)]
struct Alpha {}

pub fn module() -> Result<Module, ContextError> {
    let mut m = Module::with_item(["rugen"])?;
    m.ty::<Marker>()?;
    m.ty::<DataDescription>()?;
    m.ty::<Alpha>()?;
    m.constant("ALPHA", Alpha {}).build()?;
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

    m.associated_function(&Protocol::MUL, mul_range)?;
    m.associated_function(&Protocol::MUL, mul_alpha)?;
    m.associated_function(&Protocol::BIT_OR, bit_or)?;
    m.associated_function(&Protocol::BIT_OR, bit_or_marker)?;
    Ok(m)
}

enum Segment {
    USize(usize),
    Key(String),
}

trait ToSegment {
    fn to_segment(self) -> Segment;
}

impl ToSegment for usize {
    fn to_segment(self) -> Segment {
        Segment::USize(self)
    }
}

impl ToSegment for &str {
    fn to_segment(self) -> Segment {
        Segment::Key(self.to_string())
    }
}

impl ToSegment for String {
    fn to_segment(self) -> Segment {
        Segment::Key(self)
    }
}

struct PathHelper<'a> {
    root: bool,
    inner: &'a mut Vec<Segment>,
}

impl PathHelper<'_> {
    fn join<T: ToSegment>(&mut self, segment: T) -> PathHelper<'_> {
        self.inner.push(segment.to_segment());
        PathHelper {
            inner: self.inner,
            root: false,
        }
    }
}

impl Drop for PathHelper<'_> {
    fn drop(&mut self) {
        if !self.root {
            self.inner.pop();
        }
    }
}

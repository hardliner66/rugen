use std::{any::type_name, collections::BTreeMap, path::Path};

pub use rune;

use rand::{
    RngExt,
    seq::{IndexedRandom, WeightError},
};
use rune::{
    Any, ContextError, FromValue, Module, ToConstValue, Value,
    alloc::{self, Result, String as RuneString},
    macros::Quote,
    runtime::{
        Object, Protocol, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RuntimeError,
    },
};

#[cfg(feature = "fmt")]
use rune::{Diagnostics, Source, Sources};

use rune::T;
use rune::ast;
use rune::compile;
use rune::macros::{MacroContext, TokenStream, quote};
use rune::parse::Parser;

fn parse_expr<'a>(expr: ast::Expr) -> Quote<'a> {
    match expr {
        ast::Expr::Binary(ast::ExprBinary {
            lhs,
            op: ast::BinOp::Mul(_),
            rhs,
            ..
        }) => {
            quote!(rugen::make_description(#lhs * #rhs, line!())?)
        }
        ast::Expr::Object(o) => {
            let mut result = quote!();
            for a in o.assignments {
                let name = a.0.key;
                if let Some((_, expr)) = a.0.assign {
                    let rhs = parse_expr(expr);
                    result = quote!(#result #name: #rhs,);
                }
            }
            quote!(#{#result})
        }
        ast::Expr::Vec(v) => {
            let mut result = quote!();
            for a in v.items {
                let v = parse_expr(a.0);
                result = quote!(#result #v,);
            }
            quote!([#result])
        }
        ast::Expr::Range(r) => {
            quote!(rugen::make_description(#r, line!())?)
        }
        value => quote!(#value),
    }
}

#[rune::macro_]
fn describe(
    cx: &mut MacroContext<'_, '_, '_>,
    stream: &TokenStream,
) -> compile::Result<TokenStream> {
    let mut parser = Parser::from_token_stream(stream, cx.input_span());

    let mut fields = Vec::new();

    while !parser.is_eof()? {
        let key = if parser.peek::<ast::Ident>()? {
            let ident = parser.parse::<ast::Ident>()?;
            quote!(#ident)
        } else {
            let lit = parser.parse::<ast::LitStr>()?;
            quote!(#lit)
        };

        parser.parse::<T![:]>()?;

        let value = parse_expr(parser.parse::<ast::Expr>()?);

        fields.push((key, value));

        if parser.parse::<Option<T![,]>>()?.is_none() {
            break;
        }
    }

    parser.eof()?;

    let mut object_tokens = quote!();
    for (i, (key, value)) in fields.iter().enumerate() {
        if i > 0 {
            object_tokens = quote!(#object_tokens,); // Add a comma between fields
        }
        object_tokens = quote!(#object_tokens #key: #value);
    }

    Ok(quote!(rugen::make_description(#{#object_tokens}, line!())?).into_token_stream(cx)?)
}

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
    #[error("Invalid range start{}", .0.map_or("".to_string(), |v| format!(" at line {v}")))]
    InvalidRangeStart(Option<i64>),
    #[error("Invalid range end{}", .0.map_or("".to_string(), |v| format!(" at line {v}")))]
    InvalidRangeEnd(Option<i64>),
    #[error("Unsupported type{}", .0.map_or("".to_string(), |v| format!(" at line {v}")))]
    UnsupportedType(Option<i64>),
    #[error("Vec must have at least one value to choose from{}", .0.map_or("".to_string(), |v| format!(" at line {v}")))]
    NoValueToChooseFrom(Option<i64>),
    #[error("Min and max of range must be of the same type{}", .0.map_or("".to_string(), |v| format!(" at line {v}")))]
    MinMaxTypeMismatch(Option<i64>),
    #[error("Invalid probability: {}{}", .0, .1.map_or("".to_string(), |v| format!(" at line {v}")))]
    InvalidProbability(f64, Option<i64>),
    #[error("Count must be non-negative{}", .0.map_or("".to_string(), |v| format!(" at line {v}")))]
    CountMustBeNonNegative(Option<i64>),
    #[error("Could not convert value to expected type: {}{}", .0, .1.map_or("".to_string(), |v| format!(" at line {v}")))]
    ConversionError(String, Option<i64>),
}

#[derive(Any, thiserror::Error, Debug)]
pub enum EvaluationError {
    #[error("Vec must have at least one value to choose from")]
    NoValueToChooseFrom,
    #[error("RuntimeError: {0}")]
    RuntimeError(#[from] RuntimeError),
    #[error("alloc::Error: {0}")]
    AllocError(#[from] alloc::Error),
    #[error("WeightError: {0}")]
    WeightError(#[from] WeightError),
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
fn generate_inner(description: &DataDescription) -> Result<Value, EvaluationError> {
    let mut rng = rand::rng();
    match description {
        DataDescription::Just(v) => Ok(v.clone()),
        DataDescription::String { len } => {
            let s: String = rng
                .sample_iter(rand::distr::Alphanumeric)
                .take(
                    generate_inner(len)?
                        .as_usize()
                        .map_err(|e| EvaluationError::RuntimeError(e))?,
                )
                .map(char::from)
                .collect();
            Ok(rune::to_value(s).map_err(|e| EvaluationError::RuntimeError(e))?)
        }
        DataDescription::Choice(values) => {
            if values.is_empty() {
                return Err(EvaluationError::NoValueToChooseFrom);
            }
            let mut rng = rand::rng();
            let i = rng.random_range(0..values.len());
            generate_inner(&values[i])
        }
        DataDescription::VariableLengthArray { count, value } => Ok(rune::to_value(
            (0..generate_inner(count)?
                .as_usize()
                .map_err(|e| EvaluationError::RuntimeError(e))?)
                .map(|_| generate_inner(value))
                .collect::<Result<Vec<Value>, EvaluationError>>()?,
        )
        .map_err(|e| EvaluationError::RuntimeError(e))?),
        #[expect(clippy::cast_sign_loss)]
        #[expect(clippy::cast_possible_truncation)]
        DataDescription::FixedLengthArray { count, value } => Ok(rune::to_value(
            (0..*count)
                .map(|_| generate_inner(value))
                .collect::<Result<Vec<Value>, EvaluationError>>()?,
        )
        .map_err(|e| EvaluationError::RuntimeError(e))?),
        DataDescription::Object(obj) => {
            let mut new_obj = Object::new();
            for (k, v) in obj {
                let mut new_str = RuneString::new();
                new_str
                    .try_push_str(k)
                    .map_err(|e| EvaluationError::AllocError(e))?;
                new_obj
                    .insert(new_str, generate_inner(v)?)
                    .map_err(|e| EvaluationError::AllocError(e))?;
            }
            Ok(rune::to_value(new_obj).map_err(|e| EvaluationError::RuntimeError(e))?)
        }
        DataDescription::Optional { p, value } => {
            let mut rng = rand::rng();
            Ok(rune::to_value(
                (rng.random::<f64>() < *p)
                    .then(|| generate_inner(value))
                    .transpose()?,
            )
            .map_err(|e| EvaluationError::RuntimeError(e))?)
        }
        DataDescription::Vec(values) => {
            let mut v = Vec::new();
            for desc in values.iter() {
                v.push(generate_inner(desc)?);
            }
            Ok(rune::to_value(v).map_err(|e| EvaluationError::RuntimeError(e))?)
        }
        DataDescription::Bool => {
            Ok(rune::to_value(rng.random::<bool>())
                .map_err(|e| EvaluationError::RuntimeError(e))?)
        }
        DataDescription::UInt {
            min,
            max,
            inclusive,
        } => Ok(rune::to_value(if *inclusive {
            rng.random_range(*min..=*max)
        } else {
            rng.random_range(*min..*max)
        })
        .map_err(|e| EvaluationError::RuntimeError(e))?),
        DataDescription::Int {
            min,
            max,
            inclusive,
        } => Ok(rune::to_value(if *inclusive {
            rng.random_range(*min..=*max)
        } else {
            rng.random_range(*min..*max)
        })
        .map_err(|e| EvaluationError::RuntimeError(e))?),
        DataDescription::Char {
            min,
            max,
            inclusive,
        } => Ok(rune::to_value(if *inclusive {
            rng.random_range(*min..=*max)
        } else {
            rng.random_range(*min..*max)
        })
        .map_err(|e| EvaluationError::RuntimeError(e))?),
        DataDescription::Float {
            min,
            max,
            inclusive,
        } => Ok(rune::to_value(if *inclusive {
            rng.random_range(*min..=*max)
        } else {
            rng.random_range(*min..*max)
        })
        .map_err(|e| EvaluationError::RuntimeError(e))?),
        DataDescription::Weighted(values) => {
            let indexed = values.iter().enumerate().collect::<Vec<_>>();
            let (i, _) = indexed
                .choose_weighted(&mut rng, |v| v.1.0)
                .map_err(|e| EvaluationError::WeightError(e))?;
            Ok(rune::to_value(generate_inner(&values[*i].1)?)
                .map_err(|e| EvaluationError::RuntimeError(e))?)
        }
    }
}

#[rune::function]
pub fn generate(description: DataDescription) -> Result<Value, EvaluationError> {
    generate_inner(&description)
}

#[rune::function(instance, path = generate)]
pub fn generate_instance(description: DataDescription) -> Result<Value, EvaluationError> {
    generate_inner(&description)
}

pub fn checked_from_value<T: FromValue>(value: &Value) -> Result<T, DescriptionError> {
    checked_from_value_inner(value, None)
}

fn checked_from_value_inner<T: FromValue>(
    value: &Value,
    line: Option<i64>,
) -> Result<T, DescriptionError> {
    let res = if let Ok(v) = rune::from_value::<Result<Value, DescriptionError>>(value) {
        rune::from_value(v?)
    } else {
        rune::from_value(value.to_owned())
    };
    res.map_err(|e| {
        println!("{:?}", e);
        DescriptionError::ConversionError(type_name::<T>().to_owned(), line)
    })
}

fn range_impl(
    min: &Value,
    max: &Value,
    inclusive: bool,
    line: Option<i64>,
) -> Result<DataDescription, DescriptionError> {
    if min.type_info() != max.type_info() {
        return Err(DescriptionError::MinMaxTypeMismatch(line));
    }
    match min {
        min if min.as_integer::<u64>().is_ok() => {
            let min = min
                .as_integer::<u64>()
                .map_err(|_| DescriptionError::InvalidRangeStart(line))?;
            let max = max
                .as_integer::<u64>()
                .map_err(|_| DescriptionError::InvalidRangeEnd(line))?;

            Ok(DataDescription::UInt {
                min,
                max,
                inclusive,
            })
        }
        min if min.as_integer::<i64>().is_ok() => {
            let min = min
                .as_integer::<i64>()
                .map_err(|_| DescriptionError::InvalidRangeStart(line))?;
            let max = max
                .as_integer::<i64>()
                .map_err(|_| DescriptionError::InvalidRangeEnd(line))?;
            Ok(DataDescription::Int {
                min,
                max,
                inclusive,
            })
        }
        min if min.as_float().is_ok() => {
            let min = min
                .as_float()
                .map_err(|_| DescriptionError::InvalidRangeStart(line))?;
            let max = max
                .as_float()
                .map_err(|_| DescriptionError::InvalidRangeEnd(line))?;
            Ok(DataDescription::Float {
                min,
                max,
                inclusive,
            })
        }
        min if min.as_char().is_ok() => {
            let min = min
                .as_char()
                .map_err(|_| DescriptionError::InvalidRangeStart(line))?;
            let max = max
                .as_char()
                .map_err(|_| DescriptionError::InvalidRangeEnd(line))?;
            Ok(DataDescription::Char {
                min,
                max,
                inclusive,
            })
        }
        _ => Err(DescriptionError::UnsupportedType(line)),
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
    line: Option<i64>,
) -> Result<DataDescription, DescriptionError> {
    match desc {
        Marker::Bool => Ok(DataDescription::Bool),
        Marker::String { len } => Ok(DataDescription::String {
            len: Box::new(try_build_description_inner(len, line)?),
        }),
        Marker::Range {
            min,
            max,
            inclusive,
        } => Ok(range_impl(min, max, *inclusive, line)?),
        Marker::Choice(values) => {
            if values.is_empty() {
                Err(DescriptionError::NoValueToChooseFrom(line))
            } else {
                Ok(DataDescription::Choice(
                    values
                        .iter()
                        .map(|v| try_build_description_inner(v, line))
                        .collect::<Result<Vec<DataDescription>, DescriptionError>>()?,
                ))
            }
        }
        Marker::Weighted(values) => Ok(DataDescription::Weighted(
            values
                .iter()
                .map(|(w, v)| try_build_description_inner(v, line).map(|v| (*w, v)))
                .collect::<Result<Vec<(u32, DataDescription)>, DescriptionError>>()?,
        )),
        Marker::FixedLengthArray { count, value } => Ok(DataDescription::FixedLengthArray {
            count: *count,
            value: Box::new(try_build_description_inner(value, line)?),
        }),
        Marker::VariableLengthArray { count, value } => Ok(DataDescription::VariableLengthArray {
            count: Box::new(try_build_description_inner(count, line)?),
            value: Box::new(try_build_description_inner(value, line)?),
        }),
        Marker::Optional { p, value } => {
            let p = checked_from_value_inner(p, line)?;
            if !(0.0..=1.0).contains(&p) {
                return Err(DescriptionError::InvalidProbability(p, line));
            }
            Ok(DataDescription::Optional {
                p,
                value: Box::new(try_build_description_inner(value, line)?),
            })
        }
    }
}

fn try_build_description_inner(
    value: &Value,
    line: Option<i64>,
) -> Result<DataDescription, DescriptionError> {
    let value = checked_from_value_inner(value, line)?;
    if let Ok(desc) = rune::from_value::<DataDescription>(&value) {
        Ok(desc)
    } else if let Ok(desc) = rune::from_value::<Result<Marker, DescriptionError>>(&value) {
        Ok(try_build_description_inner(
            &rune::to_value(desc?).expect("Marker should always be able to convert to value"),
            line,
        )?)
    } else if let Ok(desc) = rune::from_value::<Marker>(&value) {
        try_build_from_marker_inner(&desc, line)
    } else if let Ok(obj) = rune::from_value::<Object>(&value) {
        Ok(DataDescription::Object(
            obj.into_iter()
                .map(|(k, v)| {
                    try_build_description_inner(&v, line).map(|v| (k.as_str().to_string(), v))
                })
                .collect::<Result<_, _>>()?,
        ))
    } else if let Ok(range) = rune::from_value::<Range>(&value) {
        Ok(range_impl(&range.start, &range.end, false, line)?)
    } else if let Ok(range) = rune::from_value::<RangeInclusive>(&value) {
        Ok(range_impl(&range.start, &range.end, true, line)?)
    } else if let Ok(range) = rune::from_value::<RangeFrom>(&value) {
        let max = value_max(&range.start).ok_or(DescriptionError::UnsupportedType(line))?;
        Ok(range_impl(&range.start, &max, true, line)?)
    } else if let Ok(range) = rune::from_value::<RangeTo>(&value) {
        let min = value_min(&range.end).ok_or(DescriptionError::UnsupportedType(line))?;
        Ok(range_impl(&min, &range.end, false, line)?)
    } else if rune::from_value::<RangeFull>(&value).is_ok() {
        Err(DescriptionError::UnsupportedType(line))
    } else if let Ok(s) = rune::from_value::<Vec<Value>>(&value) {
        Ok(DataDescription::Vec(
            s.into_iter()
                .map(|v| try_build_description_inner(&v, line))
                .collect::<Result<_, _>>()?,
        ))
    } else {
        Ok(DataDescription::Just(value))
    }
}

pub fn try_build_description(
    value: &Value,
    line: Option<i64>,
) -> Result<DataDescription, DescriptionError> {
    try_build_description_inner(value, line)
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
fn make_description(this: Value, line: i64) -> Result<DataDescription, DescriptionError> {
    try_build_description(&this, Some(line))
}

#[rune::function]
fn to_description(this: Value) -> Result<DataDescription, DescriptionError> {
    try_build_description(&this, None)
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

    m.macro_meta(describe)?;

    m.constant("ALPHA", Alpha {}).build()?;
    m.function_meta(make_description)?;
    m.function_meta(to_description)?;
    m.function_meta(generate_instance)?;
    m.function_meta(generate)?;
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

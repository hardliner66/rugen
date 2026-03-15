# RuGen (Rune data Generator)

The data generation dsl used in [mockbox](https://github.com/hardliner66/mockbox).

## Installation CLI

### Pre-Built Binaries (via [cargo-binstall](https://github.com/cargo-bins/cargo-binstall))

```sh
cargo binstall rugen-cli
```

### Pre-Built Binaries (manual download)

You can download pre-built binaries from the [latest release](https://github.com/hardliner66/rugen/releases).

### From Source

1. Clone the repository
2. Build the project:

```bash
cargo install rugen-cli
```

### As Library

`RuGen` also provides a module that can be installed into your own `Rune` vm. To use it go to your project and run:

```sh
cargo add rugen
```

For an example on how set up `RuGen` with `Rune`, take a look at the how the CLI does it:
[crates/rugen-cli/src/main.rs](./crates/rugen-cli/src/main.rs).

## How it works

`RuGen` uses the [Rune](https://rune-rs.github.io/) scripting language to build a an internal
representation of the data you want to produce. We call this a `DataDescription`. Once that's done,
the description can be used to generate random data, following the limits and options you chose while building it.

Most of this is done by looking at the data types of the individual values in the context of
a data description and interpreting them accordingly. This way, you mostly just need to write
objects, vectors and ranges, and the library takes care of the rest.

Of course it's also possible to be explicit about it, so you can find your own perfect balance between
explicitness and magic.

I would highly recommend using the implicit api, because there is less clutter and less functions to remember.

## Explicit Example

First, lets have a look at the explicit way of describing your data. This should give you a bit of how things work
and you see how the implicit api works under the hood (at least conceptually).

```rs
use rugen as r;
pub fn main() {
    r::describe(#{
        asdf: r::uint(1, 10),
        values: r::array(r::just(5), r::float(55.0, 128.0)),
        choice: r::one_of(
            [
                r::object(#{ A: r::uint(100, 200) }),
                r::object(#{ B: r::int(-100, 100) }),
                r::object(#{ C: r::float(0.5, 2.5) }),
                r::object(#{ D: r::alphanumeric(r::just(10)) }),
            ],
        ),
    })?
}
```

You start a description with `describe`, then you use the appropriate functions to compose your data description from its building blocks.
`just` represents the value itself, so `just(10)` will always evaluate to `10`. You can also use `literal`, which is an alias of `just`, if you prefer.
`uint` evaluates to a random unsigned integer, that's at least as big as the first number you pass and at most as big as the last number minus one.

Each part has its own function, making it perfectly clear what kind of data you can expect.

To try this example locally, make sure you have rugen-cli installed, then either clone the repository or download the fire from [examples/explicit.rn](./examples/explicit.rn), then you can run it with `rugen path/to/explicit.rn`.

## Implicit Example

Now, lets have a look at the implicit way of describing data. Normally, you would use the `describe` function like in the example before,
but because our CLI is only meant execute data descriptions, we can just return a rune object and the CLI will automatically call `describe` on it
for us. Just keep in mind that you when you use `RuGen` as a library, you either need to add the `describe` call to your rune scripts or call it
yourself when you get the result of the script execution.

As you can see below, most of the functions from the first example are gone. This is because we know what type of almost every value and
inside the CLI or by passing data to describe, we know that the data should be interpreted as a data description and not concrete data.

This makes it possible to derive what needs to be done, depending on the type of data we see, making it possible to do more with less work,
while still staying relatively readable. This allows less technical people use the tooling as well, without having to learn a bunch of functions.

You probably also noticed, that all the type information is gone. The reason for that is relatively simple: It's not needed most of the time.
Especially if you always use a lower and an upper limit in a range (`lower_limit..upper_limit`). The tool itself also currently only outputs
data as JSON, so there is no need to have the limits from the ranges *AND* the limits from the data type at the same time.

```rs
use rugen::*;

pub fn main() {
    #{
        asdf: 1..10,
        values: 5.values(55.0..128.0),
        range_from: 100..,
        range_to: ..100,
        choice: [
            #{ A: 100..=200 },
            #{ B: -100..100 },
            #{ C: 0.5..2.5 },
            #{ D: alphanumeric(10) },
        ].pick(),
    }
}
```

As you can see, there is way less noise from helper functions. Behind the scenes it still gets packaged into the same data description,
just with less work involved. The implicit api also supports half open ranges (`..<max>` or `<min>..`), which you can use when there only is a
upper or lower limit, but not both. This is currently not implemented in the explicit api.

There is also the full range (`..`) which would give you every value from lowest to highest possible,
but is currently not implemented as well. The reason for that is, that there is no way to know if the type of the full range is signed, unsigned
or a float, so we also can't determine which numbers to use.

To try this example locally, make sure you have rugen-cli installed, then either clone the repository or download the fire from [examples/implicit.rn](./examples/implicit.rn), then you can run it with `rugen path/to/implicit.rn`.

## API

```rs
// creates a description that evaluates to the passed value
rugen::just(value: rune::Value) -> DataDescription;

// creates a description that evaluates to a random boolean
rugen::bool() -> DataDescription;

// creates a description that evaluates to a random u64 between <min> and <max> (exclusive)
rugen::uint(min: u64, max:  u64) -> DataDescription;

// creates a description that evaluates to a random i64 between <min> and <max> (exclusive)
rugen::int(min: i64, max:  i64) -> DataDescription;

// creates a description that evaluates to a random f64 between <min> and <max> (exclusive)
rugen::float(min: f32, max: f32) -> DataDescription;

// creates a description that evaluates to a string of random alpha numeric characters that is <len> long
rugen::alphanumeric(len: DataDescription) -> DataDescription;

// creates a description that evaluates to a string of random characters between <min> and <max> (exclusive)
rugen::string(min: usize, max: usize) -> DataDescription;

// creates a description that evaluates to a random value from the passed vec
rugen::one_of(values: Vec<DataDescription>) -> DataDescription;

// creates a description that evaluates to a weighted random value from the passed vec
rugen::weighted(values: Vec<(u32, DataDescription)>) -> DataDescription;

// creates a description that evaluates to a vec of length <len>, filled with values defined by <item>
rugen::array(len: DataDescription, item: DataDescription) -> DataDescription;

// creates a description that evaluates to a an object
rugen::object(fields: HashMap<String, DataDescription>) -> DataDescription;

// creates a description that has a 0.0 < p < 1.0 chance to evaluate to an optional value defined by <item>
rugen::optional(p: DataDescription, item: DataDescription) -> DataDescription;

// creates a description that takes all items in a vec and evaluates them to values, according to their description
rugen::tuple(items: Vec<DataDescription>) -> DataDescription;

// evaluates a given description
rugen::generate(&self) -> Result<rune::Value>;
```

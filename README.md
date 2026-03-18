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
        asdf: r::range(1, 10),
        values: r::array(5, r::range(55.0, 128.0)),
        choice: r::choose(
            [
                r::object(#{ A: r::range(100, 200) }),
                r::object(#{ B: r::range(-100, 100) }),
                r::object(#{ C: r::range(0.5, 2.5) }),
                r::object(#{ D: r::string(10) }),
            ],
        ),
    })
}
```

You start a description with `describe`, then you use the appropriate functions to compose your data description from its building blocks.
Using a value directly represents the value itself, so `10` will always evaluate to `10`. `range` evaluates to a random value, according to the values you pass.

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
use rugen as r;

pub fn main() {
    #{
        asdf: 1..10,
        values: 5.values(55.0..128.0),
        range_from: 100..,
        range_to: ..100,
        choice: r::choose([
            #{ A: 100..=200 },
            #{ B: -100..100 },
            #{ C: 0.5..2.5 },
            #{ D: r::string(10) },
        ]),
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
// converts a possibly recursive value into the corresponding DataDescription
rugen::describe(value: rune::Value) -> DataDescription;

// creates a description that evaluates to a random boolean
rugen::bool() -> DataDescription;

// creates a description that evaluates to an alphanumeric string of length <len>
rugen::string(len: DataDescription) -> DataDescription;

// creates a description that has a 0.0 < p < 1.0 chance to evaluate to an optional value defined by <value>
rugen::optional(p: DataDescription, value: DataDescription) -> DataDescription;

// creates a description that evaluates to a random Value between <min> and <max> (exclusive)
rugen::range(min: rune::Value, max: rune::Value) -> DataDescription;

// creates a description that evaluates to a random Value between <min> and <max> (inclusive)
rugen::range_inclusive(min: rune::Value, max: rune::Value) -> DataDescription;

// creates a description that picks a random value from the passed vec
rugen::choose(values: Vec<DataDescription>) -> DataDescription;

// creates a description that picks a random value from the passed vec,
// while taking the weights into account
rugen::weighted(values: Vec<(u32, DataDescription)>) -> DataDescription;

// creates a description that produces a list of values by evaluating <value> for <count> times
// this variant of values uses a fixed number as a count, which makes it produce the same number of values every time
<count:u64>.values(value: DataDescription) -> DataDescription;

// creates a description that produces a list of values by first evaluating <count> and then evaluating <value> for <count> times
// this variant of values can take a DataDescription as its count, producing as many values as count evaluates to
rugen::values(count: DataDescription, value: DataDescription) -> DataDescription;
```

# Rust Fundamentals for Experienced Programmers

## Purpose

This is a detailed Rust walkthrough for someone who already knows how to
program well, but is new to Rust specifically.

The audience I am assuming is:

- strong in Kotlin, Python, and Java
- comfortable with C and C++
- interested in understanding not just *how* Rust works, but *why* it was
designed this way

This is not a full language reference. It is a mental-model guide focused on
the core ideas you need in order to read, write, debug, and reason about Rust
code with confidence.

The main theme of Rust is:

> Give C/C++-class control and performance, but move memory-safety and
> concurrency-safety checks into the type system and compiler.

---

## 1. What Rust Is Trying To Achieve

Rust is easiest to understand when you see it as a response to a familiar
problem:

- C and C++ give great control and performance, but put a lot of safety burden
on the programmer.
- Java, Kotlin, and Python remove much of that burden with GC and higher-level
runtime models, but you lose some control over memory layout, lifetime,
destruction timing, and low-level performance tuning.

Rust's answer is:

- no garbage collector
- deterministic cleanup via RAII
- move semantics by default
- aliasing and mutability rules enforced by the compiler
- expressive algebraic data types (`enum`)
- powerful generics and traits
- explicit error handling instead of exception-driven control flow

### A useful one-line mental model

Rust is roughly:

- C++ move semantics + RAII
- Kotlin sealed/data-class style expressiveness
- Java-style interface abstraction, but stronger and more compile-time oriented
- Python-level convenience in some APIs
- a compiler that aggressively checks ownership and aliasing

### The major trade

Rust makes you do more work up front:

- you think about ownership
- you think about borrowing
- you think about mutability explicitly
- you think about success/failure types explicitly

In return, the compiler catches a large class of bugs that would otherwise show
up:

- as crashes in C/C++
- as race conditions in multithreaded code
- as `null`/state bugs in Java/Kotlin
- as runtime surprises in Python

---

## 2. Rust Compared To Languages You Already Know


| Topic              | Rust                                        | Kotlin / Java                              | Python                                            | C / C++                                 |
| ------------------ | ------------------------------------------- | ------------------------------------------ | ------------------------------------------------- | --------------------------------------- |
| Memory management  | No GC; ownership + RAII                     | GC                                         | GC + refcounting details hidden                   | Manual / RAII / smart pointers          |
| Nullability        | `Option<T>`; no implicit nulls in safe code | Kotlin has nullable types, Java has `null` | `None` everywhere, dynamically checked            | Raw pointers may be null                |
| Error handling     | `Result<T, E>` and `?`                      | Exceptions                                 | Exceptions                                        | Return codes, exceptions, errno, ad hoc |
| Polymorphism       | Traits, generics, trait objects             | Interfaces, classes, generics              | Duck typing                                       | Templates, virtual dispatch             |
| Sum types          | Native `enum` with data                     | Kotlin sealed classes approximate it       | No native equivalent                              | `std::variant` or ad hoc unions         |
| Concurrency safety | Data-race freedom enforced in safe code     | Thread-safe libs, but type system weaker   | GIL in CPython, no general compile-time safety    | Powerful but easy to get wrong          |
| Mutation model     | Immutable by default, explicit `mut`        | `val` / `var`                              | Everything is a reference, mutation by convention | Free-form                               |


### Where Rust will feel familiar

- If you know C++, `move`, `RAII`, `destructor`, `reference`, `value semantics`,
`template-like specialization`, and zero-cost abstractions will feel natural.
- If you know Kotlin, `match`-style exhaustive branching, data-rich enums, and
explicit null-handling via `Option` will feel conceptually clean.
- If you know Java, traits will partly resemble interfaces, and `Result`/`Option`
will feel like stronger, more central versions of `Optional`.

### Where Rust will feel strange at first

- references are not just aliases; they come with borrowing rules
- mutability and aliasing are deeply connected
- ownership is part of API design, not an implementation detail
- many "obvious" Java/Python/C++ patterns are intentionally discouraged

---

## 3. Tooling And Project Structure

Before the language itself, you need the Rust ecosystem basics.

### 3.1 Core tools

- `rustup`: installs and manages Rust toolchains
- `rustc`: the compiler
- `cargo`: build tool, package manager, test runner, formatter driver, doc tool
- `rustfmt`: formatter
- `clippy`: lint tool
- `rustdoc`: documentation generator

### 3.2 Typical commands

```bash
cargo build
cargo check
cargo test
cargo run
cargo fmt
cargo clippy
cargo doc --open
```

### 3.3 Why `cargo check` matters

`cargo check` type-checks without producing a final optimized binary, so it is
much faster for iteration. This is the Rust equivalent of "compile often, fail
fast" and is one of the most important everyday commands.

### 3.4 Crates, modules, packages, workspaces

- **crate**: a compilation unit
- **package**: what `Cargo.toml` describes; may contain one library crate and/or
binary crates
- **workspace**: multiple packages managed together
- **module**: a namespace inside a crate

If you are coming from:

- Java/Kotlin: think "package + module tree", but driven by files and `mod`
declarations, not by class hierarchy
- Python: think modules/packages, but with strict compilation and visibility
- C++: think translation units and namespaces, but much more unified

### 3.5 Editions

Rust has language editions like `2018`, `2021`, `2024`. These are not separate
languages; they are compatibility/versioning milestones that let syntax and
defaults improve without breaking everything forever.

---

## 4. Syntax And Core Expression Model

Rust syntax is C-family, but the semantics often feel more expression-oriented.

### 4.1 Variables and mutability

```rust
let x = 10;
let mut y = 20;
y += 1;
```

- `let` binds a name
- names are immutable by default
- `mut` makes the binding mutable

This is similar to:

- Kotlin: `val` vs `var`
- Java: everything is mutable by default unless made `final`
- C++: local variables are mutable by default

Rust's default of immutability is not just style. It reduces accidental state
changes and makes ownership/borrowing easier to reason about.

### 4.2 Shadowing

```rust
let x = 5;
let x = x + 1;
let x = x * 2;
```

This is called **shadowing**. You are not mutating the same variable; you are
creating a new binding with the same name.

Why it is useful:

- type-changing transformations become ergonomic
- you can keep a simple name through a sequence of conversions
- it is often cleaner than introducing `tmp1`, `tmp2`, `tmp3`

This is more common and more idiomatic in Rust than in Java/C++.

### 4.3 Expressions vs statements

Rust is strongly expression-oriented.

```rust
let n = if flag { 1 } else { 2 };
```

`if` is an expression here, not just a statement.

Likewise:

```rust
let value = {
    let base = 10;
    base + 5
};
```

The last line without a semicolon becomes the value of the block.

This feels closer to:

- Kotlin
- functional languages

than to Java or C.

### 4.4 Functions

```rust
fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

Rust function signatures are explicit and intentionally compact.

Important details:

- parameters are always typed
- return type comes after `->`
- last expression can be returned implicitly

### 4.5 Control flow

```rust
if cond {
    // ...
} else if other {
    // ...
} else {
    // ...
}

for x in items {
    // ...
}

while cond {
    // ...
}

loop {
    break;
}
```

`for` in Rust is closer to "for each" than classic C-style indexing loops.
Rust intentionally does not have the C/Java `for (init; cond; step)` syntax.

---

## 5. Types: The Foundations You Will Use Constantly

### 5.1 Primitive scalar types

Common numeric types:

- signed: `i8`, `i16`, `i32`, `i64`, `i128`, `isize`
- unsigned: `u8`, `u16`, `u32`, `u64`, `u128`, `usize`
- floating point: `f32`, `f64`
- boolean: `bool`
- character: `char`

Rust does **not** silently choose a universal integer type in the way Python
effectively does. Numeric precision and size are explicit.

### 5.2 Tuples

```rust
let p: (i32, i32) = (3, 4);
let (x, y) = p;
```

Tuples are lightweight fixed-size product types. They are used constantly for
small multi-value returns.

### 5.3 Arrays and slices

```rust
let a: [i32; 3] = [1, 2, 3];
let s: &[i32] = &a[..];
```

- `[T; N]`: fixed-size array
- `&[T]`: borrowed slice view into contiguous elements

Think:

- C++: `std::array<T, N>` and `std::span<T>`
- Java/Kotlin: array plus a read-only view, but with much stronger static rules

### 5.4 Structs

```rust
struct Point {
    x: i32,
    y: i32,
}
```

This is familiar to C structs, Kotlin data classes, or Java records, except
that methods live in `impl` blocks rather than inside the type definition.

### 5.5 Enums

Rust enums are much more powerful than C enums.

```rust
enum Shape {
    Circle { radius: f64 },
    Rectangle { w: f64, h: f64 },
    Point,
}
```

This is one of Rust's most important features.

Conceptually:

- closer to Kotlin sealed classes
- much better integrated than Java class hierarchies for this use case
- more ergonomic than C++ `std::variant` in everyday code

### 5.6 `Option<T>`

```rust
let maybe_name: Option<String> = Some(String::from("Ada"));
let missing: Option<String> = None;
```

This is Rust's answer to nullable values.

Instead of:

- Java `null`
- Kotlin `T?`
- Python `None`
- C++ nullable pointers

Rust makes absence explicit in the type system.

This is a major design decision. It means "might not exist" is not an informal
comment or convention; it is part of the function contract.

### 5.7 `Result<T, E>`

```rust
fn parse_port(s: &str) -> Result<u16, std::num::ParseIntError> {
    s.parse()
}
```

`Result` means "either success with `T` or failure with `E`".

This replaces large amounts of exception-heavy design with explicit control
flow.

---

## 6. Ownership: The Central Rust Idea

If you only remember one Rust concept, remember this one.

### 6.1 The rule

Every value has an owner. At any point in time:

- a value has one owning binding
- when the owner goes out of scope, the value is dropped
- ownership can move to another binding

Example:

```rust
let s1 = String::from("hello");
let s2 = s1;
// s1 is no longer usable here
```

For `String`, this is a move, not a deep copy.

### 6.2 Why Rust does this

Rust wants:

- deterministic cleanup
- no double-free
- no use-after-free
- no hidden GC

Ownership is the compile-time model that makes this possible.

### 6.3 Compare with C++

This is closest to:

- move semantics
- `std::unique_ptr`
- RAII

In fact, a good first approximation is:

> In Rust, many ordinary values behave like move-only RAII-managed objects by
> default.

The big difference is that Rust applies this discipline pervasively and makes it
central to the language, not just one advanced library/tooling feature.

### 6.4 Compare with Java/Kotlin/Python

In Java/Kotlin/Python, variables usually hold references to GC-managed objects.
Assignment mostly copies the reference, not ownership.

Rust is different:

- assignment may transfer ownership
- destruction timing is deterministic
- aliasing is controlled rather than assumed

### 6.5 `Copy` vs `Clone`

Some types are cheap and safe to duplicate by bit-copy.

Examples:

- integers
- booleans
- small plain value types that implement `Copy`

```rust
let a = 5;
let b = a; // a is still usable because i32 is Copy
```

For heap-owning or nontrivial types like `String`, assignment usually moves.

If you want an explicit duplicate:

```rust
let s1 = String::from("hello");
let s2 = s1.clone();
```

Important mindset:

- `Copy` is implicit and cheap
- `Clone` is explicit and potentially expensive

### 6.6 `Drop`

When a value goes out of scope, Rust runs cleanup automatically.

This is RAII, just like C++ destructors, but structured through ownership and
the `Drop` trait.

---

## 7. Borrowing: Access Without Taking Ownership

Owning everything would be too rigid. So Rust lets you **borrow** values.

### 7.1 Shared borrows

```rust
fn len(s: &String) -> usize {
    s.len()
}
```

`&T` means: a shared borrowed reference to `T`.

Better API style is usually:

```rust
fn len(s: &str) -> usize {
    s.len()
}
```

because `&str` is more general than `&String`.

### 7.2 Mutable borrows

```rust
fn add_bang(s: &mut String) {
    s.push('!');
}
```

`&mut T` means: a temporary exclusive borrow that allows mutation.

### 7.3 The key aliasing rule

In safe Rust, at a given time you can have either:

- many shared references (`&T`)
- or one mutable reference (`&mut T`)

but not both at once.

This is the famous rule.

The deeper meaning is:

> Aliasing and mutation cannot coexist freely.

That rule is what lets Rust prevent many races and invalid states at compile
time.

### 7.4 Why this feels restrictive

Because in Java/Python/C++, aliasing is easy:

- many variables can refer to the same object
- mutation is often unconstrained
- correctness becomes a runtime discipline

Rust instead says:

- if you want mutation, prove exclusivity
- if you want many readers, mutation must pause

### 7.5 API design consequence

Function signatures encode permissions:

- `T`: takes ownership
- `&T`: read-only borrow
- `&mut T`: exclusive mutable borrow

This is one of Rust's best features. APIs document not just *type shape*, but
*resource/permission shape*.

### 7.6 Slices are borrowed views

Prefer:

- `&str` over `&String`
- `&[T]` over `&Vec<T>`

because slices/views express "I only need read access to contiguous data", not
"I require this exact container type".

This is similar to preferring interfaces over concrete classes in Java, or
`string_view` / `span`-style APIs in modern C++.

---

## 8. Lifetimes: The Part Everyone Fears

Lifetimes are less mystical than they first appear.

### 8.1 What lifetimes really are

Lifetimes are not usually about the actual runtime duration of an object. They
are a way to describe relationships between references so the compiler can prove
they are valid.

Example:

```rust
fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() >= y.len() { x } else { y }
}
```

This does **not** mean "these strings live forever". It means:

- the returned reference is valid for at most the shared overlap of the input
reference lifetimes

### 8.2 Why lifetimes exist

Without them, the compiler could not verify that returned references do not
dangle.

Rust wants you to be able to write zero-cost borrowed APIs safely. Lifetimes are
the static bookkeeping system that enables this.

### 8.3 Compare with C++

In C++, you can return references and iterators very freely, but validity is
largely on you. Rust asks you to state enough structure that the compiler can
check it.

### 8.4 Compare with GC languages

Java/Kotlin/Python usually avoid this problem because object reachability is
managed by a runtime GC. Reference validity is not expressed in the type system
the same way.

### 8.5 When you often do not write lifetimes explicitly

Rust has **lifetime elision** rules, so many functions do not need explicit
annotations.

```rust
fn first_word(s: &str) -> &str {
    // ...
#   s
}
```

The compiler can infer the relationship in many common cases.

### 8.6 When you *do* need explicit lifetimes

Mostly when:

- multiple input references are involved
- a returned reference could come from more than one input
- a struct stores borrowed data

Example:

```rust
struct Parser<'a> {
    input: &'a str,
}
```

This means `Parser` does not own the string; it borrows it.

### 8.7 Practical advice

When lifetimes become painful, ask:

1. Should this function return an owned value instead?
2. Should this struct own its data instead of borrowing it?
3. Can I shorten the borrow?
4. Am I trying to keep a reference around longer than necessary?

Beginners often try to "fight" lifetimes. A better strategy is to redesign the
ownership shape of the code.

---

## 9. Strings: One Of The First Practical Hurdles

Rust string handling trips up many experienced developers because it is precise.

### 9.1 `String` vs `&str`

- `String`: owned, growable UTF-8 buffer
- `&str`: borrowed UTF-8 string slice

This is a crucial distinction.

Think:

- `String` ~= `std::string` with ownership
- `&str` ~= `std::string_view`, but compiler-checked for validity

Compared with Kotlin/Java:

- Kotlin/Java `String` is immutable and GC-managed
- Rust `String` is mutable and owned
- `&str` often feels closer to "borrowed text view"

### 9.2 Why APIs prefer `&str`

```rust
fn greet(name: &str) {
    println!("hello, {name}");
}
```

This accepts:

- string literals
- `&String`
- substrings/slices

It is more general than taking `&String`.

### 9.3 UTF-8 matters

Rust strings are UTF-8. That means indexing by integer is not allowed:

```rust
let s = String::from("hello");
// let c = s[0]; // invalid
```

Why? Because a Unicode character may occupy multiple bytes. Rust refuses to
pretend byte indexing is character indexing.

This is a design choice in favor of correctness over convenience.

---

## 10. Collections And Core Standard Types

### 10.1 `Vec<T>`

Rust's primary growable array.

```rust
let mut v = vec![1, 2, 3];
v.push(4);
```

Think:

- Java `ArrayList<T>`
- Kotlin `MutableList<T>`
- C++ `std::vector<T>`

### 10.2 `HashMap<K, V>` and `BTreeMap<K, V>`

- `HashMap`: hash-based lookup
- `BTreeMap`: ordered map

### 10.3 `Box<T>`

A heap-allocated owning pointer with single ownership.

Closest analogy:

- `std::unique_ptr<T>`

Use it when:

- you need heap allocation
- you need recursive types
- you want to move ownership cheaply

### 10.4 `Rc<T>` and `Arc<T>`

- `Rc<T>`: single-threaded shared ownership with reference counting
- `Arc<T>`: thread-safe atomic shared ownership

These are more like:

- C++ `shared_ptr`

than Java references. The sharing is explicit, not the default.

### 10.5 `Cell<T>` and `RefCell<T>`

These provide **interior mutability**.

- `Cell<T>`: simple copy-in/copy-out style mutation
- `RefCell<T>`: borrow checking enforced at runtime instead of compile time

These are escape hatches for designs where compile-time borrowing is too rigid.

Important rule:

> If you reach for `RefCell` everywhere, you are probably importing a
> Java/Python object-graph mindset into Rust instead of redesigning around
> ownership.

### 10.6 `Mutex<T>` and `RwLock<T>`

Use these for synchronized shared mutable state across threads.

Common pattern:

```rust
use std::sync::{Arc, Mutex};

let shared = Arc::new(Mutex::new(vec![1, 2, 3]));
```

---

## 11. `Option` And `Result`: No Hidden Nulls, No Hidden Exceptions

### 11.1 Working with `Option`

```rust
let value = maybe_number.unwrap_or(0);
```

Useful methods:

- `map`
- `and_then`
- `unwrap_or`
- `unwrap_or_else`
- `is_some`
- `is_none`

### 11.2 Pattern matching

```rust
match maybe_name {
    Some(name) => println!("{name}"),
    None => println!("missing"),
}
```

### 11.3 Working with `Result`

```rust
fn read_port(s: &str) -> Result<u16, std::num::ParseIntError> {
    let n: u16 = s.parse()?;
    Ok(n)
}
```

`?` means:

- if success, unwrap and continue
- if error, return early

This is one of the most important and most ergonomic operators in Rust.

### 11.4 Why Rust avoids exception-driven design

Rust prefers errors in the type system because:

- you see failure at the call boundary
- control flow is explicit
- performance/runtime behavior is less magical
- library APIs compose clearly

This can feel verbose at first if you are used to exceptions, but it becomes
very predictable.

### 11.5 `panic!`

`panic!` means unrecoverable failure.

Use it for:

- bugs
- impossible states
- violated internal assumptions

Do **not** use `panic!` as ordinary business-logic error handling.

---

## 12. Pattern Matching And Enums: A Major Rust Superpower

### 12.1 Exhaustiveness

```rust
match shape {
    Shape::Circle { radius } => area_circle(radius),
    Shape::Rectangle { w, h } => w * h,
    Shape::Point => 0.0,
}
```

The compiler checks that all cases are covered.

This is one of Rust's nicest correctness features.

### 12.2 `if let` and `while let`

```rust
if let Some(x) = maybe_x {
    println!("{x}");
}
```

Use these when you only care about one pattern and do not need a full `match`.

### 12.3 Compare with other languages

- Kotlin: similar to `when` on sealed hierarchies
- Java: modern pattern matching is improving, but Rust remains more central and
uniform here
- Python: structural pattern matching exists, but with dynamic typing and less
compile-time guarantee
- C++: can emulate with `std::variant` + `visit`, but it is less direct

---

## 13. Traits And Generics

Traits are one of the most important abstraction mechanisms in Rust.

### 13.1 What a trait is

A trait describes shared behavior.

```rust
trait Area {
    fn area(&self) -> f64;
}
```

Implement it:

```rust
impl Area for Shape {
    fn area(&self) -> f64 {
        match self {
            Shape::Circle { radius } => std::f64::consts::PI * radius * radius,
            Shape::Rectangle { w, h } => w * h,
            Shape::Point => 0.0,
        }
    }
}
```

### 13.2 Compare with Java/Kotlin interfaces

Traits are similar to interfaces, but:

- they participate more deeply in generic constraints
- they support blanket implementations
- they can carry associated types and default methods
- they integrate tightly with static dispatch

### 13.3 Compare with C++ templates

Rust generics are monomorphized in many cases, so performance characteristics
can resemble templates. But Rust has much stronger constraint checking and more
uniform trait bounds than the old "template errors from hell" C++ experience.

### 13.4 Generic functions

```rust
fn print_area<T: Area>(x: &T) {
    println!("{}", x.area());
}
```

Equivalent longer form:

```rust
fn print_area<T>(x: &T)
where
    T: Area,
{
    println!("{}", x.area());
}
```

### 13.5 Trait bounds

You will see these often:

- `T: Clone`
- `T: Debug`
- `T: Send + Sync`
- `T: Iterator<Item = i32>`

### 13.6 Associated types

```rust
trait MyIter {
    type Item;
    fn next(&mut self) -> Option<Self::Item>;
}
```

This is cleaner than forcing callers to specify all type parameters manually.

### 13.7 Static vs dynamic dispatch

Static dispatch:

```rust
fn use_it<T: Area>(x: &T) {
    println!("{}", x.area());
}
```

Dynamic dispatch:

```rust
fn use_dyn(x: &dyn Area) {
    println!("{}", x.area());
}
```

Rough analogy:

- `T: Trait` -> compile-time specialization, like templates/generics with static dispatch
- `&dyn Trait` -> runtime polymorphism, like interface references / virtual dispatch

Use dynamic dispatch when:

- heterogeneous collections are needed
- compile-time type size is unknown
- runtime polymorphism is desired

Use generics when:

- performance matters
- call sites are type-stable
- you want inlining and static optimization

### 13.8 No classical inheritance hierarchy

Rust does not center object-oriented inheritance the way Java/C++ often do.

Instead, Rust prefers:

- composition
- enums for closed sets of variants
- traits for shared behavior

This is one of the biggest mental shifts for Java developers.

---

## 14. Methods, `impl`, And Associated Functions

Methods live in `impl` blocks:

```rust
struct Counter {
    n: usize,
}

impl Counter {
    fn new() -> Self {
        Self { n: 0 }
    }

    fn inc(&mut self) {
        self.n += 1;
    }

    fn get(&self) -> usize {
        self.n
    }
}
```

The receiver tells you the permission model:

- `self`: takes ownership
- `&self`: shared borrow
- `&mut self`: exclusive mutable borrow

This is extremely informative. In Java/Kotlin/Python, `this`/`self` usually does
not communicate ownership/borrowing permissions this clearly.

---

## 15. Iterators And Closures

### 15.1 Iterators are lazy

```rust
let evens: Vec<i32> = nums
    .iter()
    .copied()
    .filter(|n| n % 2 == 0)
    .collect();
```

Rust iterators are:

- composable
- lazy
- often zero-cost after optimization

This is conceptually similar to:

- Kotlin sequences
- Java streams
- Python generator pipelines
- C++ ranges/algorithms

but with ownership and borrowing rules integrated into the design.

### 15.2 `iter`, `iter_mut`, `into_iter`

This trio matters a lot:

- `iter()` -> iterate by shared reference
- `iter_mut()` -> iterate by mutable reference
- `into_iter()` -> consume the collection and iterate by value

This is classic Rust: iteration style and ownership style are coupled.

### 15.3 Closures

```rust
let factor = 2;
let doubled: Vec<_> = nums.iter().map(|x| x * factor).collect();
```

Rust closures can capture from the environment in different ways:

- by borrow
- by mutable borrow
- by move

The compiler infers a lot, but ownership still matters.

### 15.4 `move` closures

```rust
let s = String::from("hello");
let f = move || println!("{s}");
```

`move` transfers captured values into the closure.

This is especially important for:

- threads
- async tasks
- callback ownership boundaries

### 15.5 The `Fn`, `FnMut`, `FnOnce` family

These traits describe how a closure uses its captures:

- `Fn`: can be called without mutating/moving captures
- `FnMut`: may mutate captured state
- `FnOnce`: may consume captured values

This is one more example of Rust making semantics explicit in the type system.

---

## 16. Modules, Visibility, And Imports

### 16.1 Basic structure

```rust
mod parser;
mod graph;

use crate::graph::Node;
```

Visibility is private by default.

Use `pub` to expose items:

```rust
pub struct Node {
    pub id: usize,
}
```

### 16.2 Why private-by-default matters

Rust encourages hiding implementation details aggressively. This often leads to
better API surfaces and clearer ownership boundaries.

### 16.3 Compare with Java/Kotlin/Python/C++

- Java: package/class-based visibility, but public OO APIs dominate
- Kotlin: similar visibility modifiers, but object/class orientation differs
- Python: "we are all consenting adults"; privacy is mostly convention
- C++: headers/source split and namespace systems are more fragmented

Rust's module system tends to feel cleaner than C++, stricter than Python, and
less class-centric than Java.

---

## 17. Macros: A Quick But Important Introduction

Rust uses macros heavily, but not in the C preprocessor sense.

Examples:

- `println!`
- `vec!`
- `format!`
- `include_str!`
- `#[derive(Debug, Clone)]`

### 17.1 Declarative macros

These are pattern-based code generation tools.

They are powerful, but still token-structured and safer than raw textual macro
substitution in C/C++.

### 17.2 Procedural macros and derives

Custom derive and attribute macros are common in the ecosystem:

- `serde`
- web frameworks
- database tooling

If you come from Java/Kotlin, derive/attribute macros can feel a bit like a more
powerful compile-time annotation system.

You do not need to master macro authoring early, but you do need to be
comfortable using macros.

---

## 18. Error Handling In Practice

### 18.1 Library code vs application code

Common rule of thumb:

- library code should return meaningful `Result<T, E>`
- application binaries can use higher-level error wrappers for convenience

### 18.2 The `?` operator is the normal path

Do not think of `?` as advanced sugar. It is standard Rust error propagation.

### 18.3 `unwrap` and `expect`

```rust
let port = s.parse::<u16>().expect("port must be a valid u16");
```

Use these when:

- failure really is a bug
- startup/config/test assumptions are intentional

Avoid using `unwrap` as a lazy substitute for real error handling in production
logic.

### 18.4 Why many Rust developers like this model

It avoids:

- hidden control flow
- catch-all exception abuse
- unclear failure contracts

The code is more explicit, but it is also easier to audit.

---

## 19. Concurrency And Shared State

Rust's concurrency story is one of its strongest features.

### 19.1 The promise

In safe Rust, data races are prevented by construction.

### 19.2 `Send` and `Sync`

These marker traits express thread-safety properties:

- `Send`: can be transferred to another thread
- `Sync`: shared references can be used from multiple threads

You do not usually implement these manually, but they show up in trait bounds.

### 19.3 Common pattern: `Arc<Mutex<T>>`

```rust
use std::sync::{Arc, Mutex};

let shared = Arc::new(Mutex::new(0));
```

This means:

- `Arc`: shared ownership across threads
- `Mutex`: synchronized interior mutation

### 19.4 Why Rust helps here

In Java/C++, it is easy to accidentally share mutable state too widely.
Rust forces you to make the sharing model explicit:

- exclusive mutable borrow
- shared immutable borrow
- interior mutability under synchronization

### 19.5 Channels

Rust also supports message-passing concurrency via channels. This fits well with
ownership because moving values across channels is natural.

This is philosophically closer to:

- Go-style communication patterns
- actor-ish ownership transfer

than to "everything is a shared object graph".

---

## 20. Async Rust

Async Rust is powerful, but it has a steeper learning curve than synchronous
Rust.

### 20.1 What `async fn` means

An `async fn` returns a future. Calling it does not immediately run all the
work. It constructs a state machine that is driven by an executor.

### 20.2 Compare with other languages

- Kotlin: closest analogy is coroutines
- Python: similar to `async` / `await`, but Rust futures are more explicit and
lower-level
- Java: less like `CompletableFuture` chains, more like structured compiler-
generated state machines

### 20.3 Why async Rust feels hard

Because it combines:

- ownership
- borrowing
- lifetimes
- state machines
- executor/runtime concepts

My advice: learn synchronous Rust first, then async Rust.

### 20.4 What to know early

- async is not automatically parallel
- futures are lazy until polled/awaited
- borrowing across `.await` can be tricky
- `Send` often matters when spawning tasks

---

## 21. Unsafe Rust

Rust has `unsafe`, but it is not "turn off all checks" mode.

### 21.1 What `unsafe` allows

Typical unsafe operations include:

- dereferencing raw pointers
- calling unsafe functions
- accessing mutable statics
- implementing certain low-level abstractions
- FFI boundaries

### 21.2 What `unsafe` does *not* mean

It does not mean:

- the whole language becomes C
- the borrow checker disappears everywhere
- you can ignore all invariants

It means:

> The compiler cannot verify part of this code's safety contract, so the
> programmer must uphold it manually.

### 21.3 Why unsafe exists

Because Rust wants to be usable for:

- systems programming
- operating systems
- low-level libraries
- FFI wrappers
- performance-critical abstractions

Safe Rust is built on top of carefully written unsafe Rust in some places.

### 21.4 Good mindset

Keep unsafe:

- small
- localized
- well-documented
- wrapped in safe abstractions

This is similar to good C++ low-level library design, but Rust makes the safe /
unsafe boundary much clearer.

---

## 22. Rust's Performance Model

Rust is a compiled systems language. Performance is one of its core goals.

### 22.1 Zero-cost abstractions

Rust aims for abstractions that disappear after optimization:

- iterators
- generics
- traits in static-dispatch settings
- algebraic data types

The promise is not "always free in every case", but "abstractions should not
force unnecessary runtime overhead by default".

### 22.2 Monomorphization

Generic code is often specialized per concrete type, similar to C++ templates.
This often improves runtime performance at the cost of larger compile times and
binary size.

### 22.3 Stack vs heap

Rust cares about placement:

- values can live on the stack
- heap allocation is explicit through owning containers like `Box`, `Vec`,
`String`, `Arc`

This is much more visible than in Java/Python, and often more predictable.

### 22.4 Deterministic destruction

Resources are released when owners leave scope, not when a GC decides to run.

This matters for:

- file handles
- locks
- sockets
- memory pressure control
- latency-sensitive systems

---

## 23. Idiomatic Rust Design: How To Avoid Writing "Java In Rust"

Experienced developers often stumble not because Rust is impossible, but because
they import the wrong habits.

### 23.1 Prefer enums over class hierarchies when variants are closed

If you know all variants up front, Rust often wants:

- `enum` + `match`

not:

- base trait + many tiny heap-allocated objects

### 23.2 Prefer composition over inheritance

Rust is not built around subclassing. Lean into:

- plain structs
- helper functions
- traits for shared behavior

### 23.3 Avoid cloning just to silence the borrow checker

Bad beginner move:

- sprinkle `.clone()` everywhere until the compiler stops complaining

Sometimes cloning is right. Often it is a sign that ownership design is not yet
clear.

Instead ask:

- who should own this?
- who only needs to borrow it?
- can I shorten the borrow?
- should I restructure the loop or data flow?

### 23.4 Prefer borrowing APIs at boundaries

Good examples:

- `&str` instead of `&String`
- `&[T]` instead of `&Vec<T>`

This makes APIs more general and less coupled to specific containers.

### 23.5 Return owned data when borrowed return types become painful

Many beginners over-optimize for borrowing. Sometimes the cleanest API is:

- take borrowed input
- return owned output

This reduces lifetime complexity and often costs less than expected.

### 23.6 Keep mutability narrow

Prefer:

- immutable data by default
- short, explicit mutable sections

This aligns with Rust's strengths and leads to easier reasoning.

### 23.7 Embrace the type system

Instead of comments like:

- "this may be absent"
- "this might fail"
- "do not use after close"

try to encode those rules in:

- `Option`
- `Result`
- enums
- ownership/borrowing
- newtypes

---

## 24. Common Beginner Pain Points And How To Think Through Them

### 24.1 "cannot move out of ..."

Usually means:

- you tried to take ownership of something behind a borrow
- or you consumed a value and then tried to use it again

Fixes often involve:

- borrowing instead of moving
- cloning intentionally
- restructuring ownership

### 24.2 "borrowed value does not live long enough"

Usually means:

- you returned or stored a reference that outlives its source
- or you tried to keep a borrow longer than the owner lives

Typical fixes:

- return an owned value
- make the owner live longer
- shorten the borrow

### 24.3 "cannot borrow as mutable because it is also borrowed as immutable"

This is Rust enforcing the aliasing rule.

Typical fixes:

- end the immutable borrow earlier
- split scopes
- collect values first, mutate later
- redesign the loop to avoid overlapping borrows

### 24.4 "trait bound not satisfied"

This means generic code requires some capability you have not provided.

Common examples:

- type does not implement `Clone`
- type does not implement `Debug`
- wrong iterator item type
- missing `Send`/`Sync` in concurrent code

### 24.5 "the compiler is fighting me"

Sometimes true. But often the compiler is revealing:

- ambiguous ownership
- unclear lifetime design
- excessive shared mutable state

A useful habit is to treat compiler errors as design feedback, not just syntax
obstacles.

---

## 25. Reading Rust Code Effectively

When you open unfamiliar Rust code, read in this order:

1. What does each type own?
2. What is borrowed vs owned in function signatures?
3. Which enums encode the important states?
4. Which traits define the abstraction boundary?
5. Where does mutation happen?
6. Where do errors flow with `Result`?

This works better than reading linearly from top to bottom.

In Rust, ownership and trait structure often explain the architecture faster
than control flow alone.

---

## 26. A Minimal Mental Cheat Sheet

### If a function takes...

- `x: T` -> it takes ownership
- `x: &T` -> it reads without taking ownership
- `x: &mut T` -> it mutates with exclusive access

### If a method receiver is...

- `self` -> consumes the receiver
- `&self` -> shared read access
- `&mut self` -> exclusive mutable access

### If a type is...

- `Option<T>` -> value may be absent
- `Result<T, E>` -> operation may fail
- `Box<T>` -> single-owner heap allocation
- `Rc<T>` -> shared ownership, single-threaded
- `Arc<T>` -> shared ownership, multi-threaded
- `RefCell<T>` -> runtime borrow checks
- `Mutex<T>` -> synchronized mutation across threads

### If something hurts...

- borrow checker pain -> rethink ownership boundaries
- lifetime pain -> prefer owned return values or owning structs
- clone explosion -> likely a design smell
- trait-bound confusion -> inspect required capabilities

---

## 27. Suggested Learning Order

If you want to learn Rust efficiently, I recommend this progression:

### Stage 1: Core ownership fluency

Learn until these feel natural:

- ownership
- moves
- borrowing
- `Option`
- `Result`
- structs
- enums
- pattern matching

### Stage 2: Idiomatic everyday Rust

Then learn:

- iterators
- traits
- generics
- modules
- collections
- common std types (`Vec`, `String`, `HashMap`)

### Stage 3: Advanced language model

Then:

- lifetimes in depth
- trait objects vs generics
- interior mutability
- concurrency primitives

### Stage 4: Specialized domains

Finally:

- async
- unsafe
- FFI
- advanced macros
- performance tuning

---

## 28. Final Mental Model

If you already know Kotlin, Python, Java, and C/C++, the cleanest way to carry
Rust in your head is:

- from C++: keep RAII, move semantics, value orientation, performance awareness
- from Kotlin: keep explicit nullability instincts, exhaustive branching, data-
oriented modeling
- from Java: keep respect for interfaces/abstractions, but drop inheritance-
first design
- from Python: keep pragmatism and readability, but expect much stronger static
contracts

Then add the Rust-specific core:

> Ownership determines who is responsible.
> Borrowing determines who may temporarily access.
> Traits determine what behavior is required.
> Enums determine what states are possible.
> `Option` and `Result` determine what may be absent or fail.

Once those ideas click, Rust stops feeling like a hostile language and starts
feeling like a language with a very strong, very consistent internal logic.

---

## 29. What To Read Next In This Repo

After this general Rust guide, the best local follow-up docs in this repository
are:

- `docs/walkthrough/rust_road_router Engine API Reference.md`
- `docs/walkthrough/rust_road_router Algorithm Families.md`
- `docs/walkthrough/CCH Walkthrough.md`

That gives you this progression:

1. Rust as a language
2. `rust_road_router` as a codebase
3. CCH as the main algorithmic pipeline used here


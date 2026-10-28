---
title: "Swift and Convex type conversion"
sidebar_label: "Data Types"
hidden: false
sidebar_position: 5
description: "Customizing and converting types between the Swift app and Convex"
---

## Custom data types

Convex lets you easily express your data in the backend as TypeScript objects,
and can return those objects from queries, mutations and actions. To handle
objects on the Swift side, create `struct` definitions that conform to the
`Decodable` protocol. Usually that’s fairly trivial to do, as any `struct` with
all `Decodable` members can automatically conform.

Consider a Convex query function that returns results like this JavaScript
object:

```tsx
{
  name: "Guardians",
  uniformColors: ["blue", "white", "red"],
  wins: 80n,
  losses: 60n
}
```

That can be represented in Swift using:

```swift
struct BaseballTeam: Decodable {
  let name: String
  let uniformColors: [String]
  @ConvexInt
  var wins: Int
  @ConvexInt
  var losses: Int
}
```

Then you can pass that type as the yielding argument in your subscribe call:

```swift
convex.subscribe(to: "mlb:first_place_team",
               with: ["division": "AL Central"],
           yielding: BaseballTeam.self)
```

The data from the remote function will be deserialized to your custom struct.
Often your use of the type can be inferred from the calling context, and you can
skip the yielding argument.

## Numerical types

Numeric types like `Int` and `Double` are encoded in a special format to ensure
proper interoperation with your TypeScript backend functions. To safely use them
on the Swift side, ensure that you use one of the following property wrappers.

| Type                           | Wrapper                |
| ------------------------------ | ---------------------- |
| `Float` or `Double`            | `@ConvexFloat`         |
| `Float?` or `Double?`          | `@OptionalConvexFloat` |
| `Int` or `Int32` or `Int64`    | `@ConvexInt`           |
| `Int?` or `Int32?` or `Int64?` | `@OptionalConvexInt`   |

Note that `struct` properties with wrappers must be declared as `var`.

## Field name conversion

If your code receives objects with names that you need to or want to translate
to different names, you can use a `CodingKeys` `enum` to specify a mapping of
remote names to names on your struct. For example, imagine a backend function or
API that returns log entries like the following representing when someone came
in and went out:

```tsx
{name: "Bob", in: "2024-10-03 08:00:00", out: "2024-10-03 11:00:00"}
```

That data can’t decode directly into a `struct` because `in` is a keyword in
Swift. We can use `CodingKeys` to give it an alternate name while still
ingesting the data from the original name.

```swift
struct Log: Decodable {
  let name: String
  let inTime: String
  let outTime: String

  enum CodingKeys: String, CodingKey {
    case name
    case inTime = "in"
    case outTime = "out"
  }
}
```

## Putting it all together

In the custom data type example above, JavaScript's `BigInt` type is used in the
backend data by adding a trailing `n` to the `wins` and `losses` values which
lets the Swift code use `Int`. If instead the code used regular
JavaScript `number` types, on the Swift side those would be received as floating
point values and deserialization to `Int` would fail.

If you have a situation like that where `number` is used but by convention it
only contains integer values, you can handle that in your `struct` by using
field name conversion and custom properties to hide the floating point
representation.

```swift
struct BaseballTeam: Decodable {
  let name: String
  let uniformColors: [String]
  @ConvexFloat
  private var internalWins: Double
  @ConvexFloat
  private var internalLosses: Double

  enum CodingKeys: String, CodingKey {
    case name
    case uniformColors
    case internalWins = "wins"
    case internalLosses = "losses"
  }

  // Expose the Double values as Ints
  var wins: Int { Int(internalWins) }
  var losses: Int { Int(internalLosses) }
}
```

The pattern is to store the `Double` values privately and with different names
than the value from the backend. Then add custom properties to provide
the `Int` values.
